//! Put the value of a same-file constant in the config positions that name it.
//!
//! A config often keeps a part of its value in a top-level constant:
//! `const UNIT = { test: { setupFiles: [...] } }` and later
//! `projects: [UNIT]`, or `setupFiles: SETUP`. The config walkers read
//! literal objects, arrays and strings only. This pass replaces each such
//! identifier in a property value or an array element with a copy of the
//! constant's literal value, so that every walker reads it without change.

use oxc_allocator::{Allocator, CloneIn};
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use super::top_level_variable_declarations;

/// The maximum number of constants that one inlined value can pass through,
/// as in `const A = {...}; const B = [A]; export default { x: B }`.
const MAX_INLINE_NESTING: usize = 4;

/// Replace each identifier in a property value or an array element that names
/// a stable top-level constant with a literal value.
///
/// A constant qualifies when one top-level `const` or `let` is the only
/// binding of its name, no expression writes to it, and its value is an
/// object, array or string literal.
/// The copied source bytes stay at or below `source_len`, so the AST grows at
/// most to two times the size of the parsed source.
pub(super) fn inline_value_constants<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    source_len: usize,
) {
    let values = constant_values(allocator, program);
    if values.is_empty() {
        return;
    }
    let mut inliner = Inliner {
        allocator,
        values: &values,
        nesting: 0,
        budget: source_len,
    };
    inliner.visit_program(program);
}

/// A copy of the literal value of each stable top-level constant, by name.
fn constant_values<'a>(
    allocator: &'a Allocator,
    program: &Program<'a>,
) -> FxHashMap<String, Expression<'a>> {
    let candidates: Vec<_> = top_level_variable_declarations(program)
        .flat_map(|decl| &decl.declarations)
        .filter_map(|declarator| {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                return None;
            };
            let value = declarator.init.as_ref().and_then(literal_value)?;
            Some((id.name.as_str(), value))
        })
        .collect();
    if candidates.is_empty() {
        return FxHashMap::default();
    }
    let stable = stable_bindings(program, candidates.iter().map(|(name, _)| *name));
    candidates
        .into_iter()
        .filter(|(name, _)| stable.contains(name))
        .map(|(name, value)| (name.to_string(), value.clone_in(allocator)))
        .collect()
}

/// The literal value of a constant initializer, through parentheses and type
/// assertions.
///
/// The value can hold functions and writes. A copy of one adds a binding or a
/// write only for a name that the source already binds or writes inside it, so
/// a later stable-binding check of the same program gives the same answer.
fn literal_value<'e, 'a>(init: &'e Expression<'a>) -> Option<&'e Expression<'a>> {
    match init {
        Expression::ParenthesizedExpression(paren) => literal_value(&paren.expression),
        Expression::TSAsExpression(ts_as) => literal_value(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => literal_value(&ts_sat.expression),
        Expression::StringLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_) => Some(init),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => Some(init),
        _ => None,
    }
}

struct Inliner<'a, 'v> {
    allocator: &'a Allocator,
    values: &'v FxHashMap<String, Expression<'a>>,
    nesting: usize,
    budget: usize,
}

impl<'a> Inliner<'a, '_> {
    /// A copy of the constant value that `expr` names, when the nesting and
    /// size limits allow one more copy.
    fn copy_for(&mut self, expr: &Expression<'a>) -> Option<Expression<'a>> {
        if self.nesting >= MAX_INLINE_NESTING {
            return None;
        }
        let Expression::Identifier(id) = expr else {
            return None;
        };
        let value = self.values.get(id.name.as_str())?;
        let size = value.span().size() as usize;
        self.budget = self.budget.checked_sub(size)?;
        Some(value.clone_in(self.allocator))
    }
}

impl<'a> VisitMut<'a> for Inliner<'a, '_> {
    fn visit_object_property(&mut self, prop: &mut ObjectProperty<'a>) {
        let Some(value) = self.copy_for(&prop.value) else {
            walk_mut::walk_object_property(self, prop);
            return;
        };
        prop.value = value;
        prop.shorthand = false;
        self.nesting += 1;
        walk_mut::walk_object_property(self, prop);
        self.nesting -= 1;
    }

    fn visit_array_expression_element(&mut self, element: &mut ArrayExpressionElement<'a>) {
        let copy = element.as_expression().and_then(|expr| self.copy_for(expr));
        let Some(value) = copy else {
            walk_mut::walk_array_expression_element(self, element);
            return;
        };
        *element = ArrayExpressionElement::from(value);
        self.nesting += 1;
        walk_mut::walk_array_expression_element(self, element);
        self.nesting -= 1;
    }
}

