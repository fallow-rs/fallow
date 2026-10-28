//! Read each config of a config file that exports more than one.
//!
//! Webpack and rspack build every config of an exported array, as in
//! `module.exports = [configA, configB]` or a function that returns such an
//! array. `find_config_object` keeps its contract and returns `None` for an
//! array export, because other tools read an array export in their own way.
//! The readers here are for the tools that build each element.

use std::path::{Path, PathBuf};

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::{
    PathScope, collect_require_sources, collect_shallow_string_values, expression_to_path,
    extract_from_source, find_config_object, find_property, get_nested_expression,
    get_nested_string_or_array, is_module_exports_target, object_expression,
    path_from_config_string, resolve_alias_pairs_kinded,
};

/// The maximum number of configs read from one exported array, in source order.
const MAX_MULTI_CONFIGS: usize = 16;

/// The config objects of a program: the one object that `find_config_object`
/// finds, or else each object element of an exported config array. Elements
/// that are not object literals, such as `merge(a, b)` or a spread, are skipped.
pub fn find_config_objects<'a>(program: &'a Program<'a>) -> Vec<&'a ObjectExpression<'a>> {
    if let Some(object) = find_config_object(program) {
        return vec![object];
    }
    find_default_export_array(program)
        .map(|array| {
            array
                .elements
                .iter()
                .filter_map(|element| element.as_expression().and_then(object_expression))
                .take(MAX_MULTI_CONFIGS)
                .collect()
        })
        .unwrap_or_default()
}

/// Run `read` on each config object of the source and collect the results in
/// source order.
fn extract_each_config<T: Send>(
    source: &str,
    path: &Path,
    read: impl Fn(&Program<'_>, &ObjectExpression<'_>) -> Vec<T> + Send + Sync,
) -> Vec<T> {
    extract_from_source(source, path, |program| {
        Some(
            find_config_objects(program)
                .into_iter()
                .flat_map(|object| read(program, object))
                .collect(),
        )
    })
    .unwrap_or_default()
}

/// The entries of each config, with the base directory path that the same
/// config declares under `base_key`.
pub fn extract_each_config_entries(
    source: &str,
    path: &Path,
    entry_key: &[&str],
    base_key: &str,
) -> Vec<(Vec<String>, Option<PathBuf>)> {
    extract_each_config(source, path, |program, object| {
        let entries = get_nested_string_or_array(object, entry_key, PathScope::Program(program))
            .unwrap_or_default();
        let base = get_nested_expression(object, &[base_key]).and_then(expression_to_path);
        vec![(entries, base)]
    })
}

/// The path aliases of all configs, without duplicate pairs.
pub fn extract_each_config_path_aliases(
    source: &str,
    path: &Path,
    prop_path: &[&str],
) -> Vec<(String, PathBuf)> {
    let pairs = extract_each_config(source, path, |program, object| {
        let Some(expr) = get_nested_expression(object, prop_path) else {
            return Vec::new();
        };
        let mut visited = FxHashSet::default();
        resolve_alias_pairs_kinded(program, path, expr, &mut visited, 0)
    });
    let mut seen = FxHashSet::default();
    pairs
        .into_iter()
        .filter(|(find, replacement, _)| seen.insert((find.clone(), replacement.clone())))
        .map(|(find, replacement, _)| (find, path_from_config_string(&replacement)))
        .collect()
}

/// The `require('...')` sources in the `key` property of all configs.
pub fn extract_each_config_require_strings(source: &str, path: &Path, key: &str) -> Vec<String> {
    dedup(extract_each_config(source, path, |_, object| {
        find_property(object, key)
            .map(|prop| collect_require_sources(&prop.value))
            .unwrap_or_default()
    }))
}

/// The top-level string values of the `key` property of all configs.
pub fn extract_each_config_shallow_strings(source: &str, path: &Path, key: &str) -> Vec<String> {
    dedup(extract_each_config(source, path, |_, object| {
        find_property(object, key)
            .map(|prop| collect_shallow_string_values(&prop.value))
            .unwrap_or_default()
    }))
}

