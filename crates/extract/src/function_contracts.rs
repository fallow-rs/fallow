//! Call evidence for plain functions.
//!
//! The semantic pass records the parameter contract of each module-scope
//! function binding, each direct call whose callee is an admitted import
//! binding or one of these functions, and each other reference to these
//! functions. A later rule reads the facts to find a parameter that gets the
//! same value at each call. Calls to packages through bindings that are not
//! admitted, and calls to globals, are not stored, so the cost stays low.

use fallow_types::extract::{
    ArgFact, CallSite, CallTarget, ContractIncomplete, FunctionCalls, FunctionContract,
    FunctionContractFacts, ImportInfo, LiteralValue, LocalFunctionEscape, OptionKeyFact, ParamFact,
    ParamKind,
};
use oxc_ast::AstKind;
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingPattern, ClassElement, Declaration,
    ExportDefaultDeclarationKind, Expression, FormalParameter, FormalParameters, Function,
    IdentifierReference, ObjectPattern, Program, PropertyKey, Statement, VariableDeclaration,
    VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::{AstNodes, NodeId, Scoping, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::function_body::BodyRef;
use crate::js_text::JsText as _;

/// The count of argument facts kept for one call. `CallSite.arg_count`
/// keeps the full count, so a later position is unknown, not omitted.
pub const MAX_ARG_FACTS: usize = 16;

/// The longest string literal, in bytes, that an argument fact keeps. A
/// longer literal is `Unknown`, which a rule reads as "not constant", so the
/// cap can only hide a finding, never add one.
pub const MAX_LITERAL_STRING_BYTES: usize = 64;

/// A call site with its callee, before the sites are grouped per callee.
type FlatCall = (CallTarget, Box<str>, CallSite);

/// Collect the call evidence of one parsed program.
pub fn collect(
    semantic: &Semantic<'_>,
    program: &Program<'_>,
    imports: &[ImportInfo],
) -> FunctionContractFacts {
    let ModuleContracts {
        declarations,
        bindings,
        scopes,
        ..
    } = collect_contracts(semantic, program);
    let (values, literal_consts) = values::value_scope(semantic, program, imports);
    let mut calls = Vec::new();
    let mut escapes = Vec::new();
    collect_local_uses(
        semantic,
        &values,
        &bindings,
        &scopes,
        &mut calls,
        &mut escapes,
    );
    collect_import_calls(semantic, &values, imports, &mut calls);
    escapes.sort_by_key(|escape| escape.span_start);
    FunctionContractFacts {
        declarations,
        calls: group_calls(calls),
        escapes,
        template_callers: false,
        direct_eval: calls_direct_eval(semantic),
        literal_consts,
    }
}

/// `true` when an `eval` that no binding shadows is the callee of a call.
fn calls_direct_eval(semantic: &Semantic<'_>) -> bool {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let Some(references) = scoping.root_unresolved_references().get("eval") else {
        return false;
    };
    references.iter().any(|&reference| {
        let node_id = scoping.get_reference(reference).node_id();
        let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
            return false;
        };
        let (outer, outer_span) = unwrap_upward(nodes, node_id, identifier.span);
        matches!(
            nodes.parent_kind(outer),
            AstKind::CallExpression(call) if call.callee.span() == outer_span
        )
    })
}

/// One condition whose test is a parameter of a module-scope function, or the
/// negation of one: `p`, `!p` or `!!p` as the test of an `if`, a `?:` or a
/// loop. This is the condition that [`ParamFact::condition_reads`] counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterCondition {
    /// The module-scope binding name of the function.
    pub function: Box<str>,
    /// The 0-based index of the parameter.
    pub parameter_index: u32,
    /// The byte span of the whole test.
    pub span: (u32, u32),
    /// The count of `!` operators that the test applies to the parameter.
    pub negations: u32,
}

/// The conditions of `source` whose test is a parameter of a module-scope
/// function. A source that does not parse without errors, or that nests
/// deeper than the parse bound, gives none.
#[must_use]
pub fn parameter_conditions(path: &std::path::Path, source: &str) -> Vec<ParameterCondition> {
    let source_type = oxc_span::SourceType::from_path(path).unwrap_or_default();
    crate::parse_bounded(source, || {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return Vec::new();
        }
        let semantic = oxc_semantic::SemanticBuilder::new()
            .with_build_nodes(true)
            .build(&parsed.program)
            .semantic;
        collect_contracts(&semantic, &parsed.program).conditions
    })
    .unwrap_or_default()
}

/// The contracts of one module.
struct ModuleContracts {
    declarations: Vec<FunctionContract>,
    /// Each binding symbol with the index of its contract.
    bindings: Vec<(SymbolId, u32)>,
    /// The self-call scope of each contract.
    scopes: Vec<SelfCallScope>,
    /// The parameter conditions of each contract.
    conditions: Vec<ParameterCondition>,
}

