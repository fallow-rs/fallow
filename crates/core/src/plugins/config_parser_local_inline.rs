//! Put the value of a local binding of a config callback in the values that
//! the callback returns.
//!
//! A config file can export a function that builds its value in local
//! bindings: `const common = {...}; const web = { ...common, entry }; return
//! [web];`. The config walkers read literal objects only. This pass replaces
//! each returned identifier, each array element and property value that names
//! a local, and each object spread of a local with a copy of the literal value
//! of that local. It never runs the code of the config.
//!
//! The pass ignores the order of the statements. A returned value that names a
//! local declared after the `return` fails at run time, but this pass reads the
//! value of the local.

use oxc_allocator::{Allocator, CloneIn};
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use super::const_inline::{Inliner, constant_values, literal_value, stable_values};
use super::{
    MAX_CONFIG_BRANCH_DEPTH, MAX_CONFIG_WRAPPER_DEPTH, extract_object_from_expression,
    is_module_exports_target,
};

/// The budget of the local pass, as a multiple of the source size. A config
/// that spreads one shared local into two configs copies that local twice.
const LOCAL_BUDGET_FACTOR: usize = 2;

/// Inline the stable locals of each exported config callback into the values
/// that the callback returns.
///
/// A local qualifies when a `const` or `let` statement at the callback body's
/// own level declares it with an object, array or string literal, and it is
/// the only binding of its name in the program, with no writes, no mutating
/// calls and no call that can change it. The stable top-level constants are
/// available too. The copied AST
/// nodes stay at or below `LOCAL_BUDGET_FACTOR` times `source_len`.
pub(super) fn inline_callback_locals<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    source_len: usize,
) {
    let Some(mut values) = callback_locals(allocator, program) else {
        return;
    };
    for (name, value) in constant_values(allocator, program) {
        values.entry(name).or_insert(value);
    }
    if values.is_empty() {
        return;
    }
    let mut inliner = Inliner::new(
        allocator,
        &values,
        source_len.saturating_mul(LOCAL_BUDGET_FACTOR),
    );
    for callback in config_callbacks(program) {
        callback.inline_returns(&mut inliner, &values);
    }
}

/// A copy of the literal value of each stable local of the config callbacks.
/// Returns `None` when the program exports no config callback.
fn callback_locals<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
) -> Option<FxHashMap<String, Expression<'a>>> {
    let callbacks = config_callbacks(program);
    if callbacks.is_empty() {
        return None;
    }
    let mut candidates: Vec<(String, Expression<'a>)> = Vec::new();
    for callback in callbacks {
        let Callback::Body(body) = callback else {
            continue;
        };
        for statement in &body.statements {
            let Statement::VariableDeclaration(decl) = statement else {
                continue;
            };
            if !matches!(
                decl.kind,
                VariableDeclarationKind::Const | VariableDeclarationKind::Let
            ) {
                continue;
            }
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                if let Some(value) = declarator.init.as_ref().and_then(literal_value) {
                    candidates.push((id.name.to_string(), value.clone_in(allocator)));
                }
            }
        }
    }
    if candidates.is_empty() {
        return Some(FxHashMap::default());
    }
    let stable: FxHashSet<String> = stable_values(
        program,
        candidates
            .iter()
            .map(|(name, value)| (name.as_str(), value)),
        false,
    )
    .into_iter()
    .map(str::to_string)
    .collect();
    Some(
        candidates
            .into_iter()
            .filter(|(name, _)| stable.contains(name))
            .collect(),
    )
}

/// The body of a config callback: a function body, or the expression body of
/// an arrow.
enum Callback<'p, 'a> {
    Body(&'p mut FunctionBody<'a>),
    Expression(&'p mut Expression<'a>),
}

