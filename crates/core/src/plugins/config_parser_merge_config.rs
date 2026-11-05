//! Read each config of a Vite `mergeConfig(...)` export.
//!
//! Vite and Vitest merge the arguments of `mergeConfig(defaults, overrides)`:
//! arrays concatenate in argument order, and a later scalar value replaces an
//! earlier one. `find_config_object` returns one object, so for the export
//! `mergeConfig({ ...base, test: {} }, defineConfig({ test }))` it reads only
//! the inline base object, and the fields of the override are lost. The
//! readers here give each object argument instead.

use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::multi_config::MAX_MULTI_CONFIGS;
use super::{
    MAX_CONFIG_WRAPPER_DEPTH, extract_object_from_arrow_function, extract_object_from_function,
    find_config_object, find_variable_init, get_nested_expression, is_module_exports_target,
    top_level_variable_declarations, unwrap_to_identifier_name,
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
    let mut objects = find_exported_merge_call(program).map_or_else(Vec::new, |call| {
        let mut collector = MergeCollector::new(program);
        // A cap of 3 nested calls covers the real shapes, such as
        // `defineConfig(mergeConfig(base, defineConfig({..})))`.
        collector.arguments(call, MAX_CONFIG_WRAPPER_DEPTH, MAX_VARIABLE_HOPS);
        collector.objects
    });
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

/// The maximum number of local variables that one path through a
/// `mergeConfig` argument follows, as in `const a = b; const b = { .. }`.
/// The limit keeps the recursion shallow for a long chain of aliases.
const MAX_VARIABLE_HOPS: u8 = 8;

/// Collects the objects of a `mergeConfig` call in argument order.
///
/// An argument can repeat a variable that holds another merge, so the output
/// can grow much faster than the source. The collector walks each variable
/// once and keeps its objects. A repeated variable adds the kept objects
/// again at its own position, as Vite merges it there. The collector stops
/// at `MAX_MULTI_CONFIGS` objects.
struct MergeCollector<'a> {
    program: &'a Program<'a>,
    objects: Vec<&'a ObjectExpression<'a>>,
    /// The init expression of each top-level variable, built on first use.
    inits: Option<FxHashMap<&'a str, &'a Expression<'a>>>,
    /// The objects of each variable walk, keyed by the name and the budget
    /// that was left. A walk with less budget can give fewer objects, so a
    /// walk with more budget does not reuse its result.
    expanded: FxHashMap<(&'a str, u8, u8), Vec<&'a ObjectExpression<'a>>>,
    /// The variables that the collector walks now, to stop a cycle.
    in_progress: FxHashSet<&'a str>,
}

impl<'a> MergeCollector<'a> {
    fn new(program: &'a Program<'a>) -> Self {
        Self {
            program,
            objects: Vec::new(),
            inits: None,
            expanded: FxHashMap::default(),
            in_progress: FxHashSet::default(),
        }
    }

    fn is_full(&self) -> bool {
        self.objects.len() >= MAX_MULTI_CONFIGS
    }

    fn push(&mut self, object: Option<&'a ObjectExpression<'a>>) {
        if !self.is_full() {
            self.objects.extend(object);
        }
    }

    /// Push the objects of each argument of `call` in source order.
    fn arguments(&mut self, call: &'a CallExpression<'a>, depth: u8, hops: u8) {
        for arg in call.arguments.iter().filter_map(Argument::as_expression) {
            if self.is_full() {
                tracing::debug!(
                    read = MAX_MULTI_CONFIGS,
                    "mergeConfig gives more configs than the limit, later configs are not read"
                );
                return;
            }
            self.argument(arg, depth, hops);
        }
    }

    /// Push the objects that one `mergeConfig` argument stands for. A nested
    /// `mergeConfig(...)` gives each of its own arguments, also inside
    /// `defineConfig(...)` or a local variable. The options object of any
    /// other call, such as a plugin call, is not a config.
    ///
    /// Each nested call costs one level of `depth`. Each variable costs one
    /// of `hops`.
    fn argument(&mut self, expr: &'a Expression<'a>, depth: u8, hops: u8) {
        match expr {
            Expression::ObjectExpression(obj) => self.push(Some(obj)),
            Expression::ParenthesizedExpression(paren) => {
                self.argument(&paren.expression, depth, hops);
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.argument(&ts_sat.expression, depth, hops);
            }
            Expression::TSAsExpression(ts_as) => self.argument(&ts_as.expression, depth, hops),
            Expression::ArrowFunctionExpression(arrow) => {
                self.push(extract_object_from_arrow_function(arrow));
            }
            Expression::FunctionExpression(func) => self.push(extract_object_from_function(func)),
            Expression::CallExpression(call)
                if depth > 0 && is_merge_config_callee(&call.callee) =>
            {
                self.arguments(call, depth - 1, hops);
            }
            Expression::CallExpression(call)
                if depth > 0 && is_define_config_callee(&call.callee) =>
            {
                if let Some(config) = call.arguments.first().and_then(Argument::as_expression) {
                    self.argument(config, depth - 1, hops);
                }
            }
            Expression::Identifier(id) => self.variable(id.name.as_str(), depth, hops),
            _ => {}
        }
    }

    /// Push the objects of a local variable. The first walk keeps them, so a
    /// repeated variable costs no new walk.
    fn variable(&mut self, name: &'a str, depth: u8, hops: u8) {
        let key = (name, depth, hops);
        if let Some(kept) = self.expanded.get(&key) {
            let room = MAX_MULTI_CONFIGS.saturating_sub(self.objects.len());
            let kept: Vec<_> = kept.iter().copied().take(room).collect();
            self.objects.extend(kept);
            return;
        }
        if hops == 0 || !self.in_progress.insert(name) {
            return;
        }
        if let Some(init) = self.init(name) {
            let start = self.objects.len();
            self.argument(init, depth, hops - 1);
            let walked = self.objects[start..].to_vec();
            self.expanded.insert(key, walked);
        }
        self.in_progress.remove(name);
    }

    fn init(&mut self, name: &str) -> Option<&'a Expression<'a>> {
        let program = self.program;
        self.inits
            .get_or_insert_with(|| {
                let mut inits = FxHashMap::default();
                for declarator in
                    top_level_variable_declarations(program).flat_map(|decl| &decl.declarations)
                {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id
                        && let Some(init) = &declarator.init
                    {
                        inits.entry(id.name.as_str()).or_insert(init);
                    }
                }
                inits
            })
            .get(name)
            .copied()
    }
}

/// `mergeConfig(...)` or a namespace call such as `vite.mergeConfig(...)`.
fn is_merge_config_callee(callee: &Expression<'_>) -> bool {
    callee_name(callee) == Some(MERGE_CONFIG)
}

/// `defineConfig(...)` or `defineProject(...)`, also as a namespace call.
pub(super) fn is_define_config_callee(callee: &Expression<'_>) -> bool {
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

    use super::super::{
        extract_config_string, extract_config_string_or_array, extract_from_source,
    };
    use super::find_merged_config_objects;

    fn merged_object_count(source: &str) -> usize {
        extract_from_source(source, &ts_path(), |program| {
            Some(find_merged_config_objects(program).len())
        })
        .unwrap_or_default()
    }

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

    /// A variable that holds a merge and repeats in the export is walked
    /// once, and the output stops at the config limit.
    #[test]
    fn merge_config_repeated_merge_variable_is_bounded() {
        let repeats = vec!["shared"; 300].join(", ");
        let source = format!(
            "const shared = mergeConfig(\n\
               {{ test: {{ setupFiles: [\"./a.ts\"] }} }},\n\
               {{ test: {{ setupFiles: [\"./b.ts\"] }} }},\n\
               {{ test: {{ setupFiles: [\"./c.ts\"] }} }},\n\
             );\n\
             export default mergeConfig({repeats});\n"
        );
        let start = std::time::Instant::now();
        assert_eq!(merged_object_count(&source), 64);
        let setup_files =
            extract_config_string_or_array(&source, &ts_path(), &["test", "setupFiles"]);
        assert_eq!(setup_files.len(), 64);
        assert_eq!(setup_files[..3], ["./a.ts", "./b.ts", "./c.ts"]);
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "fan-out took {elapsed:?}"
        );
    }

    #[test]
    fn merge_config_object_count_is_capped() {
        let objects = vec!["{ test: {} }"; 100].join(", ");
        let source = format!("export default mergeConfig({objects});");
        assert_eq!(merged_object_count(&source), 64);
    }

    /// A variable that a deep path reaches first must still expand fully
    /// when a shallow argument repeats it.
    #[test]
    fn merge_config_variable_reached_deep_first_still_expands() {
        let source = r#"
            const shared = { test: { setupFiles: ["./s.ts"] } };
            const x = mergeConfig(shared, { test: { setupFiles: ["./x.ts"] } });
            export default mergeConfig(x, shared, { test: { setupFiles: ["./o.ts"] } });
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./s.ts", "./x.ts", "./s.ts", "./o.ts"]
        );
    }

    #[test]
    fn merge_config_variable_with_define_config_merge_keeps_every_field() {
        let source = r#"
            const shared = mergeConfig(
              { test: { setupFiles: ["./a.ts"] } },
              defineConfig({ test: { setupFiles: ["./b.ts"] } }),
            );
            export default mergeConfig(shared, { test: { setupFiles: ["./c.ts"] } });
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./a.ts", "./b.ts", "./c.ts"]
        );
    }

    #[test]
    fn merge_config_variable_chain_through_define_config_keeps_every_field() {
        let source = r#"
            const inner = { test: { setupFiles: ["./a.ts"] } };
            const mid = defineConfig(inner);
            export default mergeConfig(mid, { test: { setupFiles: ["./c.ts"] } });
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "setupFiles"]),
            vec!["./a.ts", "./c.ts"]
        );
    }

    /// A repeated variable keeps its place, so the last argument still sets
    /// a single value.
    #[test]
    fn merge_config_repeated_variable_keeps_its_position() {
        let source = r#"
            const shared = { test: { environment: "shared" } };
            export default mergeConfig(shared, { test: { environment: "override" } }, shared);
        "#;
        assert_eq!(
            extract_config_string(source, &ts_path(), &["test", "environment"]).as_deref(),
            Some("shared")
        );
    }

    /// A variable cycle ends, and a long variable chain stays bounded.
    #[test]
    fn merge_config_variable_cycle_and_long_chain_end() {
        let cycle = r#"
            const a = mergeConfig(b, { test: { include: ["a.ts"] } });
            const b = mergeConfig(a, { test: { include: ["b.ts"] } });
            export default mergeConfig(a);
        "#;
        assert_eq!(
            extract_config_string_or_array(cycle, &ts_path(), &["test", "include"]),
            vec!["b.ts", "a.ts"]
        );
        let chain = (1..20_000)
            .map(|i| format!("const v{i} = v{};", i - 1))
            .collect::<Vec<_>>()
            .join("\n");
        let source =
            format!("const v0 = {{ test: {{}} }};\n{chain}\nexport default mergeConfig(v19999);\n");
        assert_eq!(merged_object_count(&source), 0);
    }

    /// A walk that the call depth cut short must not hide the full walk of
    /// the same variable at a shallower position.
    #[test]
    fn merge_config_depth_cut_walk_does_not_hide_full_walk() {
        let source = r#"
            const shared = mergeConfig({ test: { include: ["a.ts"] } }, { test: { include: ["b.ts"] } });
            export default mergeConfig(
              defineConfig(mergeConfig(defineConfig(shared))),
              shared,
              { test: { include: ["c.ts"] } },
            );
        "#;
        assert_eq!(
            extract_config_string_or_array(source, &ts_path(), &["test", "include"]),
            vec!["a.ts", "b.ts", "c.ts"]
        );
    }
}
