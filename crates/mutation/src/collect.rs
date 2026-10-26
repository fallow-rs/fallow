//! Mutant site collection.
//!
//! A site is one expression that the instrumentation wraps in a switch. Each
//! site holds the candidate mutants for that expression. The walk skips the
//! positions where a mutant is invalid or useless: type positions, enum
//! members, decorators, module syntax, directives, assignment targets,
//! property keys, the arguments of logging calls, a `case` label whose
//! literal mutant is equivalent because its body is the `default` body, and
//! the `!=` mutant of a counted `for` loop test (see `counted_loop_test`),
//! the equivalent mutant of a zero comparison on the length of a known
//! array (see `ArrayBindings`), and the `true` mutant of a conditional
//! lookup in an array before `.filter(Boolean)` (see `FilteredLookups`).
//!
//! The logging rule is broad: a `console.*` or `debug(...)` call gives no
//! mutant at all. This covers each expression in its arguments (strings,
//! arithmetic, updates, callbacks), and the call statement itself. An `if`
//! statement with a pure test and only logging calls in its branches gives
//! no mutant either (see `logging_only`).

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    AccessorProperty, ArrowFunctionExpression, AssignmentExpression, AssignmentTarget,
    AssignmentTargetPropertyIdentifier, BinaryExpression, BindingIdentifier, BindingPattern,
    BooleanLiteral, CallExpression, ChainElement, ChainExpression, Class, ConditionalExpression,
    Declaration, Decorator, Directive, DoWhileStatement, ExportAllDeclaration, ExportDeclaration,
    ExportDefaultDeclaration, ExportFromDeclaration, ExportNamedDeclaration, Expression,
    ExpressionStatement, ForStatement, ForStatementInit, ForStatementLeft, FormalParameter,
    FormalParameterRest, Function, IdentifierReference, IfStatement, ImportDeclaration,
    ImportExpression, JSXAttributeValue, LogicalExpression, MemberExpression, MethodDefinition,
    MethodDefinitionKind, ObjectProperty, ObjectPropertyKind, Program, PropertyDefinition,
    PropertyKey, PropertyKind, RegExpLiteral, ReturnStatement, SimpleAssignmentTarget, Statement,
    StringLiteral, SwitchCase, SwitchStatement, TSEnumDeclaration, TSExportAssignment,
    TSExternalModuleDeclaration, TSGlobalDeclaration, TSImportEqualsDeclaration,
    TSInterfaceDeclaration, TSNamespaceDeclaration, TSNamespaceExportDeclaration, TSType,
    TSTypeAliasDeclaration, TSTypeAnnotation, TSTypeName, TSTypeOperatorOperator,
    TSTypeParameterDeclaration, TSTypeParameterInstantiation, TaggedTemplateExpression,
    UnaryExpression, UpdateExpression, VariableDeclaration, VariableDeclarationKind,
    VariableDeclarator, WhileStatement, WithStatement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::scope::ScopeFlags;
use oxc_syntax::symbol::SymbolId;

use crate::{FunctionRef, MutantChange, Operator, is_identifier_byte};

/// One candidate mutant: replace `region` with `replacement`.
pub struct Candidate {
    pub operator: Operator,
    pub region: (u32, u32),
    pub replacement: String,
    pub change: MutantChange,
}

/// One expression that the instrumentation wraps.
pub struct Site {
    pub span: (u32, u32),
    pub candidates: Vec<Candidate>,
    /// The span of the node that anchors the mutant ids: the innermost
    /// function, or the top-level statement.
    pub anchor: (u32, u32),
    pub function: Option<FunctionRef>,
}

/// The collector output.
pub struct Collected {
    /// The sites, ordered by start, then by end descending (outer first).
    pub sites: Vec<Site>,
    /// The start offsets of expression statements. A wrapper that starts a
    /// statement needs a guard against automatic semicolon insertion.
    pub statement_starts: BTreeSet<u32>,
}

struct Frame {
    reference: FunctionRef,
    constructor: bool,
    generator: bool,
}

struct PendingFunction {
    name: Option<String>,
    constructor: bool,
}

pub fn collect(program: &Program<'_>, source: &str) -> Collected {
    let arrays = ArrayBindings::scan(program);
    let lookups = source.contains(FILTER_BOOLEAN);
    let semantic =
        (arrays.is_some() || lookups).then(|| SemanticBuilder::new().build(program).semantic);
    let (known_arrays, filtered_lookups) = match &semantic {
        Some(semantic) => (
            arrays.map_or_else(BTreeSet::new, |arrays| arrays.references(semantic)),
            if lookups {
                FilteredLookups::tests(program, semantic)
            } else {
                BTreeSet::new()
            },
        ),
        None => (BTreeSet::new(), BTreeSet::new()),
    };
    let mut collector = Collector {
        source,
        sites: BTreeMap::new(),
        statement_starts: BTreeSet::new(),
        frames: Vec::new(),
        top_statement: (0, 0),
        pending: None,
        no_wrap: BTreeSet::new(),
        logical_operands: BTreeSet::new(),
        relational_operands: BTreeSet::new(),
        expression_starts: BTreeSet::new(),
        equivalent_labels: BTreeSet::new(),
        counted_loop_tests: BTreeSet::new(),
        known_arrays,
        filtered_lookups,
        const_declaration: false,
        const_initializer: false,
    };
    collector.visit_program(program);
    let mut sites: Vec<Site> = collector.sites.into_values().collect();
    sites.sort_by_key(|site| (site.span.0, std::cmp::Reverse(site.span.1)));
    Collected {
        sites,
        statement_starts: collector.statement_starts,
    }
}

struct Collector<'s> {
    source: &'s str,
    sites: BTreeMap<(u32, u32), Site>,
    statement_starts: BTreeSet<u32>,
    frames: Vec<Frame>,
    top_statement: (u32, u32),
    pending: Option<PendingFunction>,
    /// Chain expressions that must keep their reference semantics, for
    /// example `(a?.b)()`, where a wrapper would lose the `this` value.
    no_wrap: BTreeSet<(u32, u32)>,
    /// Logical expressions that are a direct operand of another logical
    /// expression. Their replacement needs parentheses: `a ?? b ?? c` with
    /// the inner `??` replaced must become `(a && b) ?? c`.
    logical_operands: BTreeSet<(u32, u32)>,
    /// Binary expressions that are a direct operand of a relational or
    /// equality expression. A swap from `<` to `!=` lowers the precedence:
    /// `c == a < b` with the `<` replaced must become `c == (a != b)`.
    relational_operands: BTreeSet<(u32, u32)>,
    /// The starts of `export default` expressions and concise arrow bodies.
    /// At these positions a leading `function`, `class` or `{` changes the
    /// parse, as at the start of a statement.
    expression_starts: BTreeSet<u32>,
    /// String and boolean case labels whose literal mutant is equivalent:
    /// the clause runs the same body as `default`. See
    /// `equivalent_case_labels`.
    equivalent_labels: BTreeSet<(u32, u32)>,
    /// Counted `for` loop tests whose `!=` mutant is equivalent. See
    /// `counted_loop_test`.
    counted_loop_tests: BTreeSet<(u32, u32)>,
    /// The identifier references that resolve to a binding that holds an
    /// array. See `ArrayBindings`.
    known_arrays: BTreeSet<ReferenceId>,
    /// Conditional tests whose `true` mutant is equivalent. See
    /// `FilteredLookups`.
    filtered_lookups: BTreeSet<(u32, u32)>,
    /// True while the visitor is in the declarators of a `const`
    /// declaration.
    const_declaration: bool,
    /// True while the visitor is in the initializer of a `const` declarator.
    /// With no function frame open, a fluent chain link there gives a
    /// `removed-chain-link` mutant. See `chain_link`.
    const_initializer: bool,
}

const fn pair(span: Span) -> (u32, u32) {
    (span.start, span.end)
}