impl<'a> Callback<'_, 'a> {
    /// Inline the values that `find_config_object` and the multi-config reader
    /// can read as the returned config: the expression body of an arrow, or the
    /// argument of each `return` at the body's own level or in the bounded
    /// branch search. Returns of nested functions belong to those functions and
    /// stay out.
    ///
    /// `find_config_object` prefers an object that a top-level `return` gives
    /// over a branch return. A top-level `return base;` gave no object before
    /// this pass, so the search used the first branch return that gives an
    /// object. To keep that result, a returned identifier at the top level
    /// stays when a branch return gives an object after inlining. A returned
    /// array stays out of this rule: `find_config_object` never reads it, and
    /// the multi-config reader reads only top-level returns.
    fn inline_returns(
        self,
        inliner: &mut Inliner<'a, '_>,
        values: &FxHashMap<String, Expression<'a>>,
    ) {
        let body = match self {
            Callback::Body(body) => body,
            Callback::Expression(expr) => {
                inliner.inline_value(expr);
                return;
            }
        };
        let mut returns = Vec::new();
        collect_returns(
            &mut body.statements,
            MAX_CONFIG_BRANCH_DEPTH,
            true,
            &mut returns,
        );
        let (top_level, branches): (Vec<_>, Vec<_>) =
            returns.into_iter().partition(|ret| ret.top_level);
        let mut branch_gives_object = false;
        for ret in branches {
            inliner.inline_value(ret.value);
            branch_gives_object |= extract_object_from_expression(ret.value).is_some();
        }
        for ret in top_level {
            if branch_gives_object && names_a_non_array(ret.value, values) {
                continue;
            }
            inliner.inline_value(ret.value);
        }
    }
}

/// Whether `expr` is an identifier whose known value is not an array literal.
fn names_a_non_array(expr: &Expression<'_>, values: &FxHashMap<String, Expression<'_>>) -> bool {
    let Expression::Identifier(id) = expr.get_inner_expression() else {
        return false;
    };
    !values
        .get(id.name.as_str())
        .is_some_and(|value| matches!(value.get_inner_expression(), Expression::ArrayExpression(_)))
}

/// The config callbacks that the program exports, in the shapes that
/// `find_config_object` reads: an exported function or arrow, also as an
/// argument of an exported call such as `defineConfig(fn)`.
fn config_callbacks<'p, 'a>(program: &'p mut Program<'a>) -> Vec<Callback<'p, 'a>> {
    let mut callbacks = Vec::new();
    let mut exported_names = Vec::new();
    let mut declarations = Vec::new();
    for statement in &mut program.body {
        match statement {
            Statement::ExportDefaultDeclaration(decl) => match &mut decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    push_function(func, &mut callbacks);
                }
                kind => {
                    if let Some(expr) = kind.as_expression_mut() {
                        collect_exported(expr, &mut callbacks, &mut exported_names);
                    }
                }
            },
            Statement::ExpressionStatement(stmt) => {
                if let Expression::AssignmentExpression(assign) = &mut stmt.expression
                    && is_module_exports_target(&assign.left)
                {
                    collect_exported(&mut assign.right, &mut callbacks, &mut exported_names);
                }
            }
            Statement::VariableDeclaration(decl) => declarations.push(&mut **decl),
            Statement::ExportDeclaration(export) => {
                if let Declaration::VariableDeclaration(decl) = &mut export.declaration {
                    declarations.push(&mut **decl);
                }
            }
            _ => {}
        }
    }
    // `module.exports = config` reads the callback of one top-level binding,
    // as `find_config_object` does.
    for decl in declarations {
        for declarator in &mut decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            if !exported_names.iter().any(|name| name == id.name.as_str()) {
                continue;
            }
            if let Some(init) = &mut declarator.init {
                collect_callbacks(init, MAX_CONFIG_WRAPPER_DEPTH, &mut callbacks);
            }
        }
    }
    callbacks
}

/// Collect the callbacks of an exported expression, and the name of an
/// exported identifier such as `config` in `module.exports = config`.
fn collect_exported<'p, 'a>(
    expr: &'p mut Expression<'a>,
    callbacks: &mut Vec<Callback<'p, 'a>>,
    exported_names: &mut Vec<String>,
) {
    if let Expression::Identifier(id) = expr.get_inner_expression() {
        exported_names.push(id.name.to_string());
        return;
    }
    collect_callbacks(expr, MAX_CONFIG_WRAPPER_DEPTH, callbacks);
}

