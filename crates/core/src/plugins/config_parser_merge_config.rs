//! Read each config of a Vite `mergeConfig(...)` export.
//!
//! Vite and Vitest merge the arguments of `mergeConfig(defaults, overrides)`:
//! arrays concatenate in argument order, and a later scalar value replaces an
//! earlier one. `find_config_object` returns one object, so for the export
//! `mergeConfig({ ...base, test: {} }, defineConfig({ test }))` it reads only
//! the inline base object, and the fields of the override are lost. The
//! readers here give each object argument instead.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;

use super::{
    MAX_CONFIG_WRAPPER_DEPTH, extract_object_from_arrow_function, extract_object_from_function,
    find_config_object, find_variable_init, get_nested_expression, is_module_exports_target,
    unwrap_to_identifier_name,
};

const MERGE_CONFIG: &str = "mergeConfig";

/// The helpers that return their first argument as the config.
const DEFINE_CONFIG_HELPERS: &[&str] = &["defineConfig", "defineProject"];

/// Run `read` on each exported config object and concatenate the results in
/// source order, as Vite `mergeConfig` concatenates arrays. `None` when no
/// config object gives a result.
pub fn read_each_config<'a, T>(
    program: &'a Program<'a>,
    mut read: impl FnMut(&'a ObjectExpression<'a>) -> Option<Vec<T>>,
) -> Option<Vec<T>> {
    let mut merged: Option<Vec<T>> = None;
    for object in find_merged_config_objects(program) {
        if let Some(values) = read(object) {
            merged.get_or_insert_with(Vec::new).extend(values);
        }
    }
    merged
}

/// Read a single value at `key_path` with `read(object, key_path)`. Vite
/// takes the value of the last `mergeConfig` argument that sets the key, so
/// only that object is read. When its value is not static, `read` gives
/// `None`, and an earlier static value does not apply.
pub fn read_last_config<'a, T>(
    program: &'a Program<'a>,
    key_path: &[&str],
    read: impl FnOnce(&'a ObjectExpression<'a>, &[&str]) -> Option<T>,
) -> Option<T> {
    find_merged_config_objects(program)
        .into_iter()
        .rev()
        .find(|object| get_nested_expression(object, key_path).is_some())
        .and_then(|object| read(object, key_path))
}

/// Run `read` on each exported config object and keep the last result. Use
/// this for a match inside a list, such as one plugin of `plugins`, because
/// `mergeConfig` concatenates the lists of all arguments.
pub fn read_last_match<'a, T>(
    program: &'a Program<'a>,
    read: impl FnMut(&'a ObjectExpression<'a>) -> Option<T>,
) -> Option<T> {
    find_merged_config_objects(program)
        .into_iter()
        .rev()
        .find_map(read)
}

/// The config objects that the program exports, in source order. For a
/// `mergeConfig(...)` export, these are the arguments that resolve to an
/// object, also through `defineConfig(...)`, `defineProject(...)` or a local
/// variable. For any other export, this is the one object that
/// `find_config_object` finds.
pub fn find_merged_config_objects<'a>(program: &'a Program<'a>) -> Vec<&'a ObjectExpression<'a>> {
    let mut objects = Vec::new();
    // A cap of 3 levels covers the real shapes, such as
    // `defineConfig(mergeConfig(base, defineConfig({..})))`, and it stops a
    // variable that refers to itself.
    if let Some(call) = find_exported_merge_call(program) {
        collect_merge_arguments(program, call, MAX_CONFIG_WRAPPER_DEPTH, &mut objects);
    }
    if objects.is_empty() {
        objects.extend(find_config_object(program));
    }
    objects
}

fn find_exported_merge_call<'a>(program: &'a Program<'a>) -> Option<&'a CallExpression<'a>> {
    program.body.iter().find_map(|stmt| match stmt {
        Statement::ExportDefaultDeclaration(decl) => decl
            .declaration
            .as_expression()
            .and_then(|expr| find_merge_call(program, expr, MAX_CONFIG_WRAPPER_DEPTH)),
        Statement::ExpressionStatement(expr_stmt) => match &expr_stmt.expression {
            Expression::AssignmentExpression(assign) if is_module_exports_target(&assign.left) => {
                find_merge_call(program, &assign.right, MAX_CONFIG_WRAPPER_DEPTH)
            }
            _ => None,
        },
        _ => None,
    })
}