fn collect_contracts(semantic: &Semantic<'_>, program: &Program<'_>) -> ModuleContracts {
    let mut contracts = Vec::new();
    let mut scopes = Vec::new();
    let mut conditions = Vec::new();
    let mut seen: FxHashSet<SymbolId> = FxHashSet::default();
    let mut bindings = Vec::new();
    for candidate in module_functions(program) {
        let Some(symbol) = candidate.symbol else {
            continue;
        };
        if !seen.insert(symbol) {
            continue;
        }
        let Ok(index) = u32::try_from(contracts.len()) else {
            break;
        };
        bindings.push((symbol, index));
        // The name of a named function expression is a second binding of the
        // same function inside its body: `const f = function g() { g(1); }`.
        if let FunctionShape::Function(function) = candidate.shape
            && let Some(inner) = function.id.as_ref().and_then(|id| id.symbol_id.get())
            && inner != symbol
        {
            bindings.push((inner, index));
        }
        let (contract, scope, tests) = build_contract(semantic, &candidate, symbol);
        conditions.extend(tests.into_iter().map(|test| ParameterCondition {
            function: contract.binding_name.clone(),
            parameter_index: test.parameter_index,
            span: (test.span.start, test.span.end),
            negations: test.negations,
        }));
        contracts.push(contract);
        scopes.push(scope);
    }
    ModuleContracts {
        declarations: contracts,
        bindings,
        scopes,
        conditions,
    }
}

/// The parameters that a self-call of one contract can pass through.
struct SelfCallScope {
    /// The span of the function, so a call inside it is a self-call.
    span: Span,
    /// One entry for each parameter: the symbol of a formal identifier
    /// parameter that is not a rest parameter and that the module never
    /// writes, `None` otherwise.
    params: Vec<Option<SymbolId>>,
    /// One entry for each parameter: the recorded option keys of an object
    /// pattern that the module never writes, with the binding of each key.
    keys: Vec<Vec<(Box<str>, SymbolId)>>,
}

impl SelfCallScope {
    /// `true` when a call at `span` is inside the function.
    fn contains(&self, span: Span) -> bool {
        self.span.start <= span.start && span.end <= self.span.end
    }
}

/// Record each read of a local function binding as a call or an escape.
fn collect_local_uses(
    semantic: &Semantic<'_>,
    values: &values::ValueScope,
    bindings: &[(SymbolId, u32)],
    scopes: &[SelfCallScope],
    calls: &mut Vec<FlatCall>,
    escapes: &mut Vec<LocalFunctionEscape>,
) {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    for &(symbol, index) in bindings {
        for reference in scoping.get_resolved_references(symbol) {
            if !reference.is_read() {
                continue;
            }
            let node_id = reference.node_id();
            let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
                continue;
            };
            match local_use(nodes, node_id, identifier.span) {
                LocalUse::Call(call_id) => calls.extend(function_call(
                    nodes,
                    (scoping, values),
                    call_id,
                    (CallTarget::Local(index), String::new()),
                    scopes.get(index as usize),
                )),
                LocalUse::Export => {}
                LocalUse::Escape => escapes.push(LocalFunctionEscape {
                    contract: index,
                    span_start: identifier.span.start,
                }),
            }
        }
    }
}

/// A function that a module-scope binding holds.
struct Candidate<'b, 'a> {
    name: &'b str,
    span_start: u32,
    symbol: Option<SymbolId>,
    shape: FunctionShape<'b, 'a>,
    mutable: bool,
}

#[derive(Clone, Copy)]
enum FunctionShape<'b, 'a> {
    Function(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

impl<'b, 'a> FunctionShape<'b, 'a> {
    fn params(self) -> &'b FormalParameters<'a> {
        match self {
            Self::Function(function) => &function.params,
            Self::Arrow(arrow) => &arrow.params,
        }
    }

    fn span(self) -> Span {
        match self {
            Self::Function(function) => function.span,
            Self::Arrow(arrow) => arrow.span,
        }
    }

    fn is_async(self) -> bool {
        match self {
            Self::Function(function) => function.r#async,
            Self::Arrow(arrow) => arrow.r#async,
        }
    }
}

fn module_functions<'b, 'a>(program: &'b Program<'a>) -> Vec<Candidate<'b, 'a>> {
    let mut candidates = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::FunctionDeclaration(function) => push_function(&mut candidates, function),
            Statement::VariableDeclaration(declaration) => {
                push_variables(&mut candidates, declaration);
            }
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::FunctionDeclaration(function) => {
                    push_function(&mut candidates, function);
                }
                Declaration::VariableDeclaration(declaration) => {
                    push_variables(&mut candidates, declaration);
                }
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &export.declaration
                {
                    push_function(&mut candidates, function);
                }
            }
            _ => {}
        }
    }
    candidates
}