fn collect_callbacks<'p, 'a>(
    expr: &'p mut Expression<'a>,
    depth: u8,
    callbacks: &mut Vec<Callback<'p, 'a>>,
) {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            collect_callbacks(&mut paren.expression, depth, callbacks);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_callbacks(&mut ts_as.expression, depth, callbacks);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_callbacks(&mut ts_sat.expression, depth, callbacks);
        }
        Expression::ArrowFunctionExpression(arrow) => match &mut arrow.body {
            ArrowFunctionBody::FunctionBody(body) => callbacks.push(Callback::Body(body)),
            body => {
                if let Some(expr) = body.as_expression_mut() {
                    callbacks.push(Callback::Expression(expr));
                }
            }
        },
        Expression::FunctionExpression(func) => push_function(func, callbacks),
        Expression::CallExpression(call) if depth > 0 => {
            for argument in &mut call.arguments {
                if let Some(expr) = argument.as_expression_mut() {
                    collect_callbacks(expr, depth - 1, callbacks);
                }
            }
        }
        _ => {}
    }
}

fn push_function<'p, 'a>(func: &'p mut Function<'a>, callbacks: &mut Vec<Callback<'p, 'a>>) {
    if func.generator {
        return;
    }
    if let Some(body) = &mut func.body {
        callbacks.push(Callback::Body(body));
    }
}

/// The argument of one `return`, and whether the `return` is at the body's own
/// level.
struct Returned<'p, 'a> {
    value: &'p mut Expression<'a>,
    top_level: bool,
}

/// The argument of each `return` in `statements`, through the same control
/// flow and depth as the returned-object search of `find_config_object`.
fn collect_returns<'p, 'a>(
    statements: &'p mut [Statement<'a>],
    depth: u8,
    top_level: bool,
    values: &mut Vec<Returned<'p, 'a>>,
) {
    if depth == 0 {
        return;
    }
    for statement in statements {
        match statement {
            Statement::ReturnStatement(ret) => {
                if let Some(value) = &mut ret.argument {
                    values.push(Returned { value, top_level });
                }
            }
            Statement::BlockStatement(block) => {
                collect_returns(&mut block.body, depth - 1, false, values);
            }
            Statement::IfStatement(if_stmt) => {
                let IfStatement {
                    consequent,
                    alternate,
                    ..
                } = &mut **if_stmt;
                collect_branch_returns(consequent, depth - 1, values);
                if let Some(alternate) = alternate {
                    collect_branch_returns(alternate, depth - 1, values);
                }
            }
            Statement::TryStatement(try_stmt) => {
                let TryStatement {
                    block,
                    handler,
                    finalizer,
                    ..
                } = &mut **try_stmt;
                collect_returns(&mut block.body, depth - 1, false, values);
                if let Some(handler) = handler {
                    collect_returns(&mut handler.body.body, depth - 1, false, values);
                }
                if let Some(finalizer) = finalizer {
                    collect_returns(&mut finalizer.body, depth - 1, false, values);
                }
            }
            Statement::SwitchStatement(switch) => {
                for case in &mut switch.cases {
                    collect_returns(&mut case.consequent, depth - 1, false, values);
                }
            }
            _ => {}
        }
    }
}