/// Find the `mergeConfig(...)` call that `expr` exports, directly, through a
/// wrapper call such as `defineConfig(mergeConfig(...))`, or through a local
/// variable.
fn find_merge_call<'a>(
    program: &'a Program<'a>,
    expr: &'a Expression<'a>,
    depth: u8,
) -> Option<&'a CallExpression<'a>> {
    if depth == 0 {
        return None;
    }
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            find_merge_call(program, &paren.expression, depth)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            find_merge_call(program, &ts_sat.expression, depth)
        }
        Expression::TSAsExpression(ts_as) => find_merge_call(program, &ts_as.expression, depth),
        Expression::CallExpression(call) if is_merge_config_callee(&call.callee) => Some(call),
        Expression::CallExpression(call) => call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .find_map(|arg| find_merge_call(program, arg, depth - 1)),
        _ => unwrap_to_identifier_name(expr)
            .and_then(|name| find_variable_init(program, name))
            .and_then(|init| find_merge_call(program, init, depth - 1)),
    }
}

/// Push the object of each argument of `call` in source order.
fn collect_merge_arguments<'a>(
    program: &'a Program<'a>,
    call: &'a CallExpression<'a>,
    depth: u8,
    objects: &mut Vec<&'a ObjectExpression<'a>>,
) {
    for arg in call.arguments.iter().filter_map(Argument::as_expression) {
        collect_merge_argument(program, arg, depth, objects);
    }
}

/// Push the objects that one `mergeConfig` argument stands for. A nested
/// `mergeConfig(...)` gives each of its own arguments, also inside
/// `defineConfig(...)` or a local variable. The options object of any other
/// call, such as a plugin call, is not a config.
///
/// Each wrapper, nested merge or variable costs one level of `depth`.
fn collect_merge_argument<'a>(
    program: &'a Program<'a>,
    expr: &'a Expression<'a>,
    depth: u8,
    objects: &mut Vec<&'a ObjectExpression<'a>>,
) {
    if depth == 0 {
        return;
    }
    match expr {
        Expression::ObjectExpression(obj) => objects.push(obj),
        Expression::ParenthesizedExpression(paren) => {
            collect_merge_argument(program, &paren.expression, depth, objects);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_merge_argument(program, &ts_sat.expression, depth, objects);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_merge_argument(program, &ts_as.expression, depth, objects);
        }
        Expression::ArrowFunctionExpression(arrow) => {
            objects.extend(extract_object_from_arrow_function(arrow));
        }
        Expression::FunctionExpression(func) => objects.extend(extract_object_from_function(func)),
        Expression::CallExpression(call) if is_merge_config_callee(&call.callee) => {
            collect_merge_arguments(program, call, depth - 1, objects);
        }
        Expression::CallExpression(call) if is_define_config_callee(&call.callee) => {
            if let Some(config) = call.arguments.first().and_then(Argument::as_expression) {
                collect_merge_argument(program, config, depth - 1, objects);
            }
        }
        _ => {
            if let Some(init) =
                unwrap_to_identifier_name(expr).and_then(|name| find_variable_init(program, name))
            {
                collect_merge_argument(program, init, depth - 1, objects);
            }
        }
    }
}

/// `mergeConfig(...)` or a namespace call such as `vite.mergeConfig(...)`.
fn is_merge_config_callee(callee: &Expression<'_>) -> bool {
    callee_name(callee) == Some(MERGE_CONFIG)
}

/// `defineConfig(...)` or `defineProject(...)`, also as a namespace call.
fn is_define_config_callee(callee: &Expression<'_>) -> bool {
    callee_name(callee).is_some_and(|name| DEFINE_CONFIG_HELPERS.contains(&name))
}