fn push_function<'b, 'a>(candidates: &mut Vec<Candidate<'b, 'a>>, function: &'b Function<'a>) {
    // An overload signature or an ambient declaration has no body. The
    // implementation carries the contract, and `Overload` marks it open.
    if function.body.is_none() {
        return;
    }
    let Some(id) = &function.id else {
        return;
    };
    candidates.push(Candidate {
        name: id.name.as_str(),
        span_start: id.span.start,
        symbol: id.symbol_id.get(),
        shape: FunctionShape::Function(function),
        mutable: false,
    });
}

fn push_variables<'b, 'a>(
    candidates: &mut Vec<Candidate<'b, 'a>>,
    declaration: &'b VariableDeclaration<'a>,
) {
    if declaration.declare {
        return;
    }
    let mutable = matches!(
        declaration.kind,
        VariableDeclarationKind::Let | VariableDeclarationKind::Var
    );
    for declarator in &declaration.declarations {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            continue;
        };
        let shape = match declarator.init.as_ref().map(unwrap_transparent) {
            Some(Expression::ArrowFunctionExpression(arrow)) => FunctionShape::Arrow(arrow),
            Some(Expression::FunctionExpression(function)) => FunctionShape::Function(function),
            _ => continue,
        };
        candidates.push(Candidate {
            name: id.name.as_str(),
            span_start: id.span.start,
            symbol: id.symbol_id.get(),
            shape,
            mutable,
        });
    }
}

/// Parentheses and TypeScript wrappers that do not change the runtime value.
fn unwrap_transparent<'b, 'a>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            _ => return expr,
        };
    }
}

fn build_contract(
    semantic: &Semantic<'_>,
    candidate: &Candidate<'_, '_>,
    symbol: SymbolId,
) -> (FunctionContract, SelfCallScope, Vec<ConditionTest>) {
    let scoping = semantic.scoping();
    let params = candidate.shape.params();
    let (mut facts, param_symbols) = param_facts(scoping, params);
    // The condition reads of each parameter come first, then those of each
    // option key in parameter order.
    let tracked: Vec<Option<SymbolId>> = param_symbols
        .iter()
        .copied()
        .chain(
            facts
                .iter()
                .flat_map(|fact| &fact.keys)
                .map(|key| key_symbol(params, key)),
        )
        .collect();

    let mut scan = BodyScan {
        scoping,
        params: &tracked,
        condition_reads: vec![0; tracked.len()],
        condition_tests: Vec::new(),
        returns_value: false,
        reads_arguments: false,
        calls_eval: false,
        function_depth: 0,
        non_arrow_depth: 0,
    };
    let (body, is_arrow) = match candidate.shape {
        FunctionShape::Function(function) => (function.body.as_deref().map(BodyRef::Block), false),
        FunctionShape::Arrow(arrow) => (Some(BodyRef::arrow(&arrow.body)), true),
    };
    // A default value is evaluated in the scope of the function, so it can
    // read `arguments` and test another parameter.
    walk::walk_formal_parameters(&mut scan, params);
    if let Some(body) = body {
        if matches!(body, BodyRef::Concise(_)) {
            scan.returns_value = true;
        }
        body.visit(&mut scan);
    }
    let (param_reads, key_reads) = scan.condition_reads.split_at(param_symbols.len());
    for (fact, reads) in facts.iter_mut().zip(param_reads) {
        fact.condition_reads = *reads;
    }
    for (key, reads) in facts
        .iter_mut()
        .flat_map(|fact| &mut fact.keys)
        .zip(key_reads)
    {
        key.condition_reads = *reads;
    }

    let incomplete = incomplete_reason(semantic, candidate, symbol, &scan, is_arrow);
    let scope = SelfCallScope {
        span: candidate.shape.span(),
        params: param_symbols
            .iter()
            .zip(&facts)
            // A self-call `f(r)` with a rest parameter `r` passes `[r]`, not `r`.
            .map(|(symbol, fact)| symbol.filter(|_| !fact.written && !fact.is_rest))
            .collect(),
        keys: facts
            .iter()
            .map(|fact| {
                fact.keys
                    .iter()
                    .filter(|key| !key.written)
                    .filter_map(|key| {
                        key_symbol(params, key).map(|symbol| (key.key.clone(), symbol))
                    })
                    .collect()
            })
            .collect(),
    };
    let contract = FunctionContract {
        binding_name: candidate.name.into(),
        span_start: candidate.span_start,
        params: facts,
        returns_value: scan.returns_value,
        is_async: candidate.shape.is_async(),
        incomplete,
    };
    // A tracked index past the parameters is an option key. The tests of a
    // key are not parameter conditions.
    let parameter_count = contract.params.len();
    let tests = scan
        .condition_tests
        .into_iter()
        .filter(|test| {
            usize::try_from(test.parameter_index).is_ok_and(|index| index < parameter_count)
        })
        .collect();
    (contract, scope, tests)
}

