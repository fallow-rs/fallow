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
use rustc_hash::FxHashMap;

use super::const_inline::{Inliner, constant_values, literal_value, stable_bindings};
use super::{MAX_CONFIG_BRANCH_DEPTH, MAX_CONFIG_WRAPPER_DEPTH, is_module_exports_target};

/// The budget of the local pass, as a multiple of the source size. A config
/// that spreads one shared local into two configs copies that local twice.
const LOCAL_BUDGET_FACTOR: usize = 2;

/// Inline the stable locals of each exported config callback into the values
/// that the callback returns.
///
/// A local qualifies when a `const` or `let` statement at the callback body's
/// own level declares it with an object, array or string literal, and it is
/// the only binding of its name in the program, with no writes and no mutating
/// calls. The stable top-level constants are available too. The copied source
/// bytes stay at or below `LOCAL_BUDGET_FACTOR` times `source_len`.
pub(super) fn inline_callback_locals<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    source_len: usize,
) {
    let mut values = callback_locals(allocator, program);
    if values.is_empty() {
        return;
    }
    for (name, value) in constant_values(allocator, program) {
        values.entry(name).or_insert(value);
    }
    let mut inliner = Inliner::new(
        allocator,
        &values,
        source_len.saturating_mul(LOCAL_BUDGET_FACTOR),
    );
    for callback in config_callbacks(program) {
        for value in callback.returned_values() {
            inliner.inline_value(value);
        }
    }
}

/// A copy of the literal value of each stable local of the config callbacks.
fn callback_locals<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
) -> FxHashMap<String, Expression<'a>> {
    let mut candidates: Vec<(String, Expression<'a>)> = Vec::new();
    for callback in config_callbacks(program) {
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
        return FxHashMap::default();
    }
    let stable = stable_bindings(
        program,
        candidates.iter().map(|(name, _)| name.as_str()),
        false,
        false,
    );
    let stable: Vec<String> = stable.into_iter().map(str::to_string).collect();
    candidates
        .into_iter()
        .filter(|(name, _)| stable.contains(name))
        .collect()
}

/// The body of a config callback: a function body, or the expression body of
/// an arrow.
enum Callback<'p, 'a> {
    Body(&'p mut FunctionBody<'a>),
    Expression(&'p mut Expression<'a>),
}

impl<'p, 'a> Callback<'p, 'a> {
    /// The values that `find_config_object` can read as the returned config:
    /// the expression body of an arrow, or the argument of each `return` at the
    /// body's own level or in the bounded branch search. Returns of nested
    /// functions belong to those functions and stay out.
    ///
    /// `find_config_object` prefers an object that a top-level `return` gives
    /// over a branch return. A top-level `return base;` gave no object before
    /// this pass, so the search used the first branch return. To keep that
    /// result, a returned identifier at the top level stays when a branch
    /// return exists.
    fn returned_values(self) -> Vec<&'p mut Expression<'a>> {
        let expr = match self {
            Callback::Body(body) => {
                let mut returns = Vec::new();
                collect_returns(
                    &mut body.statements,
                    MAX_CONFIG_BRANCH_DEPTH,
                    true,
                    &mut returns,
                );
                let has_branch_return = returns.iter().any(|ret| !ret.top_level);
                return returns
                    .into_iter()
                    .filter(|ret| {
                        !(has_branch_return
                            && ret.top_level
                            && matches!(
                                ret.value.get_inner_expression(),
                                Expression::Identifier(_)
                            ))
                    })
                    .map(|ret| ret.value)
                    .collect();
            }
            Callback::Expression(expr) => expr,
        };
        vec![expr]
    }
}

/// The config callbacks that the program exports, in the shapes that
/// `find_config_object` reads: an exported function or arrow, also as an
/// argument of an exported call such as `defineConfig(fn)`.
fn config_callbacks<'p, 'a>(program: &'p mut Program<'a>) -> Vec<Callback<'p, 'a>> {
    let mut callbacks = Vec::new();
    for statement in &mut program.body {
        match statement {
            Statement::ExportDefaultDeclaration(decl) => match &mut decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    push_function(func, &mut callbacks);
                }
                kind => {
                    if let Some(expr) = kind.as_expression_mut() {
                        collect_callbacks(expr, MAX_CONFIG_WRAPPER_DEPTH, &mut callbacks);
                    }
                }
            },
            Statement::ExpressionStatement(stmt) => {
                if let Expression::AssignmentExpression(assign) = &mut stmt.expression
                    && is_module_exports_target(&assign.left)
                {
                    collect_callbacks(&mut assign.right, MAX_CONFIG_WRAPPER_DEPTH, &mut callbacks);
                }
            }
            _ => {}
        }
    }
    callbacks
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