fn dedup(values: Vec<String>) -> Vec<String> {
    let mut seen = FxHashSet::default();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Find an exported array config, the `defineWorkspace([...])` /
/// `vitest.workspace.{ts,js}` shape and the webpack multi-config shape. Handles
/// `export default [...]`, `module.exports = [...]` and
/// `export default defineWorkspace([...])` / `defineConfig([...])` (the array as
/// the call's first argument), function configs with a static array return, and
/// parenthesised / `as` wrappers.
pub(super) fn find_default_export_array<'a>(
    program: &'a Program<'a>,
) -> Option<&'a ArrayExpression<'a>> {
    for stmt in &program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(decl) => {
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &decl.declaration
                {
                    if function.generator {
                        return None;
                    }
                    return function.body.as_ref().and_then(|body| returned_array(body));
                }
                return decl
                    .declaration
                    .as_expression()
                    .and_then(array_from_config_expression);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression
                    && is_module_exports_target(&assign.left)
                {
                    return array_from_config_expression(&assign.right);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read only returns belonging to this callback, never nested helper functions.
fn returned_array<'a>(body: &'a FunctionBody<'a>) -> Option<&'a ArrayExpression<'a>> {
    body.statements
        .iter()
        .find_map(|statement| match statement {
            Statement::ReturnStatement(returned) => Some(returned),
            _ => None,
        })
        .and_then(|returned| returned.argument.as_ref())
        .and_then(array_from_expression)
}

/// A config provider invokes its callback once; returned callbacks are values.
fn array_from_config_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ArrayExpression<'a>> {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => returned_array(body),
            body => body.as_expression().and_then(array_from_expression),
        },
        Expression::FunctionExpression(function) => {
            if function.generator {
                return None;
            }
            function.body.as_ref().and_then(|body| returned_array(body))
        }
        Expression::ParenthesizedExpression(paren) => {
            array_from_config_expression(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => array_from_config_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => {
            array_from_config_expression(&ts_sat.expression)
        }
        Expression::CallExpression(call) => call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(array_from_config_expression),
        _ => array_from_expression(expr),
    }
}

fn array_from_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ArrayExpression<'a>> {
    match expr {
        Expression::ArrayExpression(arr) => Some(arr),
        Expression::ParenthesizedExpression(paren) => array_from_expression(&paren.expression),
        Expression::TSAsExpression(ts_as) => array_from_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => array_from_expression(&ts_sat.expression),
        Expression::CallExpression(call) => call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(array_from_expression),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::super::extract_from_source;
    use super::{find_config_object, find_config_objects};

    fn count(source: &str) -> usize {
        extract_from_source(source, Path::new("webpack.config.js"), |program| {
            Some(find_config_objects(program).len())
        })
        .unwrap_or_default()
    }

    #[test]
    fn each_object_of_an_exported_array_is_a_config() {
        assert_eq!(
            count("module.exports = [{ entry: 'a' }, { entry: 'b' }];"),
            2
        );
        assert_eq!(
            count("module.exports = () => [{ entry: 'a' }, merge(a, b), { entry: 'b' }];"),
            2
        );
        assert_eq!(
            count(
                "module.exports = (env) => { const a = { entry: 'a' }; const b = { ...a }; return [a, b]; };"
            ),
            2
        );
        assert_eq!(
            count("export default function () { return [{ a: 1 }]; }"),
            1
        );
        assert_eq!(count("module.exports = { entry: 'a' };"), 1);
        assert_eq!(count("module.exports = ['a', 'b'];"), 0);
        assert_eq!(count("module.exports = factory();"), 0);
    }

    #[test]
    fn exported_array_count_is_bounded() {
        let elements = vec!["{ entry: 'a' }"; 40].join(", ");
        assert_eq!(count(&format!("module.exports = [{elements}];")), 16);
    }

    #[test]
    fn single_config_reader_keeps_none_for_an_array_export() {
        let allocator = Allocator::default();
        let source = "module.exports = [{ entry: 'a' }, { entry: 'b' }];";
        let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
        assert!(find_config_object(&parsed.program).is_none());
    }
}