fn is_function_like(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
    )
}

/// The value of a literal case label, compared with JS strict equality.
#[derive(PartialEq)]
enum LabelValue<'a> {
    String(&'a str),
    Boolean(bool),
    /// A number or `null`. No string or boolean mutant can take its value.
    Other,
}

fn label_value<'a>(test: &Expression<'a>) -> Option<LabelValue<'a>> {
    match test {
        // A label with a lone surrogate equals no string that a mutant gives.
        Expression::StringLiteral(it) => Some(
            it.value
                .as_str()
                .map_or(LabelValue::Other, LabelValue::String),
        ),
        Expression::BooleanLiteral(it) => Some(LabelValue::Boolean(it.value)),
        Expression::NumericLiteral(_) | Expression::NullLiteral(_) => Some(LabelValue::Other),
        _ => None,
    }
}

/// The value that the `string-literal` or `boolean-literal` mutant gives.
fn mutated_value(value: &LabelValue<'_>) -> Option<LabelValue<'static>> {
    match value {
        LabelValue::String("") => Some(LabelValue::String("fallow")),
        LabelValue::String(_) => Some(LabelValue::String("")),
        LabelValue::Boolean(flag) => Some(LabelValue::Boolean(!flag)),
        LabelValue::Other => None,
    }
}

/// True when the statement always leaves the clause, so control never falls
/// through into the next clause.
fn is_exit(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BreakStatement(it) => it.label.is_none(),
        Statement::ContinueStatement(it) => it.label.is_none(),
        _ => false,
    }
}

/// The source text of a clause body that ends in an exit.
fn exiting_body<'s>(source: &'s str, case: &SwitchCase<'_>) -> Option<&'s str> {
    let first = case.consequent.first()?;
    let last = case.consequent.last()?;
    if !is_exit(last) {
        return None;
    }
    source.get(first.span().start as usize..last.span().end as usize)
}

/// The spans of the case labels whose literal mutant cannot change the
/// result (#3639). For such a label, the original and the mutated switch
/// both run the `default` body for each input that the label matched, and
/// the mutated value selects no other clause. The rule needs all of these:
///
/// - every label is a string, number, boolean or `null` literal, so a label
///   has no side effect and the evaluation order does not matter;
/// - the clause body and the `default` body are the same text and both end
///   in an unconditional exit, so neither falls through;
/// - no other label has the original or the mutated value.
fn equivalent_case_labels(source: &str, switch: &SwitchStatement<'_>) -> Vec<(u32, u32)> {
    let Some(default_body) = switch
        .cases
        .iter()
        .find(|case| case.test.is_none())
        .and_then(|case| exiting_body(source, case))
    else {
        return Vec::new();
    };
    let mut labels = Vec::with_capacity(switch.cases.len());
    for case in &switch.cases {
        let Some(test) = &case.test else { continue };
        let Some(value) = label_value(test) else {
            return Vec::new();
        };
        labels.push((case, test.span(), value));
    }
    labels
        .iter()
        .enumerate()
        .filter(|(index, (case, _, value))| {
            let Some(mutated) = mutated_value(value) else {
                return false;
            };
            exiting_body(source, case) == Some(default_body)
                && labels
                    .iter()
                    .enumerate()
                    .all(|(other, (_, _, other_value))| {
                        other == *index || (other_value != value && *other_value != mutated)
                    })
        })
        .map(|(_, (_, span, _))| pair(*span))
        .collect()
}

/// The largest integer that a `number` holds without a loss of precision.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The value of a non-negative safe integer literal.
fn safe_integer(expression: &Expression<'_>) -> Option<f64> {
    let Expression::NumericLiteral(literal) = expression else {
        return None;
    };
    let value = literal.value;
    (value.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER).contains(&value)).then_some(value)
}

/// True when `update` moves the counter `name` one step: up for `<`, down
/// for `>`.
fn is_unit_step(update: &Expression<'_>, name: &str, up: bool) -> bool {
    match update {
        Expression::UpdateExpression(update) => {
            let operator = if up {
                UpdateOperator::Increment
            } else {
                UpdateOperator::Decrement
            };
            update.operator == operator
                && matches!(&update.argument, SimpleAssignmentTarget::AssignmentTargetIdentifier(id) if id.name == name)
        }
        Expression::AssignmentExpression(assignment) => {
            let operator = if up {
                AssignmentOperator::Addition
            } else {
                AssignmentOperator::Subtraction
            };
            assignment.operator == operator
                && matches!(&assignment.left, AssignmentTarget::AssignmentTargetIdentifier(id) if id.name == name)
                && matches!(&assignment.right, Expression::NumericLiteral(one) if one.value.total_cmp(&1.0).is_eq())
        }
        _ => false,
    }
}

/// The test of a counted `for` loop, where `i < N` and `i != N` select the
/// same iterations, so the `!=` mutant is equivalent. The rule needs all of
/// these:
///
/// - the init declares one `let` counter with a safe integer start `s`;
///   a `var` counter can have a write outside the loop;
/// - the test is `i < N` with `s <= N`, or `i > N` with `s >= N`, where `N`
///   is a safe integer literal, so the counter reaches the bound exactly;
/// - the update is `i++`, `++i` or `i += 1` for `<`, and the decrement for
///   `>`;
/// - the body has no write to a variable with the counter name, also in a
///   nested function, and no `eval` call.
fn counted_loop_test(it: &ForStatement<'_>) -> Option<(u32, u32)> {
    let Some(ForStatementInit::VariableDeclaration(init)) = &it.init else {
        return None;
    };
    let [declarator] = init.declarations.as_slice() else {
        return None;
    };
    if init.kind != VariableDeclarationKind::Let {
        return None;
    }
    let BindingPattern::BindingIdentifier(counter) = &declarator.id else {
        return None;
    };
    let name = counter.name.as_str();
    let start = safe_integer(declarator.init.as_ref()?)?;
    let Some(Expression::BinaryExpression(test)) = &it.test else {
        return None;
    };
    if !test.left.is_specific_id(name) {
        return None;
    }
    let bound = safe_integer(&test.right)?;
    let up = match test.operator {
        BinaryOperator::LessThan if start <= bound => true,
        BinaryOperator::GreaterThan if start >= bound => false,
        _ => return None,
    };
    if !is_unit_step(it.update.as_ref()?, name, up) {
        return None;
    }
    let mut writes = CounterWrites { name, found: false };
    writes.visit_statement(&it.body);
    (!writes.found).then_some(pair(test.span))
}

/// Finds a write to a variable by name, or an `eval` call that can make one.
/// The check uses the name only, so a shadowed variable also counts.
struct CounterWrites<'n> {
    name: &'n str,
    found: bool,
}

impl<'a> Visit<'a> for CounterWrites<'_> {
    fn visit_simple_assignment_target(&mut self, it: &SimpleAssignmentTarget<'a>) {
        let target = match it {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                identifier.name == self.name
            }
            _ => it
                .get_expression()
                .is_some_and(|inner| callee_without_wrappers(inner).is_specific_id(self.name)),
        };
        self.found |= target;
        walk::walk_simple_assignment_target(self, it);
    }

    fn visit_assignment_target_property_identifier(
        &mut self,
        it: &AssignmentTargetPropertyIdentifier<'a>,
    ) {
        self.found |= it.binding.name == self.name;
        walk::walk_assignment_target_property_identifier(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.found |= callee_without_wrappers(&it.callee).is_specific_id("eval");
        walk::walk_call_expression(self, it);
    }
}

/// The global names that a known-array proof by constructor or type depends
/// on. A file that binds one of these names gets no known-array proof.
const ARRAY_GLOBALS: [&str; 2] = ["Array", "ReadonlyArray"];

/// How a binding proves that it holds an array.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayProof {
    /// A `const` array value or a TypeScript array type. Each value that
    /// the binding can hold is an array.
    Fixed,
    /// A rest parameter without a type annotation. It starts as an array,
    /// so it qualifies only when no write to it exists.
    Unwritten,
}