/// One fact for each formal parameter, and the symbol of each identifier
/// parameter.
fn param_facts(
    scoping: &Scoping,
    params: &FormalParameters<'_>,
) -> (Vec<ParamFact>, Vec<Option<SymbolId>>) {
    let mut facts = Vec::new();
    let mut symbols = Vec::new();
    for param in &params.items {
        let (name, kind, symbol) = pattern_fact(&param.pattern);
        symbols.push(symbol);
        facts.push(ParamFact {
            name,
            span_start: param.span.start,
            kind,
            optional: param.optional,
            has_default: param.initializer.is_some(),
            is_rest: false,
            condition_reads: 0,
            written: pattern_written(scoping, &param.pattern),
            keys: option_keys(scoping, param),
        });
    }
    if let Some(rest) = &params.rest {
        let (name, kind, symbol) = pattern_fact(&rest.rest.argument);
        symbols.push(symbol);
        facts.push(ParamFact {
            name,
            span_start: rest.span.start,
            kind,
            optional: false,
            has_default: false,
            is_rest: true,
            condition_reads: 0,
            written: pattern_written(scoping, &rest.rest.argument),
            keys: Vec::new(),
        });
    }
    (facts, symbols)
}

/// `true` when the module writes any binding of a parameter pattern.
///
/// A `var` or function declaration in the body that redeclares the parameter
/// can replace its value without a write reference, so each redeclaration
/// also counts as a write.
fn pattern_written(scoping: &Scoping, pattern: &BindingPattern<'_>) -> bool {
    pattern.get_binding_identifiers().iter().any(|id| {
        id.symbol_id.get().is_some_and(|symbol| {
            !scoping.symbol_redeclarations(symbol).is_empty()
                || scoping
                    .get_resolved_references(symbol)
                    .any(oxc_semantic::Reference::is_write)
        })
    })
}

/// The recorded option keys of an object-pattern parameter.
fn option_keys(scoping: &Scoping, param: &FormalParameter<'_>) -> Vec<OptionKeyFact> {
    let Some((pattern, pattern_default)) = object_pattern(&param.pattern) else {
        return Vec::new();
    };
    // A default other than `{}` can set a key that no caller passes.
    if let Some(default) = param.initializer.as_deref().or(pattern_default)
        && !matches!(
            unwrap_transparent(default),
            Expression::ObjectExpression(object) if object.properties.is_empty()
        )
    {
        return Vec::new();
    }
    let mut keys: Vec<OptionKeyFact> = Vec::new();
    for property in &pattern.properties {
        if property.computed {
            continue;
        }
        let key = match &property.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().to_string(),
            PropertyKey::StringLiteral(literal) => literal.value.text().to_string(),
            _ => continue,
        };
        if values::PROTOTYPE_KEYS.contains(&key.as_str()) || key.len() > values::MAX_KEY_BYTES {
            continue;
        }
        let (id, has_default) = match &property.value {
            BindingPattern::BindingIdentifier(id) => (id, false),
            BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
                BindingPattern::BindingIdentifier(id) => (id, true),
                _ => continue,
            },
            _ => continue,
        };
        if keys.iter().any(|existing| *existing.key == *key) {
            continue;
        }
        keys.push(OptionKeyFact {
            key: key.into(),
            binding: id.name.as_str().into(),
            span_start: id.span.start,
            has_default,
            condition_reads: 0,
            written: id.symbol_id.get().is_none_or(|symbol| {
                !scoping.symbol_redeclarations(symbol).is_empty()
                    || scoping
                        .get_resolved_references(symbol)
                        .any(oxc_semantic::Reference::is_write)
            }),
        });
    }
    keys
}

/// The object pattern of a parameter, with the default that an assignment
/// pattern around it holds.
fn object_pattern<'b, 'a>(
    pattern: &'b BindingPattern<'a>,
) -> Option<(&'b ObjectPattern<'a>, Option<&'b Expression<'a>>)> {
    match pattern {
        BindingPattern::ObjectPattern(object) => Some((object, None)),
        BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
            BindingPattern::ObjectPattern(object) => Some((object, Some(&assignment.right))),
            _ => None,
        },
        _ => None,
    }
}

