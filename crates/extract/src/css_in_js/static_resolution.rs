//! Static value resolution state for the StyleX consumer scan.

use oxc_ast::ast::{Expression, IdentifierReference};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use super::object::Lib;
use super::tokens::{is_static_stylex_theme_override, unwrap_transparent_expression};

#[cfg(test)]
thread_local! {
    static RESOLUTION_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, not(miri)))]
#[path = "static_resolution_tests.rs"]
mod tests;

/// Most constants that one static value resolution expands. Shared values
/// make the expansion a graph, so without a budget a chain of constants that
/// each read the previous one twice expands exponentially.
const MAX_STATIC_RESOLUTION_STEPS: usize = 10_000;

/// Deepest nesting of constant expansions in one static value resolution.
const MAX_STATIC_RESOLUTION_DEPTH: usize = 64;

/// The constants that a static value resolution is expanding, with a step
/// budget and memos of finished checks.
///
/// A constant can only read constants declared before it, so a finished
/// check of a constant does not depend on the expansion that reached it. A
/// check that reached a limit depends on it, so the memos skip that result.
#[derive(Clone, Default)]
pub(super) struct StaticVisit<'a> {
    active: FxHashSet<&'a str>,
    /// Expansions so far. Clones share the count.
    steps: std::rc::Rc<std::cell::Cell<usize>>,
    /// The current check reached a limit. Clones share the flag.
    capped: std::rc::Rc<std::cell::Cell<bool>>,
    primitive: FxHashMap<&'a str, bool>,
    bigint: FxHashMap<&'a str, bool>,
    theme_override: FxHashMap<&'a str, bool>,
}

impl<'a> StaticVisit<'a> {
    /// Start the expansion of `name`. Returns `false` when `name` is already
    /// expanding, or when the resolution reached its depth or step limit.
    pub(super) fn insert(&mut self, name: &'a str) -> bool {
        #[cfg(test)]
        RESOLUTION_STEPS.with(|steps| steps.set(steps.get() + 1));
        let steps = self.steps.get();
        if steps >= MAX_STATIC_RESOLUTION_STEPS || self.active.len() >= MAX_STATIC_RESOLUTION_DEPTH
        {
            self.capped.set(true);
            return false;
        }
        self.steps.set(steps + 1);
        self.active.insert(name)
    }

    /// Start a check whose result can go in a memo. Returns the limit state
    /// of the enclosing check, for [`Self::finish_check`].
    pub(super) fn start_check(&self) -> bool {
        self.capped.replace(false)
    }

    /// Finish the check that [`Self::start_check`] started. Returns `true`
    /// when the check reached no limit, so its result can go in a memo.
    fn finish_check(&self, outer_capped: bool) -> bool {
        let capped = self.capped.get();
        self.capped.set(outer_capped || capped);
        !capped
    }

    pub(super) fn remove(&mut self, name: &str) {
        self.active.remove(name);
    }

    /// The memoized BigInt check of `name`. A function parameter can shadow
    /// the constant, so a check with parameters in scope has no memo.
    pub(super) fn bigint_memo(&self, name: &str, with_parameters: bool) -> Option<bool> {
        if with_parameters {
            return None;
        }
        self.bigint.get(name).copied()
    }

    /// Finish the BigInt check of `name` that [`Self::start_check`] started,
    /// and memoize its result when the memo can hold it.
    pub(super) fn memoize_bigint(
        &mut self,
        name: &'a str,
        with_parameters: bool,
        outer_capped: bool,
        is_bigint: bool,
    ) {
        if self.finish_check(outer_capped) && !with_parameters {
            self.bigint.insert(name, is_bigint);
        }
    }
}

pub(super) fn is_definitely_static_primitive<'a>(
    expression: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut StaticVisit<'a>,
) -> bool {
    match unwrap_transparent_expression(expression) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::TemplateLiteral(template) => template.expressions.iter().all(|expression| {
            is_definitely_static_primitive(
                expression,
                before,
                static_values,
                root_reference_spans,
                visiting,
            )
        }),
        Expression::UnaryExpression(unary) => {
            !unary.operator.is_delete()
                && is_definitely_static_primitive(
                    &unary.argument,
                    before,
                    static_values,
                    root_reference_spans,
                    visiting,
                )
        }
        Expression::BinaryExpression(binary) => is_static_stylex_primitive_pair(
            &binary.left,
            &binary.right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::LogicalExpression(logical) => is_static_stylex_primitive_pair(
            &logical.left,
            &logical.right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::ConditionalExpression(conditional) => is_static_stylex_primitive_pair(
            &conditional.consequent,
            &conditional.alternate,
            before,
            static_values,
            root_reference_spans,
            visiting,
        ),
        Expression::SequenceExpression(sequence) => {
            sequence.expressions.last().is_some_and(|expression| {
                is_definitely_static_primitive(
                    expression,
                    before,
                    static_values,
                    root_reference_spans,
                    visiting,
                )
            })
        }
        Expression::Identifier(identifier) => {
            if !root_reference_spans.contains(&identifier.span) {
                return false;
            }
            let name = identifier.name.as_str();
            let Some(&(declaration_start, value)) = static_values.get(name) else {
                return false;
            };
            if declaration_start >= before {
                return false;
            }
            if let Some(&is_primitive) = visiting.primitive.get(name) {
                return is_primitive;
            }
            if !visiting.insert(name) {
                return false;
            }
            let outer_capped = visiting.start_check();
            let is_primitive = is_definitely_static_primitive(
                value,
                declaration_start,
                static_values,
                root_reference_spans,
                visiting,
            );
            visiting.remove(name);
            if visiting.finish_check(outer_capped) {
                visiting.primitive.insert(name, is_primitive);
            }
            is_primitive
        }
        _ => false,
    }
}

fn is_static_stylex_primitive_pair<'a>(
    left: &'a Expression<'a>,
    right: &'a Expression<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut StaticVisit<'a>,
) -> bool {
    is_definitely_static_primitive(left, before, static_values, root_reference_spans, visiting)
        && is_definitely_static_primitive(
            right,
            before,
            static_values,
            root_reference_spans,
            visiting,
        )
}

pub(super) fn is_static_stylex_theme_override_binding<'a>(
    identifier: &IdentifierReference<'a>,
    before: u32,
    static_values: &FxHashMap<&'a str, (u32, &'a Expression<'a>)>,
    imports: &FxHashMap<&'a str, (Lib, &'a str)>,
    root_reference_spans: &FxHashSet<Span>,
    visiting: &mut StaticVisit<'a>,
) -> bool {
    if !root_reference_spans.contains(&identifier.span) {
        return false;
    }
    let name = identifier.name.as_str();
    let Some(&(declaration_start, value)) = static_values.get(name) else {
        return false;
    };
    if declaration_start >= before {
        return false;
    }
    if let Some(&is_static) = visiting.theme_override.get(name) {
        return is_static;
    }
    if !visiting.insert(name) {
        return false;
    }
    let outer_capped = visiting.start_check();
    let is_static = is_static_stylex_theme_override(
        value,
        declaration_start,
        static_values,
        imports,
        root_reference_spans,
        visiting,
    );
    visiting.remove(name);
    if visiting.finish_check(outer_capped) {
        visiting.theme_override.insert(name, is_static);
    }
    is_static
}