/// Finds the identifier references that resolve to a binding that holds an
/// array, so that `name.length` is a non-negative integer. A binding
/// qualifies when it is:
///
/// - a `const` declarator with an array literal, `Array.from(...)` or
///   `Array.of(...)` initializer;
/// - a declarator or parameter with a TypeScript array type annotation:
///   `T[]`, `readonly T[]`, `Array<T>`, `ReadonlyArray<T>` or a tuple;
/// - a rest parameter, when no write to it exists or it has an array type.
///
/// Scope resolution decides which binding a reference uses, so a global or
/// a different binding with the same name gets no proof. A binding with a
/// redeclaration is not proven. A `with` statement or a direct `eval` call
/// can bind a name at run time, so they cancel every proof. A type
/// annotation is a type-level proof only: a cast such as `as any` can pass
/// another value. The rule accepts this, as the type checker does.
#[derive(Default)]
struct ArrayBindings {
    qualifying: BTreeMap<(u32, u32), ArrayProof>,
    array_globals_bound: bool,
    dynamic_scope: bool,
}

impl ArrayBindings {
    /// The syntactic pass. It returns `None` when no binding can qualify,
    /// so that the caller can skip the semantic build.
    fn scan(program: &Program<'_>) -> Option<Self> {
        let mut bindings = Self::default();
        bindings.visit_program(program);
        let usable = !bindings.qualifying.is_empty()
            && !bindings.dynamic_scope
            && !bindings.array_globals_bound;
        usable.then_some(bindings)
    }

    fn references(&self, semantic: &Semantic<'_>) -> BTreeSet<ReferenceId> {
        let scoping = semantic.scoping();
        let mut references = BTreeSet::new();
        for symbol in scoping.symbol_ids() {
            let Some(proof) = self.qualifying.get(&pair(scoping.symbol_span(symbol))) else {
                continue;
            };
            if !scoping.symbol_redeclarations(symbol).is_empty() {
                continue;
            }
            if *proof == ArrayProof::Unwritten
                && scoping
                    .get_resolved_references(symbol)
                    .any(|reference| reference.is_write())
            {
                continue;
            }
            references.extend(scoping.get_resolved_reference_ids(symbol).iter().copied());
        }
        references
    }
}

fn is_array_type(annotation: Option<&TSTypeAnnotation<'_>>) -> bool {
    let Some(annotation) = annotation else {
        return false;
    };
    match &annotation.type_annotation {
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => true,
        TSType::TSTypeOperatorType(operator) => {
            operator.operator == TSTypeOperatorOperator::Readonly
                && matches!(
                    operator.type_annotation,
                    TSType::TSArrayType(_) | TSType::TSTupleType(_)
                )
        }
        TSType::TSTypeReference(reference) => {
            reference.type_arguments.is_some()
                && matches!(&reference.type_name, TSTypeName::IdentifierReference(name) if ARRAY_GLOBALS.contains(&name.name.as_str()))
        }
        _ => false,
    }
}

/// True for an array literal, `Array.from(...)` or `Array.of(...)`.
fn is_array_value(init: &Expression<'_>) -> bool {
    match init {
        Expression::ArrayExpression(_) => true,
        Expression::CallExpression(call) => matches!(
            &call.callee,
            Expression::StaticMemberExpression(member)
                if member.object.is_specific_id("Array")
                    && matches!(member.property.name.as_str(), "from" | "of")
        ),
        _ => false,
    }
}

impl<'a> Visit<'a> for ArrayBindings {
    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        for declarator in &it.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let constant = it.kind == VariableDeclarationKind::Const
                && declarator.init.as_ref().is_some_and(is_array_value);
            if constant || is_array_type(declarator.type_annotation.as_deref()) {
                self.qualifying.insert(pair(id.span), ArrayProof::Fixed);
            }
        }
        walk::walk_variable_declaration(self, it);
    }

    fn visit_formal_parameter(&mut self, it: &FormalParameter<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.pattern
            && is_array_type(it.type_annotation.as_deref())
        {
            self.qualifying.insert(pair(id.span), ArrayProof::Fixed);
        }
        walk::walk_formal_parameter(self, it);
    }

    fn visit_formal_parameter_rest(&mut self, it: &FormalParameterRest<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.rest.argument {
            let proof = if is_array_type(it.type_annotation.as_deref()) {
                ArrayProof::Fixed
            } else {
                ArrayProof::Unwritten
            };
            self.qualifying.insert(pair(id.span), proof);
        }
        walk::walk_formal_parameter_rest(self, it);
    }

    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.array_globals_bound |= ARRAY_GLOBALS.contains(&it.name.as_str());
    }

    fn visit_with_statement(&mut self, it: &WithStatement<'a>) {
        self.dynamic_scope = true;
        walk::walk_with_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.dynamic_scope |= callee_without_wrappers(&it.callee).is_specific_id("eval");
        walk::walk_call_expression(self, it);
    }
}

/// The text that a file must contain to hold a `FilteredLookups` site. The
/// check is a cheap gate for the semantic build. A file that spells the call
/// another way only keeps its mutant.
const FILTER_BOOLEAN: &str = "filter(Boolean)";

/// The object keys that a falsy primitive converts to. A lookup with a
/// falsy key reads one of these keys.
const FALSY_KEYS: [&str; 6] = ["undefined", "null", "false", "0", "", "NaN"];

/// Finds the conditional tests whose `true` mutant is equivalent because the
/// array that holds the conditional drops every falsy value:
///
/// ```text
/// const alignment = { left: "text-left" } as const;
/// ["base", align ? alignment[align] : ""].filter(Boolean)
/// ```
///
/// For a truthy `align`, the mutant takes the same branch. For a falsy
/// `align`, the mutant reads `alignment[align]`, which is `undefined`, and
/// `filter(Boolean)` drops it as it drops `""`. A site qualifies when:
///
/// - the conditional is a direct element of an array literal, and the array
///   literal is the receiver of a `.filter(Boolean)` call that is not
///   optional, with the global `Boolean`;
/// - the test is an identifier with a local binding, and the consequent is
///   `obj[test]` or `obj?.[test]` with the same binding as its key;
/// - the alternate is `""`, an empty template, `null`, the global
///   `undefined`, `false`, `0` or `0n`;
/// - `obj` is a `const` binding that is not exported and has no
///   redeclaration, and its initializer is an object literal, optionally in
///   parentheses, `as` or `satisfies`;
/// - the declaration of `obj` ends before the conditional starts, so the
///   mutant read of `obj` cannot run in its temporal dead zone;
/// - the object literal has only plain properties with identifier, string
///   or number keys, no spread, no getter, setter or method, no `__proto__`
///   key and no key from `FALSY_KEYS`;
/// - each value reference to `obj` is the object of a member read that is
///   not a write target, a `delete` operand or a callee, so the object
///   keeps its keys.
///
/// A `with` statement or a direct `eval` call cancels every proof. A change
/// to `Object.prototype` or `Array.prototype.filter` at run time can break
/// the proof. A call from the initializer of `obj` to a hoisted function
/// that holds the conditional can still read `obj` in its temporal dead
/// zone. The rule accepts these risks.
struct FilteredLookups<'s, 'b> {
    semantic: &'s Semantic<'b>,
    /// The binding span of each closed object, with the end of its
    /// declarator.
    objects: BTreeMap<(u32, u32), u32>,
    member_reads: BTreeSet<ReferenceId>,
    unsafe_reads: BTreeSet<ReferenceId>,
    /// The test span, the conditional start and the object binding of each
    /// qualifying element.
    candidates: Vec<((u32, u32), u32, SymbolId)>,
    exported: bool,
    dynamic_scope: bool,
}