/// The symbol of the binding of an option key, found by its start byte.
fn key_symbol(params: &FormalParameters<'_>, key: &OptionKeyFact) -> Option<SymbolId> {
    params.items.iter().find_map(|param| {
        param
            .pattern
            .get_binding_identifiers()
            .into_iter()
            .find(|id| id.span.start == key.span_start)
            .and_then(|id| id.symbol_id.get())
    })
}

fn pattern_fact(pattern: &BindingPattern<'_>) -> (Option<Box<str>>, ParamKind, Option<SymbolId>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => (
            Some(id.name.as_str().into()),
            ParamKind::Identifier,
            id.symbol_id.get(),
        ),
        BindingPattern::ObjectPattern(_) => (None, ParamKind::Object, None),
        BindingPattern::ArrayPattern(_) => (None, ParamKind::Array, None),
        BindingPattern::AssignmentPattern(assignment) => pattern_fact(&assignment.left),
    }
}

fn incomplete_reason(
    semantic: &Semantic<'_>,
    candidate: &Candidate<'_, '_>,
    symbol: SymbolId,
    scan: &BodyScan<'_, '_>,
    is_arrow: bool,
) -> Option<ContractIncomplete> {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let mut bodies = 0usize;
    let mut signatures = 0usize;
    let mut others = 0usize;
    for declaration in scoping.symbol_declarations(symbol) {
        match nodes.kind(declaration) {
            AstKind::Function(function) if function.body.is_some() => bodies += 1,
            AstKind::Function(_) => signatures += 1,
            _ => others += 1,
        }
    }
    let params = candidate.shape.params();
    let has_decorator = params
        .items
        .iter()
        .any(|param| !param.decorators.is_empty())
        || params
            .rest
            .as_ref()
            .is_some_and(|rest| !rest.decorators.is_empty());
    let generator =
        matches!(candidate.shape, FunctionShape::Function(function) if function.generator);
    if signatures > 0 && bodies == 1 && others == 0 {
        Some(ContractIncomplete::Overload)
    } else if bodies + others > 1 {
        Some(ContractIncomplete::DuplicateDeclaration)
    } else if candidate.mutable {
        Some(ContractIncomplete::MutableBinding)
    } else if scoping
        .get_resolved_references(symbol)
        .any(oxc_semantic::Reference::is_write)
    {
        Some(ContractIncomplete::Reassigned)
    } else if generator {
        Some(ContractIncomplete::Generator)
    } else if has_decorator {
        Some(ContractIncomplete::Decorator)
    } else if scan.reads_arguments && !is_arrow {
        Some(ContractIncomplete::ArgumentsRead)
    } else if scan.calls_eval {
        Some(ContractIncomplete::DirectEval)
    } else {
        None
    }
}

/// A test that reads a tracked symbol, before the contract has a name.
struct ConditionTest {
    /// The index into the tracked symbols: each parameter, then each option
    /// key.
    parameter_index: u32,
    span: Span,
    negations: u32,
}

/// One walk over a function body.
struct BodyScan<'s, 'p> {
    scoping: &'s Scoping,
    params: &'p [Option<SymbolId>],
    condition_reads: Vec<u32>,
    /// Each test that `condition_reads` counts.
    condition_tests: Vec<ConditionTest>,
    returns_value: bool,
    reads_arguments: bool,
    /// A direct `eval` can read `arguments` and assign a parameter at any
    /// depth.
    calls_eval: bool,
    /// Nesting of functions and arrows below the scanned function.
    function_depth: u32,
    /// Nesting of non-arrow functions, which bind their own `arguments`.
    non_arrow_depth: u32,
}

impl<'a> BodyScan<'_, '_> {
    /// The parts of a class element that run in the enclosing scope.
    fn visit_class_element_outer(&mut self, element: &ClassElement<'a>) {
        let (decorators, key, computed) = match element {
            ClassElement::MethodDefinition(method) => {
                (&method.decorators, &method.key, method.computed)
            }
            ClassElement::PropertyDefinition(property) => {
                (&property.decorators, &property.key, property.computed)
            }
            ClassElement::AccessorProperty(accessor) => {
                (&accessor.decorators, &accessor.key, accessor.computed)
            }
            ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => return,
        };
        self.visit_decorators(decorators);
        if computed {
            self.visit_property_key(key);
        }
        // A parameter decorator of a method runs when the class is defined.
        if let ClassElement::MethodDefinition(method) = element {
            for param in &method.value.params.items {
                self.visit_decorators(&param.decorators);
            }
        }
    }

    /// The parts of a class element that bind their own `arguments`.
    fn visit_class_element_inner(&mut self, element: &ClassElement<'a>) {
        match element {
            ClassElement::MethodDefinition(method) => {
                self.visit_function(&method.value, ScopeFlags::Function);
            }
            ClassElement::PropertyDefinition(property) => {
                if let Some(value) = &property.value {
                    self.visit_expression(value);
                }
            }
            ClassElement::AccessorProperty(accessor) => {
                if let Some(value) = &accessor.value {
                    self.visit_expression(value);
                }
            }
            ClassElement::StaticBlock(block) => self.visit_static_block(block),
            ClassElement::TSIndexSignature(_) => {}
        }
    }
}

