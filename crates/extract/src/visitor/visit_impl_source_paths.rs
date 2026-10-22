//! Dotted source paths of member chains for the taint-source bindings.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;

use rustc_hash::FxHashSet;

use super::visit_chain_names::MAX_RECORDED_CHAIN_LINKS;

/// Flatten an expression to a dotted member path, unwrapping `await` and parens.
/// Returns `None` for anything that is not an identifier-rooted static-member
/// chain (call results, computed members, etc. are not flattened: a conservative
/// miss, never a wrong source link).
///
/// The security-sink walk asks for this path at every link of a chain, so a
/// path longer than `MAX_RECORDED_CHAIN_LINKS` links is `None`. One call then
/// stays bounded, and the walk is linear in the chain length.
pub(super) fn flatten_member_path(expr: &Expression<'_>) -> Option<String> {
    let mut properties = Vec::new();
    let mut expr = expr;
    let root = loop {
        crate::cost::walk_step();
        if properties.len() > MAX_RECORDED_CHAIN_LINKS {
            return None;
        }
        match expr {
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::AwaitExpression(await_expr) => expr = &await_expr.argument,
            Expression::Identifier(ident) => break ident.name.as_str(),
            // `import.meta` is a meta property, not a member chain; flattening it as
            // `import.meta` lets `import.meta.env.X` reads be modeled as a source the
            // same way `process.env.X` is (issue #890, Vite secrets).
            Expression::ImportMeta(_) => break "import.meta",
            Expression::NewTarget(_) => break "new.target",
            Expression::StaticMemberExpression(member) => {
                properties.push(member.property.name.as_str());
                expr = &member.object;
            }
            _ => return None,
        }
    };
    Some(join_member_path(root, &properties))
}

/// `root.p1.p2...` from the properties of a member chain, outermost first.
pub(in crate::visitor) fn join_member_path(
    root: &str,
    properties_outermost_first: &[impl AsRef<str>],
) -> String {
    let len = root.len()
        + properties_outermost_first
            .iter()
            .map(|property| property.as_ref().len() + 1)
            .sum::<usize>();
    let mut path = String::with_capacity(len);
    path.push_str(root);
    for property in properties_outermost_first.iter().rev() {
        path.push('.');
        path.push_str(property.as_ref());
    }
    path
}

/// The source path for a DIRECT binding (`const id = req.query.id`): the OBJECT
/// path of the member-access init, i.e. the chain with its final property
/// dropped (`req.query`). A bare-identifier init (`const x = req`) has no object
/// to drop and is not a source binding on its own (`None`). A PUBLIC env var
/// (`process.env.NEXT_PUBLIC_X`, `import.meta.env.VITE_Y`) is build-inlined and
/// is NOT a secret source, so it is dropped here (issue #890).
fn tainted_source_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => tainted_source_path(&paren.expression),
        Expression::AwaitExpression(await_expr) => tainted_source_path(&await_expr.argument),
        Expression::StaticMemberExpression(member) => {
            if let Some(full) = flatten_member_path(expr)
                && fallow_types::extract::is_public_env_path(&full)
            {
                return None;
            }
            flatten_member_path(&member.object)
        }
        _ => None,
    }
}

pub(super) fn push_unique_string(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|existing| {
        crate::cost::walk_step();
        existing == &value
    }) {
        out.push(value);
    }
}

#[cfg(test)]
thread_local! {
    /// Candidate scans on this thread, so a test can pin a linear walk.
    static CANDIDATE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn source_path_candidates(expr: &Expression<'_>) -> Vec<String> {
    #[cfg(test)]
    CANDIDATE_SCANS.with(|scans| scans.set(scans.get() + 1));
    let mut out = Vec::new();
    if let Some(path) = flatten_member_path(expr) {
        push_unique_string(&mut out, path);
    }
    if let Some(path) = tainted_source_path(expr) {
        push_unique_string(&mut out, path);
    }
    out
}

pub(super) fn binding_source_path_candidates(expr: &Expression<'_>) -> Vec<String> {
    let mut out = UniquePaths::default();
    collect_binding_source_path_candidates(expr, &mut out);
    out.paths
}

/// Distinct paths in first-seen order. A `+` chain of N distinct operands gives
/// N candidates, and a scan of the list at each push is quadratic.
#[derive(Default)]
struct UniquePaths {
    paths: Vec<String>,
    seen: FxHashSet<String>,
}

impl UniquePaths {
    fn push(&mut self, path: String) {
        if self.seen.insert(path.clone()) {
            self.paths.push(path);
        }
    }
}

fn collect_binding_source_path_candidates(expr: &Expression<'_>, out: &mut UniquePaths) {
    // Unwrap the wrappers first. Their own candidates are those of the inner
    // expression (parens, `await`) or none (TS wrappers), and a candidate scan at
    // each layer of `await await ... x` is quadratic.
    let mut expr = expr;
    loop {
        crate::cost::walk_step();
        expr = match expr {
            Expression::ParenthesizedExpression(paren) => &paren.expression,
            Expression::TSAsExpression(ts_as) => &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => &ts_non_null.expression,
            Expression::AwaitExpression(await_expr) => &await_expr.argument,
            _ => break,
        };
    }
    for path in source_path_candidates(expr) {
        out.push(path);
    }

    match expr {
        Expression::TemplateLiteral(template) => {
            for expression in &template.expressions {
                collect_binding_source_path_candidates(expression, out);
            }
        }
        Expression::BinaryExpression(binary)
            if binary.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            collect_binding_source_path_candidates(&binary.left, out);
            collect_binding_source_path_candidates(&binary.right, out);
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                collect_binding_source_path_candidates(&property.value, out);
            }
        }
        _ => {}
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    /// Run `check` on the expression of the module statement `source`.
    fn with_expression<R>(source: &str, check: impl FnOnce(&Expression<'_>) -> R) -> R {
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::mjs()).parse();
        assert!(parsed.diagnostics.is_empty(), "the fixture must parse");
        let Some(Statement::ExpressionStatement(statement)) = parsed.program.body.first() else {
            panic!("expected an expression statement");
        };
        check(&statement.expression)
    }

    #[test]
    fn flatten_member_path_builds_a_chain_at_the_cap() {
        let links = ".b".repeat(MAX_RECORDED_CHAIN_LINKS);
        let path = with_expression(&format!("a{links};"), flatten_member_path);
        assert_eq!(path, Some(format!("a{links}")));
    }

    #[test]
    fn flatten_member_path_refuses_a_chain_past_the_cap() {
        let links = ".b".repeat(MAX_RECORDED_CHAIN_LINKS + 1);
        assert_eq!(
            with_expression(&format!("a{links};"), flatten_member_path),
            None
        );
    }

    #[test]
    fn concat_operands_give_distinct_candidates_in_first_seen_order() {
        let candidates = with_expression(
            "req.query + `${req.body}` + req.query + req.body;",
            binding_source_path_candidates,
        );
        assert_eq!(candidates, ["req.query", "req", "req.body"]);
    }

    #[test]
    fn awaits_around_a_source_give_its_candidates_in_one_scan() {
        let plain = with_expression("req.query;", binding_source_path_candidates);
        CANDIDATE_SCANS.with(|scans| scans.set(0));
        let awaited = with_expression(
            &format!("{}(req.query);", "await ".repeat(200)),
            binding_source_path_candidates,
        );
        assert_eq!(awaited, plain);
        assert_eq!(plain, ["req.query", "req"]);
        // A scan at each `await` layer is quadratic on a long chain.
        assert_eq!(CANDIDATE_SCANS.with(std::cell::Cell::get), 1);
    }
}