impl<'s, 'b> FilteredLookups<'s, 'b> {
    fn tests(program: &Program<'_>, semantic: &'s Semantic<'b>) -> BTreeSet<(u32, u32)> {
        let mut lookups = Self {
            semantic,
            objects: BTreeMap::new(),
            member_reads: BTreeSet::new(),
            unsafe_reads: BTreeSet::new(),
            candidates: Vec::new(),
            exported: false,
            dynamic_scope: false,
        };
        lookups.visit_program(program);
        if lookups.dynamic_scope {
            return BTreeSet::new();
        }
        lookups
            .candidates
            .iter()
            .filter(|(_, start, object)| lookups.closed_object(*object, *start))
            .map(|(test, _, _)| *test)
            .collect()
    }

    /// The local binding of `identifier`, or `None` for a global.
    fn symbol(&self, identifier: &IdentifierReference<'_>) -> Option<SymbolId> {
        let reference = identifier.reference_id.get()?;
        self.semantic.scoping().get_reference(reference).symbol_id()
    }

    fn is_global(&self, expression: &Expression<'_>, name: &str) -> bool {
        matches!(expression, Expression::Identifier(id)
            if id.name == name && id.reference_id.get().is_some() && self.symbol(id).is_none())
    }

    /// True when `symbol` is a closed object that is declared before
    /// `conditional_start`.
    fn closed_object(&self, symbol: SymbolId, conditional_start: u32) -> bool {
        let scoping = self.semantic.scoping();
        let declared_before = self
            .objects
            .get(&pair(scoping.symbol_span(symbol)))
            .is_some_and(|end| *end <= conditional_start);
        if !declared_before || !scoping.symbol_redeclarations(symbol).is_empty() {
            return false;
        }
        scoping.get_resolved_reference_ids(symbol).iter().all(|id| {
            let reference = scoping.get_reference(*id);
            reference.is_type()
                || (!reference.flags().is_member_write_target()
                    && self.member_reads.contains(id)
                    && !self.unsafe_reads.contains(id))
        })
    }

    /// The object binding of a qualifying element, or `None`.
    fn element_object(&self, element: &Expression<'_>) -> Option<SymbolId> {
        let Expression::ConditionalExpression(conditional) = element.without_parentheses() else {
            return None;
        };
        let Expression::Identifier(test) = conditional.test.without_parentheses() else {
            return None;
        };
        let (object, key) = match conditional.consequent.without_parentheses() {
            Expression::ComputedMemberExpression(member) => (&member.object, &member.expression),
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::ComputedMemberExpression(member) => {
                    (&member.object, &member.expression)
                }
                _ => return None,
            },
            _ => return None,
        };
        let (Expression::Identifier(object), Expression::Identifier(key)) = (object, key) else {
            return None;
        };
        let test_symbol = self.symbol(test)?;
        if key.name != test.name || self.symbol(key) != Some(test_symbol) {
            return None;
        }
        if !self.falsy_literal(conditional.alternate.without_parentheses()) {
            return None;
        }
        self.symbol(object)
    }

    fn falsy_literal(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::StringLiteral(literal) => literal.value.is_empty(),
            Expression::TemplateLiteral(template) => {
                template.expressions.is_empty()
                    && template
                        .quasis
                        .iter()
                        .all(|quasi| quasi.value.raw.is_empty())
            }
            Expression::NullLiteral(_) => true,
            Expression::BooleanLiteral(literal) => !literal.value,
            Expression::NumericLiteral(literal) => literal.value == 0.0,
            Expression::BigIntLiteral(literal) => literal.is_zero(),
            _ => self.is_global(expression, "undefined"),
        }
    }

    fn mark_unsafe(&mut self, expression: &Expression<'_>) {
        if let Some(member) = callee_without_wrappers(expression).as_member_expression()
            && let Expression::Identifier(object) = member.object()
            && let Some(reference) = object.reference_id.get()
        {
            self.unsafe_reads.insert(reference);
        }
    }

    fn record_read(&mut self, object: &Expression<'_>) {
        if let Expression::Identifier(object) = object
            && let Some(reference) = object.reference_id.get()
        {
            self.member_reads.insert(reference);
        }
    }
}

/// True for an object literal that cannot hold a key from `FALSY_KEYS` or
/// change its prototype.
fn is_closed_object_literal(init: &Expression<'_>) -> bool {
    let Expression::ObjectExpression(object) = callee_without_wrappers(init) else {
        return false;
    };
    object.properties.iter().all(|property| {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return false;
        };
        if property.computed || property.method || property.kind != PropertyKind::Init {
            return false;
        }
        let key = match &property.key {
            PropertyKey::StaticIdentifier(name) => name.name.as_str(),
            // A key with a lone surrogate is not `__proto__` or a falsy key.
            PropertyKey::StringLiteral(literal) => match literal.value.as_str() {
                Some(key) => key,
                None => return true,
            },
            PropertyKey::NumericLiteral(literal) => {
                return literal.value != 0.0 && !literal.value.is_nan();
            }
            _ => return false,
        };
        key != "__proto__" && !FALSY_KEYS.contains(&key)
    })
}

impl<'a> Visit<'a> for FilteredLookups<'_, '_> {
    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        // Only the exported variable itself is public. A `const` in the body
        // of an exported function is local.
        self.exported = matches!(it.declaration, Declaration::VariableDeclaration(_));
        walk::walk_export_declaration(self, it);
        self.exported = false;
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if it.kind == VariableDeclarationKind::Const && !self.exported {
            for declarator in &it.declarations {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id
                    && declarator
                        .init
                        .as_ref()
                        .is_some_and(is_closed_object_literal)
                {
                    self.objects.insert(pair(id.span), declarator.span.end);
                }
            }
        }
        let exported = std::mem::replace(&mut self.exported, false);
        walk::walk_variable_declaration(self, it);
        self.exported = exported;
    }

    fn visit_expression(&mut self, it: &Expression<'a>) {
        if let Some(member) = it.as_member_expression() {
            self.record_read(member.object());
        }
        walk::walk_expression(self, it);
    }

    fn visit_chain_element(&mut self, it: &ChainElement<'a>) {
        if let Some(member) = it.as_member_expression() {
            self.record_read(member.object());
        }
        walk::walk_chain_element(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        if it.operator == UnaryOperator::Delete {
            self.mark_unsafe(&it.argument);
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        self.mark_unsafe(&it.tag);
        walk::walk_tagged_template_expression(self, it);
    }

    fn visit_with_statement(&mut self, it: &WithStatement<'a>) {
        self.dynamic_scope = true;
        walk::walk_with_statement(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.dynamic_scope |= callee_without_wrappers(&it.callee).is_specific_id("eval");
        self.mark_unsafe(&it.callee);
        if let Expression::StaticMemberExpression(member) = &it.callee
            && !it.optional
            && !member.optional
            && member.property.name == "filter"
            && let Expression::ArrayExpression(array) = member.object.without_parentheses()
            && let [argument] = it.arguments.as_slice()
            && argument
                .as_expression()
                .is_some_and(|argument| self.is_global(argument, "Boolean"))
        {
            for element in &array.elements {
                let Some(element) = element.as_expression() else {
                    continue;
                };
                if let Some(object) = self.element_object(element)
                    && let Expression::ConditionalExpression(conditional) =
                        element.without_parentheses()
                {
                    self.candidates.push((
                        pair(conditional.test.span()),
                        conditional.span.start,
                        object,
                    ));
                }
            }
        }
        walk::walk_call_expression(self, it);
    }
}

/// The comparison operator with its operands swapped: `0 < n` is `n > 0`.
const fn mirrored(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::LessThan => BinaryOperator::GreaterThan,
        BinaryOperator::LessEqualThan => BinaryOperator::GreaterEqualThan,
        BinaryOperator::GreaterThan => BinaryOperator::LessThan,
        BinaryOperator::GreaterEqualThan => BinaryOperator::LessEqualThan,
        other => other,
    }
}

/// The replacement that is equivalent for `n <operator> 0` when `n` is a
/// non-negative integer: `n > 0` is `n != 0`, `n <= 0` is `n == 0`,
/// `n < 0` is always false and `n >= 0` is always true.
const fn equivalent_zero_replacement(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::GreaterThan => Some("!="),
        BinaryOperator::LessEqualThan => Some("=="),
        BinaryOperator::LessThan => Some("false"),
        BinaryOperator::GreaterEqualThan => Some("true"),
        _ => None,
    }
}