/// Whether one top-level `const` or `let` is the only binding of `name` in the
/// program, and no expression writes to it or to one of its members.
pub(super) fn holds_one_stable_object(program: &Program<'_>, name: &str) -> bool {
    !stable_bindings(program, [name]).is_empty()
}

/// The subset of `names` that [`holds_one_stable_object`] accepts, from one
/// walk of the program.
fn stable_bindings<'n>(
    program: &Program<'_>,
    names: impl IntoIterator<Item = &'n str>,
) -> FxHashSet<&'n str> {
    let mut usage = NameUsage::new(
        names
            .into_iter()
            .filter(|name| declares_top_level_const_or_let(program, name)),
    );
    if usage.counts.is_empty() {
        return FxHashSet::default();
    }
    usage.visit_program(program);
    usage
        .counts
        .into_iter()
        .filter(|(_, count)| count.bindings == 1 && count.writes == 0)
        .map(|(name, _)| name)
        .collect()
}

/// Whether a top-level `const` or `let` statement declares `name`. A `var` is
/// function scoped and hoisted, so a later statement can hold its value.
fn declares_top_level_const_or_let(program: &Program<'_>, name: &str) -> bool {
    top_level_variable_declarations(program).any(|decl| {
        matches!(
            decl.kind,
            VariableDeclarationKind::Const | VariableDeclarationKind::Let
        ) && decl.declarations.iter().any(|declarator| {
            matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if id.name == name)
        })
    })
}

/// What one program does with one name: the number of bindings that declare it,
/// and the number of expressions that write to it or to one of its members.
#[derive(Default)]
struct NameCount {
    bindings: usize,
    writes: usize,
}

/// The [`NameCount`] of each name in a set, from one walk of a program.
struct NameUsage<'n> {
    counts: FxHashMap<&'n str, NameCount>,
    write_depth: usize,
}

impl<'n> NameUsage<'n> {
    fn new(names: impl IntoIterator<Item = &'n str>) -> Self {
        Self {
            counts: names
                .into_iter()
                .map(|name| (name, NameCount::default()))
                .collect(),
            write_depth: 0,
        }
    }
}

impl<'a> Visit<'a> for NameUsage<'_> {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        if let Some(count) = self.counts.get_mut(identifier.name.as_str()) {
            count.bindings += 1;
        }
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if self.write_depth > 0
            && let Some(count) = self.counts.get_mut(identifier.name.as_str())
        {
            count.writes += 1;
        }
    }

    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        self.write_depth += 1;
        walk::walk_assignment_target(self, target);
        self.write_depth -= 1;
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        self.write_depth += 1;
        walk::walk_update_expression(self, expression);
        self.write_depth -= 1;
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if expression.operator != UnaryOperator::Delete {
            walk::walk_unary_expression(self, expression);
            return;
        }
        self.write_depth += 1;
        walk::walk_unary_expression(self, expression);
        self.write_depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{
        extract_config_array_nested_string_or_array, extract_config_string,
        extract_config_string_or_array,
    };

    fn ts_path() -> &'static Path {
        Path::new("config.ts")
    }

    fn js_path() -> &'static Path {
        Path::new("config.js")
    }

    #[test]
    fn same_file_constants_resolve_in_values_and_array_elements() {
        let source = r#"
            const SETUP = "./a.ts";
            const MORE = ["./b.ts", SETUP] as const;
            const ITEM = { files: MORE } satisfies Item;
            export default { items: [ITEM], single: SETUP };
        "#;
        assert_eq!(
            extract_config_array_nested_string_or_array(source, ts_path(), &["items"], &["files"]),
            vec!["./b.ts", "./a.ts"]
        );
        assert_eq!(
            extract_config_string(source, ts_path(), &["single"]),
            Some("./a.ts".to_string())
        );
    }

    #[test]
    fn constant_with_a_function_resolves_and_unstable_constants_do_not() {
        let source = r#"
            const WITH_FN = { files: ["./a.ts"], load: (x) => x };
            const SHADOWED = ["./b.ts"];
            function f(SHADOWED) { return SHADOWED; }
            const SELF = [SELF];
            export default { a: WITH_FN, b: SHADOWED, c: SELF };
        "#;
        assert_eq!(
            extract_config_string_or_array(source, js_path(), &["a", "files"]),
            vec!["./a.ts"]
        );
        assert!(extract_config_string_or_array(source, js_path(), &["b"]).is_empty());
        assert!(extract_config_string_or_array(source, js_path(), &["c"]).is_empty());
    }
}