impl BodyScan<'_, '_> {
    fn count_test(&mut self, test: &Expression<'_>) {
        let mut expr = unwrap_transparent(test);
        let mut negations = 0;
        while let Expression::UnaryExpression(unary) = expr
            && unary.operator == oxc_syntax::operator::UnaryOperator::LogicalNot
        {
            expr = unwrap_transparent(&unary.argument);
            negations += 1;
        }
        let Expression::Identifier(identifier) = expr else {
            return;
        };
        let Some(symbol) = self.resolve(identifier) else {
            return;
        };
        let Some(index) = self.params.iter().position(|param| *param == Some(symbol)) else {
            return;
        };
        self.condition_reads[index] += 1;
        if let Ok(parameter_index) = u32::try_from(index) {
            self.condition_tests.push(ConditionTest {
                parameter_index,
                span: test.span(),
                negations,
            });
        }
    }

    fn resolve(&self, identifier: &IdentifierReference<'_>) -> Option<SymbolId> {
        let reference = identifier.reference_id.get()?;
        self.scoping.get_reference(reference).symbol_id()
    }
}

impl<'a> Visit<'a> for BodyScan<'_, '_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.function_depth += 1;
        self.non_arrow_depth += 1;
        walk::walk_function(self, function, flags);
        self.non_arrow_depth -= 1;
        self.function_depth -= 1;
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, arrow);
        self.function_depth -= 1;
    }

    fn visit_class(&mut self, class: &oxc_ast::ast::Class<'a>) {
        // Decorators, the `extends` clause and computed keys run in the
        // enclosing scope, so they read its `arguments`. Methods and field
        // initializers bind their own `arguments`, and their returns are not
        // returns of the scanned function.
        self.visit_decorators(&class.decorators);
        if let Some(heritage) = &class.heritage {
            self.visit_expression(&heritage.expression);
        }
        for element in &class.body.body {
            self.visit_class_element_outer(element);
        }
        self.function_depth += 1;
        self.non_arrow_depth += 1;
        for element in &class.body.body {
            self.visit_class_element_inner(element);
        }
        self.non_arrow_depth -= 1;
        self.function_depth -= 1;
    }

    fn visit_return_statement(&mut self, statement: &oxc_ast::ast::ReturnStatement<'a>) {
        if self.function_depth == 0 && statement.argument.is_some() {
            self.returns_value = true;
        }
        walk::walk_return_statement(self, statement);
    }

    fn visit_call_expression(&mut self, call: &oxc_ast::ast::CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && callee.name == "eval"
            && self.resolve(callee).is_none()
        {
            self.calls_eval = true;
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if self.non_arrow_depth == 0
            && identifier.name == "arguments"
            && self.resolve(identifier).is_none()
        {
            self.reads_arguments = true;
        }
    }

    fn visit_if_statement(&mut self, statement: &oxc_ast::ast::IfStatement<'a>) {
        self.count_test(&statement.test);
        walk::walk_if_statement(self, statement);
    }

    fn visit_conditional_expression(&mut self, expr: &oxc_ast::ast::ConditionalExpression<'a>) {
        self.count_test(&expr.test);
        walk::walk_conditional_expression(self, expr);
    }

    fn visit_while_statement(&mut self, statement: &oxc_ast::ast::WhileStatement<'a>) {
        self.count_test(&statement.test);
        walk::walk_while_statement(self, statement);
    }

    fn visit_do_while_statement(&mut self, statement: &oxc_ast::ast::DoWhileStatement<'a>) {
        self.count_test(&statement.test);
        walk::walk_do_while_statement(self, statement);
    }

    fn visit_for_statement(&mut self, statement: &oxc_ast::ast::ForStatement<'a>) {
        if let Some(test) = &statement.test {
            self.count_test(test);
        }
        walk::walk_for_statement(self, statement);
    }
}

enum LocalUse {
    Call(NodeId),
    Export,
    Escape,
}

