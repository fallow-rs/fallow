//! The equivalence rule of a conditional lookup before `.filter(Boolean)`.
//! See `FilteredLookups`.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    BindingPattern, CallExpression, ChainElement, Declaration, ExportDeclaration, Expression,
    IdentifierReference, ObjectPropertyKind, Program, PropertyKey, PropertyKind,
    TaggedTemplateExpression, UnaryExpression, VariableDeclaration, VariableDeclarationKind,
    WithStatement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Semantic;
use oxc_span::GetSpan;
use oxc_syntax::operator::UnaryOperator;
use oxc_syntax::reference::ReferenceId;
use oxc_syntax::symbol::SymbolId;

use super::{callee_without_wrappers, pair};

/// The text that a file must contain to hold a `FilteredLookups` site. The
/// check is a cheap gate for the semantic build. A file that spells the call
/// another way only keeps its mutant.
pub(super) const FILTER_BOOLEAN: &str = "filter(Boolean)";

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
pub(super) struct FilteredLookups<'s, 'b> {
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
    pub(super) fn tests(program: &Program<'_>, semantic: &'s Semantic<'b>) -> BTreeSet<(u32, u32)> {
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