fn is_zero(expression: &Expression<'_>) -> bool {
    matches!(
        callee_without_wrappers(expression),
        Expression::NumericLiteral(literal) if literal.value == 0.0
    )
}

fn key_name(key: &PropertyKey<'_>) -> Option<String> {
    key.static_name()
        .and_then(|name| name.as_js_str().as_str().map(str::to_owned))
}

/// `console.*` and `debug(...)` calls. The collector skips the full call:
/// it gives no mutant for any expression in the arguments, not only for the
/// strings. Logic in a logging argument is therefore not tested.
///
/// The check reads the callee through parentheses and TypeScript wrappers,
/// so `(console.log as any)(x)` is a logging call too.
pub fn is_logging_callee(callee: &Expression<'_>) -> bool {
    let callee = callee_without_wrappers(callee);
    if callee.is_specific_id("debug") {
        return true;
    }
    callee
        .as_member_expression()
        .is_some_and(|member| callee_without_wrappers(member.object()).is_specific_id("console"))
}

/// A `require(...)` call, also behind parentheses and TypeScript wrappers.
fn is_require_callee(callee: &Expression<'_>) -> bool {
    callee_without_wrappers(callee).is_specific_id("require")
}

/// The callee without parentheses and TypeScript expression wrappers, at any
/// depth. Type stripping keeps the reference, so `(o?.m as any)()` calls `m`
/// with `this` set to `o`, the same as `(o?.m)()`.
fn callee_without_wrappers<'e, 'a>(callee: &'e Expression<'a>) -> &'e Expression<'a> {
    match callee {
        Expression::ParenthesizedExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSAsExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSNonNullExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSTypeAssertion(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSInstantiationExpression(inner) => callee_without_wrappers(&inner.expression),
        _ => callee,
    }
}

/// True when `expression` is part of an optional chain that has an optional
/// link at this level or below. A wrapper around such a receiver would end
/// the short circuit: `a?.b().c()` and `(a?.b()).c()` differ when `a` is
/// nullish.
fn has_optional_link(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::CallExpression(call) => call.optional || has_optional_link(&call.callee),
        Expression::TSNonNullExpression(inner) => has_optional_link(&inner.expression),
        _ => expression
            .as_member_expression()
            .is_some_and(|member| member.optional() || has_optional_link(member.object())),
    }
}

fn is_skipped_call(call: &CallExpression<'_>) -> bool {
    matches!(call.callee, Expression::Super(_))
        || is_logging_callee(&call.callee)
        || is_require_callee(&call.callee)
}

const fn is_relational(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
            | BinaryOperator::Equality
            | BinaryOperator::Inequality
            | BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
    )
}

/// The operators at the relational or equality precedence level.
const fn is_comparison_level(operator: BinaryOperator) -> bool {
    is_relational(operator) || matches!(operator, BinaryOperator::In | BinaryOperator::Instanceof)
}

/// True when the replacement operator binds weaker than `operator`, so that
/// a token swap inside a comparison regroups the operands.
const fn lowers_precedence(operator: BinaryOperator, replacement: &str) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
    ) && matches!(replacement.as_bytes(), b"==" | b"!=")
}

/// The non-redundant relational replacements: operator texts, then the
/// constant that replaces the whole expression.
const fn relational_replacements(
    operator: BinaryOperator,
) -> Option<(&'static [&'static str], &'static str)> {
    let replacements: (&'static [&'static str], &'static str) = match operator {
        BinaryOperator::LessThan => (&["<=", "!="], "false"),
        BinaryOperator::LessEqualThan => (&["<", "=="], "true"),
        BinaryOperator::GreaterThan => (&[">=", "!="], "false"),
        BinaryOperator::GreaterEqualThan => (&[">", "=="], "true"),
        BinaryOperator::StrictEquality => (&["!=="], "false"),
        BinaryOperator::StrictInequality => (&["==="], "true"),
        BinaryOperator::Equality => (&["!="], "false"),
        BinaryOperator::Inequality => (&["=="], "true"),
        _ => return None,
    };
    Some(replacements)
}

const fn arithmetic_replacement(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::Addition => Some("-"),
        BinaryOperator::Subtraction => Some("+"),
        BinaryOperator::Multiplication => Some("/"),
        BinaryOperator::Division | BinaryOperator::Remainder => Some("*"),
        _ => None,
    }
}