fn callee_name<'a>(callee: &'a Expression<'_>) -> Option<&'a str> {
    match callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{extract_config_string, extract_config_string_or_array};

    fn ts_path() -> PathBuf {
        PathBuf::from("config.ts")
    }

    /// Vitest's documented way to share a Vite config with the test runner.
    #[test]
    fn merge_config_extracts_nested_define_config_object() {
        let source = r#"
            import { defineConfig, mergeConfig } from 'vitest/config';
            import viteConfig from './vite.config';
            export default mergeConfig(viteConfig, defineConfig({
                test: { environment: "jsdom" }
            }));
        "#;
        assert_eq!(
            extract_config_string(source, &ts_path(), &["test", "environment"]).as_deref(),
            Some("jsdom")
        );
    }

    #[test]
    fn define_config_wrapping_merge_config_extracts_object() {
        let source = r#"
            import { defineConfig, mergeConfig } from 'vitest/config';
            import base from './base';
            export default defineConfig(mergeConfig(base, { test: { environment: "jsdom" } }));
        "#;
        assert_eq!(
            extract_config_string(source, &ts_path(), &["test", "environment"]).as_deref(),
            Some("jsdom")
        );
    }

    /// An inline base object at argument zero must not hide the fields of
    /// the override config after it.
    #[test]
    fn merge_config_reads_every_object_argument() {
        let source = r#"
            import { defineConfig, mergeConfig } from 'vite';
            import baseConfig from '../../vite.config.ts';
            export default mergeConfig(
              { ...baseConfig, test: { setupFiles: ["./base.setup.ts"], environment: "node" } },
              defineConfig({
                test: {
                  setupFiles: ["./src/a.setup.ts", "./src/b.setup.ts"],
                  environment: "jsdom",
                },
              }),
            );
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./base.setup.ts", "./src/a.setup.ts", "./src/b.setup.ts"]
        );
        assert_eq!(
            extract_config_string(source, &ts_path(), &["test", "environment"]).as_deref(),
            Some("jsdom"),
            "the last argument overrides a single value, as in Vite"
        );
    }

    #[test]
    fn merge_config_reads_define_project_and_local_arguments() {
        let source = r#"
            import { defineProject, mergeConfig } from 'vitest/config';
            const shared = { test: { setupFiles: "./shared.setup.ts" } };
            const config = mergeConfig(
              shared,
              mergeConfig({ test: {} }, defineProject({ test: { setupFiles: ["./own.setup.ts"] } })),
            );
            export default config;
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./shared.setup.ts", "./own.setup.ts"]
        );
    }

    #[test]
    fn merge_config_under_module_exports_reads_every_argument() {
        let source = r#"
            const vite = require('vite');
            module.exports = vite.mergeConfig({ test: {} }, { test: { include: ["a.test.ts"] } });
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "include"]),
            vec!["a.test.ts"]
        );
    }

    /// The options object of an unrelated call is not a config.
    #[test]
    fn merge_config_skips_objects_inside_other_calls() {
        let source = r#"
            import { mergeConfig } from 'vite';
            export default mergeConfig(
              { test: { setupFiles: ["./a.ts"] } },
              somePlugin({ test: { setupFiles: ["./bogus.ts"] } }),
            );
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./a.ts"]
        );
    }

    #[test]
    fn merge_config_reads_every_argument_of_a_local_merge() {
        let source = r#"
            import { mergeConfig } from 'vite';
            const shared = mergeConfig({ test: { setupFiles: ["./a.ts"] } }, { test: { setupFiles: ["./b.ts"] } });
            export default mergeConfig(shared, { test: { setupFiles: ["./c.ts"] } });
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./a.ts", "./b.ts", "./c.ts"]
        );
    }

    #[test]
    fn merge_config_reads_every_argument_of_a_merge_inside_define_config() {
        let source = r#"
            import { defineConfig, mergeConfig } from 'vite';
            export default mergeConfig(
              { test: { setupFiles: ["./a.ts"] } },
              defineConfig(mergeConfig({ test: { setupFiles: ["./b.ts"] } }, { test: { setupFiles: ["./c.ts"] } })),
            );
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./a.ts", "./b.ts", "./c.ts"]
        );
    }

    /// Vite uses the later value, so a later value that is not static hides
    /// the earlier static value.
    #[test]
    fn merge_config_later_dynamic_value_wins_over_earlier_literal() {
        let source = r#"
            import { mergeConfig } from 'vite';
            export default mergeConfig(
              { test: { environment: "node" } },
              { test: { environment: process.env.E } },
            );
        "#;
        assert_eq!(
            extract_config_string(source, &ts_path(), &["test", "environment"]),
            None
        );
    }
}