/// Classify a read of a local function binding.
fn local_use(nodes: &AstNodes<'_>, node_id: NodeId, span: Span) -> LocalUse {
    let (outer, outer_span) = unwrap_upward(nodes, node_id, span);
    let parent = nodes.parent_id(outer);
    match nodes.kind(parent) {
        AstKind::CallExpression(call)
            if call.callee.span() == outer_span
                && !call.optional
                && !matches!(nodes.parent_kind(parent), AstKind::ChainExpression(_)) =>
        {
            LocalUse::Call(parent)
        }
        // The opening element of the same JSX element records the use of a
        // closing element.
        AstKind::ExportSpecifier(_)
        | AstKind::ExportDefaultDeclaration(_)
        | AstKind::JSXClosingElement(_) => LocalUse::Export,
        _ => LocalUse::Escape,
    }
}

/// Walk up through parentheses and the TypeScript expression wrappers that
/// do not change the runtime value.
fn unwrap_upward(nodes: &AstNodes<'_>, node_id: NodeId, span: Span) -> (NodeId, Span) {
    let mut current = node_id;
    let mut current_span = span;
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        let parent_span = match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(node) => node.span,
            AstKind::TSAsExpression(node) if node.expression.span() == current_span => node.span,
            AstKind::TSSatisfiesExpression(node) if node.expression.span() == current_span => {
                node.span
            }
            AstKind::TSNonNullExpression(node) => node.span,
            AstKind::TSInstantiationExpression(node) if node.expression.span() == current_span => {
                node.span
            }
            _ => break,
        };
        current = parent;
        current_span = parent_span;
    }
    (current, current_span)
}

/// Record each direct call whose callee root is an admitted import binding.
fn collect_import_calls(
    semantic: &Semantic<'_>,
    values: &values::ValueScope,
    imports: &[ImportInfo],
    calls: &mut Vec<FlatCall>,
) {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    for (index, import) in imports.iter().enumerate() {
        let Ok(import_index) = u32::try_from(index) else {
            break;
        };
        let Some(symbol) = crate::binding_references::admitted_import_symbol(semantic, import)
        else {
            continue;
        };
        for reference in scoping.get_resolved_references(symbol) {
            if !reference.is_value() {
                continue;
            }
            let node_id = reference.node_id();
            let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
                continue;
            };
            let (chain, chain_span, path) = static_member_chain(nodes, node_id, identifier.span);
            let LocalUse::Call(call_id) = local_use(nodes, chain, chain_span) else {
                continue;
            };
            if let Some(call) = function_call(
                nodes,
                (scoping, values),
                call_id,
                (CallTarget::Import(import_index), path),
                None,
            ) {
                calls.push(call);
            }
        }
    }
}

/// The outermost static member chain over a root identifier, with its
/// dot-separated member suffix.
fn static_member_chain(
    nodes: &AstNodes<'_>,
    node_id: NodeId,
    span: Span,
) -> (NodeId, Span, String) {
    let mut current = node_id;
    let mut current_span = span;
    let mut path = String::new();
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        let AstKind::StaticMemberExpression(member) = nodes.kind(parent) else {
            break;
        };
        if member.object.span() != current_span || member.optional {
            break;
        }
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(member.property.name.as_str());
        current = parent;
        current_span = member.span;
    }
    (current, current_span, path)
}

/// The call site at `call_id`. `scope` is the self-call scope of a local
/// target, so a self-call can record a pass-through parameter.
fn function_call(
    nodes: &AstNodes<'_>,
    (scoping, values): (&Scoping, &values::ValueScope),
    call_id: NodeId,
    (target, member_path): (CallTarget, String),
    scope: Option<&SelfCallScope>,
) -> Option<FlatCall> {
    let AstKind::CallExpression(call) = nodes.kind(call_id) else {
        return None;
    };
    let self_call = scope.filter(|scope| scope.contains(call.span));
    let arg_count = u32::try_from(call.arguments.len()).unwrap_or(u32::MAX);
    let spread_from = call
        .arguments
        .iter()
        .position(Argument::is_spread)
        .and_then(|index| u32::try_from(index).ok());
    let mut args: Vec<ArgFact> = call
        .arguments
        .iter()
        .take(spread_from.map_or(MAX_ARG_FACTS, |index| MAX_ARG_FACTS.min(index as usize)))
        .enumerate()
        .map(|(position, argument)| {
            let context = ArgContext {
                scoping,
                values,
                pass_through: self_call.map_or(&[][..], |scope| &scope.params),
                pass_through_keys: self_call
                    .and_then(|scope| scope.keys.get(position))
                    .map_or(&[][..], Vec::as_slice),
            };
            arg_fact(&context, argument)
        })
        .collect();
    if args.iter().all(|arg| *arg == ArgFact::Unknown) {
        args = Vec::new();
    }
    let site = CallSite {
        arg_count,
        args,
        spread_from,
        result_used: result_used(nodes, call_id),
        span_start: call.span.start,
    };
    Some((target, member_path.into_boxed_str(), site))
}