fn collect_branch_returns<'p, 'a>(
    statement: &'p mut Statement<'a>,
    depth: u8,
    values: &mut Vec<Returned<'p, 'a>>,
) {
    match statement {
        Statement::BlockStatement(block) => collect_returns(&mut block.body, depth, false, values),
        other => collect_returns(std::slice::from_mut(other), depth, false, values),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::super::{
        extract_config_path_aliases, extract_config_string, extract_config_string_or_array,
    };

    fn js() -> &'static Path {
        Path::new("webpack.config.js")
    }

    fn aliases(source: &str) -> Vec<(String, PathBuf)> {
        extract_config_path_aliases(source, js(), &["resolve", "alias"])
    }

    #[derive(Default)]
    struct NodeCount(usize);

    impl<'a> oxc_ast_visit::Visit<'a> for NodeCount {
        fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
            self.0 += 1;
        }
    }

    /// The number of AST nodes after both inline passes.
    fn inlined_node_count(source: &str) -> usize {
        super::super::extract_from_source(source, js(), |program| {
            let mut count = NodeCount::default();
            oxc_ast_visit::Visit::visit_program(&mut count, program);
            Some(count.0)
        })
        .expect("source parses")
    }

    #[test]
    fn copies_of_expanded_constants_stay_within_the_budget() {
        // The top-level pass expands `C` to 64 copies of `A`, but the source
        // of `C` stays short. Each pass charges a copy by its node count, so
        // the AST stays within the node budget of both passes.
        let props: Vec<String> = (0..50).map(|i| format!("p{i}: {i}")).collect();
        let eight = |name: &str| {
            (0..8)
                .map(|i| format!("k{i}: {name}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let many_c = vec!["C"; 64].join(", ");
        let source = format!(
            "const A = {{ {props} }};\nconst B = {{ {b} }};\nconst C = {{ {c} }};\n\
             module.exports = () => ({{ entry: [{many_c}] }});\n/* {pad} */\n",
            props = props.join(", "),
            b = eight("A"),
            c = eight("B"),
            pad = "x".repeat(30_000),
        );
        let nodes = inlined_node_count(&source);
        assert!(
            nodes <= 4 * source.len(),
            "{nodes} nodes for a source of {} bytes",
            source.len()
        );
    }

    #[test]
    fn many_callback_locals_stay_linear() {
        // The stable filter must not compare each local with each other one.
        let count = 60_000;
        let locals = (0..count)
            .map(|i| format!("const l{i} = \"x\";"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("module.exports = () => {{\n{locals}return {{ entry: l0 }};\n}};\n");
        let start = std::time::Instant::now();
        assert_eq!(
            extract_config_string(&source, js(), &["entry"]),
            Some("x".to_string())
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "{count} locals took {elapsed:?}"
        );
    }

    #[test]
    fn too_many_aliases_block_the_root() {
        let aliases = (0..=4096)
            .map(|i| format!("const a{i} = OPTS.files;"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!(
            "const OPTS = {{ files: [\"./a.ts\"] }};\n{aliases}\nexport default {{ x: OPTS }};\n"
        );
        assert!(extract_config_string_or_array(&source, js(), &["x", "files"]).is_empty());
    }

    fn linkedom() -> Vec<(String, PathBuf)> {
        vec![(
            "linkedom".to_string(),
            PathBuf::from("./src/shims/linkedom.ts"),
        )]
    }

    #[test]
    fn returned_local_with_a_local_spread_resolves() {
        let source = r#"
            const path = require("path");
            module.exports = (env, argv) => {
                const common = { resolve: { alias: { linkedom: path.resolve(__dirname, "./src/shims/linkedom.ts") } } };
                const bg = { ...common, entry: "./src/bg.ts" };
                return bg;
            };
        "#;
        assert_eq!(aliases(source), linkedom());
        assert_eq!(
            extract_config_string_or_array(source, js(), &["entry"]),
            vec!["./src/bg.ts"]
        );
    }

    #[test]
    fn function_declaration_spread_of_a_local_resolves() {
        let source = r#"
            export default function (env) {
                const common = { resolve: { alias: { linkedom: "./src/shims/linkedom.ts" } } };
                return { ...common, entry: { main: "./src/main.ts" } };
            }
        "#;
        assert_eq!(aliases(source), linkedom());
        assert_eq!(
            extract_config_string_or_array(source, js(), &["entry"]),
            vec!["./src/main.ts"]
        );
    }

    #[test]
    fn arrow_expression_body_spread_of_a_top_level_constant_resolves() {
        let source = r#"
            const common = { resolve: { alias: { linkedom: "./src/shims/linkedom.ts" } } };
            module.exports = (env) => ({ ...common, entry: "./src/main.ts" });
        "#;
        assert_eq!(aliases(source), linkedom());
    }

    #[test]
    fn member_spread_keeps_the_last_value_of_a_key() {
        let source = r#"
            module.exports = () => {
                const common = { resolve: { alias: { a: "./a.ts" }, fallback: { fs: false } } };
                return {
                    resolve: { ...common.resolve, fallback: { ...common.resolve.fallback, net: false } },
                    other: { ...common.resolve, alias: { b: "./b.ts" } },
                };
            };
        "#;
        assert_eq!(
            extract_config_path_aliases(source, js(), &["resolve", "alias"]),
            vec![("a".to_string(), PathBuf::from("./a.ts"))]
        );
        assert_eq!(
            extract_config_path_aliases(source, js(), &["other", "alias"]),
            vec![("b".to_string(), PathBuf::from("./b.ts"))]
        );
    }

    #[test]
    fn spread_after_a_literal_key_overrides_that_key() {
        let source = r#"
            module.exports = () => {
                const common = { alias: { a: "./a.ts" } };
                return { resolve: { alias: { b: "./b.ts" }, ...common } };
            };
        "#;
        assert_eq!(
            aliases(source),
            vec![("a".to_string(), PathBuf::from("./a.ts"))]
        );
    }

    #[test]
    fn local_that_an_alias_or_object_assign_can_change_does_not_resolve() {
        let cases = [
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; Object.assign(common.resolve.alias, { a: "./b.ts" }); return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const { resolve } = common; resolve.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const r = common.resolve; r.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; let r; r = common.resolve; r.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const r = common?.resolve; r.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const r = common.resolve; const q = r.alias; q.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; Object.defineProperty(common.resolve.alias, "a", { value: "./b.ts" }); return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; Reflect.set(common.resolve.alias, "a", "./b.ts"); return common; };"#,
        ];
        for source in cases {
            assert!(aliases(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn local_that_an_alias_only_reads_still_resolves() {
        let source = r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const r = common.resolve; const n = r.alias.a.length; return common; };"#;
        assert_eq!(aliases(source).len(), 1, "{source}");
    }

    #[test]
    fn top_level_returned_local_does_not_hide_a_branch_return() {
        let source = r#"
            export default defineConfig(({ mode }) => {
                const base = { resolve: { conditions: ["x"] } };
                if (mode === "test") {
                    return { ...base, test: { setupFiles: ["./src/setup.ts"] } };
                }
                return base;
            });
        "#;
        let path = Path::new("vitest.config.ts");
        assert_eq!(
            extract_config_string_or_array(source, path, &["test", "setupFiles"]),
            vec!["./src/setup.ts"]
        );
        assert_eq!(
            extract_config_string_or_array(source, path, &["resolve", "conditions"]),
            vec!["x"]
        );
    }

    #[test]
    fn branch_return_without_an_object_keeps_the_top_level_returned_local() {
        let vite = r#"
            export default defineConfig(({ command }) => {
                const config = { test: { setupFiles: ["./src/setup.ts"] } };
                if (command === "serve") {
                    return mergeConfig(config, devOverrides);
                }
                return config;
            });
        "#;
        assert_eq!(
            extract_config_string_or_array(
                vite,
                Path::new("vite.config.ts"),
                &["test", "setupFiles"]
            ),
            vec!["./src/setup.ts"]
        );
        let webpack = r#"
            module.exports = (env) => {
                const config = { entry: "./src/main.ts" };
                if (!env) return null;
                return config;
            };
        "#;
        assert_eq!(
            extract_config_string(webpack, js(), &["entry"]),
            Some("./src/main.ts".to_string())
        );
    }

    #[test]
    fn branch_return_of_an_array_keeps_the_top_level_returned_array() {
        let source = r#"
            module.exports = (env) => {
                const a = { entry: "./src/a.ts" };
                const b = { entry: "./src/b.ts" };
                const all = [a, b];
                if (env.only) return [a];
                return all;
            };
        "#;
        let entries: Vec<_> =
            super::super::extract_each_config_entries(source, js(), &["entry"], "context")
                .into_iter()
                .flat_map(|(entries, _)| entries)
                .collect();
        assert_eq!(entries, vec!["./src/a.ts", "./src/b.ts"]);
    }

    #[test]
    fn branch_return_of_an_object_still_wins_over_a_returned_array_local() {
        // A top-level array never gives an object to `find_config_object`, so
        // the inlined array keeps the multi-config reader working.
        let source = r#"
            module.exports = (env) => {
                const a = { entry: "./src/a.ts" };
                const all = [a];
                if (env.single) return { entry: "./src/single.ts" };
                return all;
            };
        "#;
        assert_eq!(
            extract_config_string(source, js(), &["entry"]),
            Some("./src/single.ts".to_string())
        );
    }

    #[test]
    fn config_function_exported_through_a_binding_resolves_locals() {
        let cases = [
            r#"const config = (env) => { const common = { resolve: { alias: { linkedom: "./src/shims/linkedom.ts" } } }; return { ...common, entry: "./src/main.ts" }; }; module.exports = config;"#,
            r#"const config = function (env) { const common = { resolve: { alias: { linkedom: "./src/shims/linkedom.ts" } } }; return { ...common, entry: "./src/main.ts" }; }; export default config;"#,
        ];
        for source in cases {
            assert_eq!(aliases(source), linkedom(), "{source}");
            assert_eq!(
                extract_config_string(source, js(), &["entry"]),
                Some("./src/main.ts".to_string()),
                "{source}"
            );
        }
    }

    #[test]
    fn returned_top_level_constant_resolves_without_a_callback_local() {
        let cases = [
            r#"const base = { entry: "./src/main.ts" }; module.exports = () => { return base; };"#,
            r#"const base = { entry: "./src/main.ts" }; module.exports = () => base;"#,
        ];
        for source in cases {
            assert_eq!(
                extract_config_string(source, js(), &["entry"]),
                Some("./src/main.ts".to_string()),
                "{source}"
            );
        }
    }

    #[test]
    fn local_that_a_hidden_alias_or_a_call_can_change_does_not_resolve() {
        let entry_cases = [
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; for (const c of [web]) c.entry = "./src/new.ts"; return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; const list = [web]; for (const c of list) c.entry = "./src/new.ts"; return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; const configure = (cfg) => { cfg.entry = "./src/new.ts"; }; configure(web); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; _.merge(web, { entry: "./src/new.ts" }); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; const list = [web]; addPlugins(list); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; const all = { web }; configure(all); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; globalThis.Object.assign(web, { entry: "./src/new.ts" }); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/old.ts" }; const { assign } = Object; assign(web, { entry: "./src/new.ts" }); return [web]; };"#,
        ];
        for source in entry_cases {
            let entries: Vec<_> =
                super::super::extract_each_config_entries(source, js(), &["entry"], "context")
                    .into_iter()
                    .flat_map(|(entries, _)| entries)
                    .collect();
            assert!(entries.is_empty(), "{source}: {entries:?}");
        }
        let alias_cases = [
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; let r; ({ resolve: r } = common); r.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const list = [common]; list[0].resolve.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const holder = { c: common }; holder.c.resolve.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const web = { ...common }; web.resolve.alias.a = "./b.ts"; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const r = common.resolve; configure(r); return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; configure(common.resolve); return common; };"#,
        ];
        for source in alias_cases {
            assert!(aliases(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn top_level_constant_that_a_call_can_change_does_not_resolve() {
        // Only a direct argument counts, and `new` counts like a call.
        let cases = [
            r#"const OPTS = { files: ["./a.ts"] }; configure(OPTS); export default { x: OPTS };"#,
            r#"const OPTS = { files: ["./a.ts"] }; new Mutator(OPTS); export default { x: OPTS };"#,
            r#"const { merge } = require("lodash"); const OPTS = { files: ["./a.ts"] }; merge(OPTS, {}); export default { x: OPTS };"#,
        ];
        for source in cases {
            assert!(
                extract_config_string_or_array(
                    source,
                    Path::new("vitest.config.ts"),
                    &["x", "files"]
                )
                .is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn constant_in_a_literal_argument_or_an_exported_call_still_resolves() {
        let path = Path::new("vitest.config.ts");
        let cases = [
            r#"import { defineVitestConfig } from "@nuxt/test-utils/config"; const setupFiles = ["./test/setup.ts"]; export default defineVitestConfig({ test: { setupFiles } });"#,
            r#"import { defineConfig } from "vitest/config"; import checker from "vite-plugin-checker"; const setupFiles = ["./test/setup.ts"]; export default defineConfig({ plugins: [checker({ files: setupFiles })], test: { setupFiles } });"#,
        ];
        for source in cases {
            assert_eq!(
                extract_config_string_or_array(source, path, &["test", "setupFiles"]),
                vec!["./test/setup.ts"],
                "{source}"
            );
        }
    }

    #[test]
    fn local_that_a_read_only_call_gets_still_resolves() {
        let cases = [
            r#"module.exports = () => { const web = { entry: "./src/main.ts" }; console.log(web); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/main.ts" }; const out = path.resolve(__dirname, web.entry); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/main.ts" }; const keys = Object.keys(web); return [web]; };"#,
            r#"module.exports = () => { const web = { entry: "./src/main.ts" }; const merged = Object.assign({}, web); return [web]; };"#,
            r#"module.exports = () => { const ENTRY = "./src/main.ts"; check(ENTRY); const web = { entry: ENTRY }; return [web]; };"#,
        ];
        for source in cases {
            let entries: Vec<_> =
                super::super::extract_each_config_entries(source, js(), &["entry"], "context")
                    .into_iter()
                    .flat_map(|(entries, _)| entries)
                    .collect();
            assert_eq!(entries, vec!["./src/main.ts"], "{source}");
        }
    }

    #[test]
    fn unstable_or_unknown_locals_do_not_resolve() {
        let cases = [
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; common.resolve = {}; return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; common.plugins.push(1); return common; };"#,
            r#"module.exports = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; const f = (common) => common; return common; };"#,
            r#"module.exports = () => { var common = { resolve: { alias: { a: "./a.ts" } } }; return common; };"#,
            r#"module.exports = () => { const common = merge(a, { resolve: { alias: { a: "./a.ts" } } }); return { ...common }; };"#,
            r#"module.exports = () => ({ ...require("./base"), entry: "./a.ts" });"#,
            r#"module.exports = () => { const helper = () => { const common = { resolve: { alias: { a: "./a.ts" } } }; return common; }; return helper(); };"#,
        ];
        for source in cases {
            assert!(aliases(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn key_before_an_unresolved_spread_stays() {
        let source = r#"
            module.exports = () => {
                const base = { entry: "./base.ts" };
                return { ...base, entry: "./first.ts", ...unknown };
            };
        "#;
        assert_eq!(
            extract_config_string(source, js(), &["entry"]),
            Some("./first.ts".to_string())
        );
    }

    #[test]
    fn literal_duplicate_keys_without_a_spread_keep_the_first_match() {
        let source = r#"module.exports = { entry: "./first.ts", entry: "./second.ts" };"#;
        assert_eq!(
            extract_config_string(source, js(), &["entry"]),
            Some("./first.ts".to_string())
        );
    }

    #[test]
    fn deep_chains_stop_at_the_bound_and_self_spreads_end() {
        let source = r#"
            module.exports = () => {
                const a = { ...a, entry: "./a.ts" };
                const l1 = { x: "./deep.ts" };
                const l2 = { l1 };
                const l3 = { l2 };
                const l4 = { l3 };
                const l5 = { l4 };
                return { ...a, l5 };
            };
        "#;
        assert_eq!(
            extract_config_string(source, js(), &["entry"]),
            Some("./a.ts".to_string())
        );
        assert_eq!(
            extract_config_string(source, js(), &["l5", "l4", "l3", "l2", "l1", "x"]),
            None
        );
    }
}