fn is_text_operand(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

/// The byte length of the `//` comment at the start of `rest`. The comment
/// ends before the first line terminator: `\n`, `\r`, U+2028 or U+2029.
fn line_comment_len(rest: &[u8]) -> usize {
    const LINE_SEPARATOR: &[u8] = "\u{2028}".as_bytes();
    const PARAGRAPH_SEPARATOR: &[u8] = "\u{2029}".as_bytes();
    (0..rest.len())
        .find(|&index| {
            let tail = &rest[index..];
            matches!(tail, [b'\n' | b'\r', ..])
                || tail.starts_with(LINE_SEPARATOR)
                || tail.starts_with(PARAGRAPH_SEPARATOR)
        })
        .unwrap_or(rest.len())
}

impl Collector<'_> {
    fn text(&self, start: u32, end: u32) -> &str {
        self.source
            .get(start as usize..end as usize)
            .unwrap_or_default()
    }

    fn byte(&self, offset: u32) -> Option<u8> {
        self.source.as_bytes().get(offset as usize).copied()
    }

    /// Finds `token` in `from..to`, after whitespace and comments only.
    ///
    /// The comment check comes first: a comment starts with `/`, the same
    /// byte as the division token.
    fn find_token(&self, from: u32, to: u32, token: &str) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let end = (to as usize).min(bytes.len());
        let mut index = from as usize;
        while index < end {
            let rest = bytes.get(index..end)?;
            let comment = matches!(rest, [b'/', b'/' | b'*', ..]);
            if !comment && rest.starts_with(token.as_bytes()) {
                return u32::try_from(index).ok();
            }
            match rest {
                [b'/', b'/', ..] => {
                    index += line_comment_len(rest);
                }
                [b'/', b'*', ..] => {
                    let close = rest.windows(2).position(|w| w == b"*/")?;
                    index += close + 2;
                }
                [byte, ..] if byte.is_ascii_whitespace() => index += 1,
                [first, ..] if *first >= 0x80 => {
                    // Unicode whitespace such as NBSP or line separators.
                    let ch = self.source.get(index..)?.chars().next()?;
                    if !ch.is_whitespace() {
                        return None;
                    }
                    index += ch.len_utf8();
                }
                _ => return None,
            }
        }
        None
    }

    /// Adds spaces where the replacement would join an identifier next to
    /// the region, for example `void 0` after the `do` keyword in `do(f)()`.
    fn separate(&self, region: (u32, u32), replacement: String) -> String {
        let bytes = replacement.as_bytes();
        let before = region.0.checked_sub(1).and_then(|offset| self.byte(offset));
        let joins_before = bytes.first().copied().is_some_and(is_identifier_byte)
            && before.is_some_and(is_identifier_byte);
        let joins_after = bytes.last().copied().is_some_and(is_identifier_byte)
            && self.byte(region.1).is_some_and(is_identifier_byte);
        match (joins_before, joins_after) {
            (false, false) => replacement,
            (true, false) => format!(" {replacement}"),
            (false, true) => format!("{replacement} "),
            (true, true) => format!(" {replacement} "),
        }
    }

    fn add(
        &mut self,
        site: Span,
        operator: Operator,
        region: (u32, u32),
        replacement: String,
        change: MutantChange,
    ) {
        let replacement = self.separate(region, replacement);
        let anchor = self
            .frames
            .last()
            .map_or(self.top_statement, |frame| frame.reference.span);
        let function = self.frames.last().map(|frame| frame.reference.clone());
        self.sites
            .entry(pair(site))
            .or_insert_with(|| Site {
                span: pair(site),
                candidates: Vec::new(),
                anchor,
                function,
            })
            .candidates
            .push(Candidate {
                operator,
                region,
                replacement,
                change,
            });
    }

    /// Adds a space after `operator` when the next byte would merge with it
    /// into another token, for example `a+-b` to `a- -b`.
    fn pad_operator(&self, operator: &str, end: u32) -> String {
        match self.byte(end) {
            Some(b'+' | b'-' | b'/' | b'*') => format!("{operator} "),
            _ => operator.to_owned(),
        }
    }

    /// The replacement for a removed prefix token: a space when the removal
    /// would join two identifier characters, for example `return!a`.
    fn removal_text(&self, start: u32, end: u32) -> String {
        let before = start.checked_sub(1).and_then(|offset| self.byte(offset));
        let after = self.byte(end);
        match (before, after) {
            (Some(left), Some(right)) if is_identifier_byte(left) && is_identifier_byte(right) => {
                " ".to_owned()
            }
            _ => String::new(),
        }
    }

    fn negate(&mut self, test: &Expression<'_>) {
        let skip = match test.without_parentheses() {
            Expression::BinaryExpression(binary) => is_relational(binary.operator),
            Expression::LogicalExpression(_)
            | Expression::ImportMeta(_)
            | Expression::NewTarget(_) => true,
            _ => false,
        };
        if skip {
            return;
        }
        let region = pair(test.span());
        let equivalent_true = self.filtered_lookups.contains(&region);
        for value in ["true", "false"] {
            if equivalent_true && value == "true" {
                continue;
            }
            self.add(
                test.span(),
                Operator::NegateCondition,
                region,
                value.to_owned(),
                MutantChange::new(MutantChange::CONDITION, value),
            );
        }
    }

    fn removable_call(expression: &Expression<'_>) -> bool {
        match expression.without_parentheses() {
            Expression::CallExpression(call) => !is_skipped_call(call),
            Expression::AwaitExpression(await_expression) => {
                Self::removable_call(&await_expression.argument)
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => !is_skipped_call(call),
                _ => false,
            },
            _ => false,
        }
    }

    fn returns_value(&self, argument: &Expression<'_>) -> bool {
        if self
            .frames
            .last()
            .is_some_and(|frame| frame.constructor || frame.generator)
        {
            return false;
        }
        match argument.without_parentheses() {
            Expression::Identifier(identifier) => identifier.name != "undefined",
            Expression::UnaryExpression(unary) => unary.operator != UnaryOperator::Void,
            Expression::BooleanLiteral(_)
            | Expression::ImportMeta(_)
            | Expression::NewTarget(_) => false,
            _ => true,
        }
    }

    fn push_frame(&mut self, span: Span, name: Option<String>, generator: bool) {
        let pending = self.pending.take();
        let constructor = pending.as_ref().is_some_and(|pending| pending.constructor);
        let name = name.or_else(|| pending.and_then(|pending| pending.name));
        self.frames.push(Frame {
            reference: FunctionRef {
                name,
                span: pair(span),
            },
            constructor,
            generator,
        });
    }

    fn set_pending(&mut self, value: &Expression<'_>, name: Option<String>, constructor: bool) {
        self.pending = is_function_like(value).then_some(PendingFunction { name, constructor });
    }

    fn chain_links(&self, element: &ChainElement<'_>, links: &mut Vec<(u32, &'static str)>) {
        match element {
            ChainElement::CallExpression(call) => {
                self.call_link(call.optional, &call.callee, links);
            }
            ChainElement::TSNonNullExpression(non_null) => {
                self.expression_links(&non_null.expression, links);
            }
            _ => {
                if let Some(member) = element.as_member_expression() {
                    self.member_link(member, links);
                }
            }
        }
    }

    fn expression_links(&self, expression: &Expression<'_>, links: &mut Vec<(u32, &'static str)>) {
        match expression {
            Expression::CallExpression(call) => self.call_link(call.optional, &call.callee, links),
            Expression::TSNonNullExpression(non_null) => {
                self.expression_links(&non_null.expression, links);
            }
            _ => {
                if let Some(member) = expression.as_member_expression() {
                    self.member_link(member, links);
                }
            }
        }
    }

    fn call_link(
        &self,
        optional: bool,
        callee: &Expression<'_>,
        links: &mut Vec<(u32, &'static str)>,
    ) {
        if optional && let Some(offset) = self.find_token(callee.span().end, u32::MAX, "?.") {
            links.push((offset, ""));
        }
        self.expression_links(callee, links);
    }

    fn member_link(&self, member: &MemberExpression<'_>, links: &mut Vec<(u32, &'static str)>) {
        let object = member.object();
        if member.optional()
            && let Some(offset) = self.find_token(object.span().end, u32::MAX, "?.")
        {
            // `1?.x` to `1.x` would lex the dot as a decimal point.
            let number_before =
                matches!(object, Expression::NumericLiteral(_)) && offset == object.span().end;
            let replacement = match member {
                MemberExpression::ComputedMemberExpression(_) => "",
                _ if number_before => " .",
                _ => ".",
            };
            links.push((offset, replacement));
        }
        self.expression_links(object, links);
    }

    /// True when `expression` is always a non-negative integer. Only the
    /// `length` of an array literal or of a known array qualifies, never an
    /// arbitrary object with a `length` property, which can be negative.
    fn is_non_negative_integer(&self, expression: &Expression<'_>) -> bool {
        let Expression::StaticMemberExpression(member) = callee_without_wrappers(expression) else {
            return false;
        };
        if member.optional || member.property.name != "length" {
            return false;
        }
        match callee_without_wrappers(&member.object) {
            Expression::ArrayExpression(_) => true,
            Expression::Identifier(id) => id
                .reference_id
                .get()
                .is_some_and(|reference| self.known_arrays.contains(&reference)),
            _ => false,
        }
    }

    /// The relational replacement that gives the same result as `it`, if
    /// one is provable. The collector does not emit it.
    fn equivalent_replacement(&self, it: &BinaryExpression<'_>) -> Option<&'static str> {
        if self.counted_loop_tests.contains(&pair(it.span)) {
            return Some("!=");
        }
        if is_zero(&it.right) && self.is_non_negative_integer(&it.left) {
            return equivalent_zero_replacement(it.operator);
        }
        if is_zero(&it.left) && self.is_non_negative_integer(&it.right) {
            return equivalent_zero_replacement(mirrored(it.operator));
        }
        None
    }

    /// Adds a `removed-chain-link` candidate when `call` is a link of a
    /// fluent chain: `r.m(args)` with a static member `.m` and a receiver `r`
    /// that is a call. The mutant removes `.m(args)`, so `z.number().int()`
    /// becomes `z.number()`. The root call of a chain is not a link.
    ///
    /// The callee must be the member itself, without parentheses or a type
    /// wrapper: the removed region ends at the end of the call, and a wrapper
    /// around the member would lose its closing parenthesis.
    fn chain_link(&mut self, call: &CallExpression<'_>) {
        if call.optional {
            return;
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return;
        };
        if member.optional || has_optional_link(&member.object) {
            return;
        }
        let Expression::CallExpression(receiver) = callee_without_wrappers(&member.object) else {
            return;
        };
        if is_skipped_call(receiver) {
            return;
        }
        // The region starts at the dot, so whitespace and comments before
        // the link stay and the mutant line is the line of the link.
        let receiver_end = member.object.span().end;
        let start = self
            .find_token(receiver_end, member.property.span.start, ".")
            .unwrap_or(receiver_end);
        self.add(
            call.span,
            Operator::RemovedChainLink,
            (start, call.span.end),
            String::new(),
            MutantChange::new(MutantChange::CALL, ""),
        );
    }

    fn logical_operand(&self, operand: &Expression<'_>) -> String {
        let text = self.text(operand.span().start, operand.span().end);
        if matches!(operand, Expression::LogicalExpression(_)) {
            format!("({text})")
        } else {
            text.to_owned()
        }
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        for statement in &it.body {
            self.top_statement = pair(statement.span());
            self.visit_statement(statement);
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.declare || it.body.is_none() {
            self.pending = None;
            return;
        }
        let name = it.id.as_ref().map(|id| id.name.as_str().to_owned());
        self.push_frame(it.span, name, it.generator);
        walk::walk_function(self, it, flags);
        self.frames.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let Some(body) = it.get_expression() {
            self.expression_starts.insert(body.span().start);
        }
        self.push_frame(it.span, None, false);
        walk::walk_arrow_function_expression(self, it);
        self.frames.pop();
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        if it.value.body.is_none() {
            return;
        }
        let constructor = it.kind == MethodDefinitionKind::Constructor;
        self.pending = Some(PendingFunction {
            name: key_name(&it.key),
            constructor,
        });
        self.visit_function(&it.value, ScopeFlags::Function);
    }

    fn visit_property_definition(&mut self, it: &PropertyDefinition<'a>) {
        if it.declare {
            return;
        }
        if let Some(value) = &it.value {
            self.set_pending(value, key_name(&it.key), false);
            self.visit_expression(value);
        }
    }

    fn visit_accessor_property(&mut self, it: &AccessorProperty<'a>) {
        if let Some(value) = &it.value {
            self.set_pending(value, key_name(&it.key), false);
            self.visit_expression(value);
        }
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        self.set_pending(&it.value, key_name(&it.key), false);
        self.visit_expression(&it.value);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if !it.declare {
            let outer = self.const_declaration;
            self.const_declaration = it.kind == VariableDeclarationKind::Const;
            walk::walk_variable_declaration(self, it);
            self.const_declaration = outer;
        }
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        self.visit_binding_pattern(&it.id);
        if let Some(init) = &it.init {
            let name = it
                .id
                .get_binding_identifier()
                .map(|id| id.name.as_str().to_owned());
            self.set_pending(init, name, false);
            let outer = self.const_initializer;
            self.const_initializer = self.const_declaration;
            self.visit_expression(init);
            self.const_initializer = outer;
        }
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if !it.declare {
            walk::walk_class(self, it);
        }
    }

    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        let expression = &it.expression;
        self.statement_starts.insert(expression.span().start);
        if Self::removable_call(expression) {
            let span = expression.span();
            self.add(
                span,
                Operator::RemovedCall,
                pair(span),
                "void 0".to_owned(),
                MutantChange::new(MutantChange::CALL, "void 0"),
            );
        }
        self.visit_expression(expression);
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        if let Some(expression) = it.declaration.as_expression() {
            self.expression_starts.insert(expression.span().start);
        }
        walk::walk_export_default_declaration(self, it);
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if let Some(argument) = &it.argument {
            if self.returns_value(argument) {
                let span = argument.span();
                self.add(
                    span,
                    Operator::ReturnValue,
                    pair(span),
                    "undefined".to_owned(),
                    MutantChange::new(MutantChange::EXPRESSION, "undefined"),
                );
            }
            self.visit_expression(argument);
        }
    }

    fn visit_if_statement(&mut self, it: &IfStatement<'a>) {
        if crate::logging_only::is_logging_only_if(it) {
            return;
        }
        walk::walk_if_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        walk::walk_while_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        walk::walk_do_while_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        if let Some(test) = counted_loop_test(it) {
            self.counted_loop_tests.insert(test);
        }
        walk::walk_for_statement(self, it);
        if let Some(test) = &it.test {
            self.negate(test);
        }
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        walk::walk_conditional_expression(self, it);
        self.negate(&it.test);
    }

    fn visit_binary_expression(&mut self, it: &BinaryExpression<'a>) {
        let operator = it.operator;
        let found = self.find_token(it.left.span().end, it.right.span().start, operator.as_str());
        if let Some(start) = found {
            let end = start + operator.as_str().len() as u32;
            if let Some(replacement) = arithmetic_replacement(operator) {
                let concatenation =
                    operator == BinaryOperator::Addition || operator == BinaryOperator::Subtraction;
                let text =
                    concatenation && (is_text_operand(&it.left) || is_text_operand(&it.right));
                if !text {
                    let padded = self.pad_operator(replacement, end);
                    self.add(
                        it.span,
                        Operator::ArithmeticOperator,
                        (start, end),
                        padded,
                        MutantChange::new(operator.as_str(), replacement),
                    );
                }
            }
            if let Some((operators, constant)) = relational_replacements(operator) {
                let equivalent = self.equivalent_replacement(it);
                if equivalent != Some(constant) {
                    self.add(
                        it.span,
                        Operator::RelationalOperator,
                        pair(it.span),
                        constant.to_owned(),
                        MutantChange::new(MutantChange::EXPRESSION, constant),
                    );
                }
                let operand = self.relational_operands.contains(&pair(it.span));
                for replacement in operators {
                    if equivalent == Some(*replacement) {
                        continue;
                    }
                    let change = MutantChange::new(operator.as_str(), replacement);
                    if operand && lowers_precedence(operator, replacement) {
                        let text = format!(
                            "({}{replacement}{})",
                            self.text(it.span.start, start),
                            self.text(end, it.span.end),
                        );
                        self.add(
                            it.span,
                            Operator::RelationalOperator,
                            pair(it.span),
                            text,
                            change,
                        );
                    } else {
                        self.add(
                            it.span,
                            Operator::RelationalOperator,
                            (start, end),
                            (*replacement).to_owned(),
                            change,
                        );
                    }
                }
            }
        }
        if is_comparison_level(operator) {
            for operand in [&it.left, &it.right] {
                if matches!(operand, Expression::BinaryExpression(_)) {
                    self.relational_operands.insert(pair(operand.span()));
                }
            }
        }
        walk::walk_binary_expression(self, it);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        let operator = it.operator;
        let found = self.find_token(it.left.span().end, it.right.span().start, operator.as_str());
        if let Some(start) = found {
            let end = start + operator.as_str().len() as u32;
            let replacement = match operator {
                LogicalOperator::And => "||",
                LogicalOperator::Or | LogicalOperator::Coalesce => "&&",
            };
            let change = MutantChange::new(operator.as_str(), replacement);
            let left_nested = matches!(it.left, Expression::LogicalExpression(_));
            let right_nested = matches!(it.right, Expression::LogicalExpression(_));
            if left_nested {
                self.logical_operands.insert(pair(it.left.span()));
            }
            if right_nested {
                self.logical_operands.insert(pair(it.right.span()));
            }
            let operand = self.logical_operands.contains(&pair(it.span));
            if left_nested || right_nested || operand {
                let text = format!(
                    "{}{}{replacement}{}{}",
                    self.logical_operand(&it.left),
                    self.text(it.left.span().end, start),
                    self.text(end, it.right.span().start),
                    self.logical_operand(&it.right),
                );
                let text = if operand { format!("({text})") } else { text };
                self.add(
                    it.span,
                    Operator::LogicalOperator,
                    pair(it.span),
                    text,
                    change,
                );
            } else {
                self.add(
                    it.span,
                    Operator::LogicalOperator,
                    (start, end),
                    replacement.to_owned(),
                    change,
                );
            }
        }
        walk::walk_logical_expression(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        let mutable = match it.operator {
            UnaryOperator::Delete => return,
            UnaryOperator::LogicalNot => !matches!(
                it.argument.without_parentheses(),
                Expression::UnaryExpression(inner) if inner.operator == UnaryOperator::LogicalNot
            ),
            UnaryOperator::UnaryNegation => true,
            _ => false,
        };
        if mutable && self.text(it.span.start, it.span.start + 1) == it.operator.as_str() {
            let change = MutantChange::new(it.operator.as_str(), "");
            if self.statement_starts.contains(&it.span.start)
                || self.expression_starts.contains(&it.span.start)
            {
                // Without the operator, `!function () {}()` would start with
                // `function` and parse as a declaration.
                let argument = it.argument.span();
                let replacement = format!("({})", self.text(argument.start, argument.end));
                self.add(
                    it.span,
                    Operator::UnaryOperator,
                    pair(it.span),
                    replacement,
                    change,
                );
            } else {
                let region = (it.span.start, it.span.start + 1);
                let replacement = self.removal_text(region.0, region.1);
                self.add(
                    it.span,
                    Operator::UnaryOperator,
                    region,
                    replacement,
                    change,
                );
            }
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        let (text, replacement) = match it.operator {
            UpdateOperator::Increment => ("++", "--"),
            UpdateOperator::Decrement => ("--", "++"),
        };
        let region = if it.prefix {
            (it.span.start, it.span.start + 2)
        } else {
            (it.span.end.saturating_sub(2), it.span.end)
        };
        if self.text(region.0, region.1) != text {
            return;
        }
        let before = region.0.checked_sub(1).and_then(|offset| self.byte(offset));
        let change = MutantChange::new(text, replacement);
        let replacement = if it.prefix && matches!(before, Some(b'+' | b'-')) {
            format!(" {replacement}")
        } else {
            replacement.to_owned()
        };
        self.add(
            it.span,
            Operator::UpdateOperator,
            region,
            replacement,
            change,
        );
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        let replacement = match it.operator {
            AssignmentOperator::Addition => Some("-="),
            AssignmentOperator::Subtraction => Some("+="),
            _ => None,
        };
        let token = it.operator.as_str();
        if let Some(replacement) = replacement
            && let Some(start) = self.find_token(it.left.span().end, it.right.span().start, token)
        {
            let region = (start, start + token.len() as u32);
            self.add(
                it.span,
                Operator::ArithmeticOperator,
                region,
                replacement.to_owned(),
                MutantChange::new(token, replacement),
            );
        }
        self.visit_expression(&it.right);
    }

    fn visit_chain_expression(&mut self, it: &ChainExpression<'a>) {
        if let ChainElement::CallExpression(call) = &it.expression
            && is_logging_callee(&call.callee)
        {
            return;
        }
        if !self.no_wrap.contains(&pair(it.span)) {
            let mut links = Vec::new();
            self.chain_links(&it.expression, &mut links);
            links.sort_unstable();
            for (offset, replacement) in links {
                let region = (offset, offset + 2);
                self.add(
                    it.span,
                    Operator::OptionalChaining,
                    region,
                    replacement.to_owned(),
                    MutantChange::new("?.", replacement.trim_start()),
                );
            }
        }
        walk::walk_chain_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if is_logging_callee(&it.callee) || is_require_callee(&it.callee) {
            return;
        }
        if let Expression::ChainExpression(chain) = callee_without_wrappers(&it.callee) {
            self.no_wrap.insert(pair(chain.span));
        }
        if self.const_initializer && self.frames.is_empty() {
            self.chain_link(it);
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_switch_statement(&mut self, it: &SwitchStatement<'a>) {
        let labels = equivalent_case_labels(self.source, it);
        self.equivalent_labels.extend(labels);
        walk::walk_switch_statement(self, it);
    }

    fn visit_boolean_literal(&mut self, it: &BooleanLiteral) {
        if self.equivalent_labels.contains(&pair(it.span)) {
            return;
        }
        let (original, replacement) = if it.value {
            ("true", "false")
        } else {
            ("false", "true")
        };
        self.add(
            it.span,
            Operator::BooleanLiteral,
            pair(it.span),
            replacement.to_owned(),
            MutantChange::new(original, replacement),
        );
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        if self.equivalent_labels.contains(&pair(it.span)) {
            return;
        }
        let (empty, filled) = match self.text(it.span.start, it.span.start + 1) {
            "\"" => ("\"\"", "\"fallow\""),
            "'" => ("''", "'fallow'"),
            _ => return,
        };
        let (replacement, change) = if it.value.is_empty() {
            (filled, MutantChange::new(empty, filled))
        } else {
            (empty, MutantChange::new(MutantChange::STRING, empty))
        };
        self.add(
            it.span,
            Operator::StringLiteral,
            pair(it.span),
            replacement.to_owned(),
            change,
        );
    }

    fn visit_reg_exp_literal(&mut self, it: &RegExpLiteral<'a>) {
        let raw = self.text(it.span.start, it.span.end).to_owned();
        for (replacement, change) in crate::regex::mutations(&raw) {
            self.add(
                it.span,
                Operator::RegexLiteral,
                pair(it.span),
                replacement,
                change,
            );
        }
    }

    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        self.visit_template_literal(&it.quasi);
    }

    fn visit_jsx_attribute_value(&mut self, it: &JSXAttributeValue<'a>) {
        if !matches!(it, JSXAttributeValue::StringLiteral(_)) {
            walk::walk_jsx_attribute_value(self, it);
        }
    }

    fn visit_ts_namespace_declaration(&mut self, it: &TSNamespaceDeclaration<'a>) {
        if !it.declare {
            walk::walk_ts_namespace_declaration(self, it);
        }
    }

    // Positions that never produce a mutant.
    fn visit_property_key(&mut self, _it: &PropertyKey<'a>) {}
    fn visit_directive(&mut self, _it: &Directive<'a>) {}
    fn visit_decorator(&mut self, _it: &Decorator<'a>) {}
    fn visit_assignment_target(&mut self, _it: &AssignmentTarget<'a>) {}
    fn visit_simple_assignment_target(&mut self, _it: &SimpleAssignmentTarget<'a>) {}
    fn visit_for_statement_left(&mut self, _it: &ForStatementLeft<'a>) {}
    fn visit_import_expression(&mut self, _it: &ImportExpression<'a>) {}
    fn visit_import_declaration(&mut self, _it: &ImportDeclaration<'a>) {}
    fn visit_export_named_declaration(&mut self, _it: &ExportNamedDeclaration<'a>) {}
    fn visit_export_from_declaration(&mut self, _it: &ExportFromDeclaration<'a>) {}
    fn visit_export_all_declaration(&mut self, _it: &ExportAllDeclaration<'a>) {}
    fn visit_ts_export_assignment(&mut self, _it: &TSExportAssignment<'a>) {}
    fn visit_ts_namespace_export_declaration(&mut self, _it: &TSNamespaceExportDeclaration<'a>) {}
    fn visit_ts_import_equals_declaration(&mut self, _it: &TSImportEqualsDeclaration<'a>) {}
    fn visit_ts_external_module_declaration(&mut self, _it: &TSExternalModuleDeclaration<'a>) {}
    fn visit_ts_global_declaration(&mut self, _it: &TSGlobalDeclaration<'a>) {}
    fn visit_ts_enum_declaration(&mut self, _it: &TSEnumDeclaration<'a>) {}
    fn visit_ts_type_alias_declaration(&mut self, _it: &TSTypeAliasDeclaration<'a>) {}
    fn visit_ts_interface_declaration(&mut self, _it: &TSInterfaceDeclaration<'a>) {}
    fn visit_ts_type(&mut self, _it: &TSType<'a>) {}
    fn visit_ts_type_annotation(&mut self, _it: &TSTypeAnnotation<'a>) {}
    fn visit_ts_type_parameter_declaration(&mut self, _it: &TSTypeParameterDeclaration<'a>) {}
    fn visit_ts_type_parameter_instantiation(&mut self, _it: &TSTypeParameterInstantiation<'a>) {}
}