/// Group the sites per callee. The groups follow their first site, and the
/// sites of a group follow source order.
fn group_calls(mut flat: Vec<FlatCall>) -> Vec<FunctionCalls> {
    flat.sort_by_key(|(_, _, site)| site.span_start);
    let mut groups: Vec<FunctionCalls> = Vec::new();
    let mut index: FxHashMap<(CallTarget, Box<str>), usize> = FxHashMap::default();
    for (target, member_path, site) in flat {
        match index.entry((target, member_path)) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                groups[*entry.get()].sites.push(site);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let (target, member_path) = entry.key().clone();
                entry.insert(groups.len());
                groups.push(FunctionCalls {
                    target,
                    member_path,
                    sites: vec![site],
                });
            }
        }
    }
    groups
}

/// What the fact of one argument depends on.
struct ArgContext<'s, 'v> {
    scoping: &'s Scoping,
    values: &'v values::ValueScope,
    /// The parameters that a self-call can pass on unchanged. Empty for each
    /// other call.
    pass_through: &'s [Option<SymbolId>],
    /// The option keys of the parameter at the position of the argument
    /// that a self-call can pass on unchanged. Empty for each other call.
    pass_through_keys: &'s [(Box<str>, SymbolId)],
}

/// The fact of one argument.
fn arg_fact(context: &ArgContext<'_, '_>, argument: &Argument<'_>) -> ArgFact {
    let Some(expr) = argument.as_expression() else {
        return ArgFact::Unknown;
    };
    let scoping = context.scoping;
    let expr = unwrap_transparent(expr);
    if let Some(literal) = literal_value(scoping, expr) {
        return ArgFact::Literal(literal);
    }
    if let Expression::ObjectExpression(object) = expr {
        return values::object_arg(scoping, context.values, object, context.pass_through_keys);
    }
    if let Some(index) = pass_through_param(scoping, expr, context.pass_through) {
        return ArgFact::Param(index);
    }
    context
        .values
        .binding(scoping, expr)
        .map_or(ArgFact::Unknown, ArgFact::Binding)
}

/// The index of the pass-through parameter that `expr` names.
fn pass_through_param(
    scoping: &Scoping,
    expr: &Expression<'_>,
    pass_through: &[Option<SymbolId>],
) -> Option<u32> {
    let Expression::Identifier(identifier) = expr else {
        return None;
    };
    let symbol = scoping
        .get_reference(identifier.reference_id.get()?)
        .symbol_id()?;
    let index = pass_through
        .iter()
        .position(|param| *param == Some(symbol))?;
    u32::try_from(index).ok()
}

fn literal_value(scoping: &Scoping, expr: &Expression<'_>) -> Option<LiteralValue> {
    match expr {
        Expression::BooleanLiteral(literal) => Some(LiteralValue::Boolean(literal.value)),
        Expression::NullLiteral(_) => Some(LiteralValue::Null),
        Expression::NumericLiteral(literal) => Some(number(literal.value)),
        Expression::UnaryExpression(unary)
            if unary.operator == oxc_syntax::operator::UnaryOperator::UnaryNegation =>
        {
            match unwrap_transparent(&unary.argument) {
                Expression::NumericLiteral(literal) => Some(number(-literal.value)),
                _ => None,
            }
        }
        Expression::StringLiteral(literal) => string(&literal.value.text()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .and_then(|cooked| string(&cooked.text())),
        Expression::Identifier(identifier) if identifier.name == "undefined" => {
            let resolved = identifier
                .reference_id
                .get()
                .and_then(|reference| scoping.get_reference(reference).symbol_id());
            resolved.is_none().then_some(LiteralValue::Undefined)
        }
        _ => None,
    }
}

fn string(value: &str) -> Option<LiteralValue> {
    (value.len() <= MAX_LITERAL_STRING_BYTES).then(|| LiteralValue::String(value.into()))
}

fn number(value: f64) -> LiteralValue {
    LiteralValue::Number(value.to_bits())
}

/// `false` when the call, under parentheses, `await` or `void`, is the whole
/// expression of a statement.
fn result_used(nodes: &AstNodes<'_>, call_id: NodeId) -> bool {
    let mut current = call_id;
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            return true;
        }
        match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(_) | AstKind::AwaitExpression(_) => current = parent,
            AstKind::UnaryExpression(unary)
                if unary.operator == oxc_syntax::operator::UnaryOperator::Void =>
            {
                current = parent;
            }
            AstKind::ExpressionStatement(_) => return false,
            _ => return true,
        }
    }
}

#[path = "function_contracts_values.rs"]
mod values;

#[cfg(test)]
#[path = "function_contracts_tests.rs"]
mod tests;
