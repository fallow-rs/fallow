//! React Compiler dependency crediting, shared by the Vite and Electron plugins.
//!
//! `babel-plugin-react-compiler` is rarely referenced by its literal package
//! name in a config; it is enabled through `@vitejs/plugin-react` / the
//! standalone `@rolldown/plugin-babel` plugin. This module credits the
//! dependency when React Compiler is wired through the documented shapes so it
//! is not reported as an unused dependency.
//!
//! Supported shapes (all provenance-checked: the `react` / `babel` callees and
//! the `reactCompilerPreset` helper must be imported from their real packages):
//!
//! - `react({ babel: { plugins: ["babel-plugin-react-compiler"] } })` (#623)
//! - `react({ babel: { plugins: [["react-compiler", {}]] } })` (#623)
//! - `babel({ babel: { plugins: ["babel-plugin-react-compiler"] } })` (#623)
//! - `babel({ presets: [reactCompilerPreset()] })` (#751, the documented
//!   `@vitejs/plugin-react` 6.x + `@rolldown/plugin-babel` canonical shape)
//! - `react({ babel: { presets: [reactCompilerPreset()] } })` (#751)
//! - the same arrays inside electron-vite `main` / `preload` / `renderer`
//!   sections (#751)
//!
//! Out of scope (documented limitations, reported as unused so the user is at
//! least told the dependency looks unreferenced): the namespace-import form
//! `import * as vr from '@vitejs/plugin-react'; vr.reactCompilerPreset()`, the
//! variable-indirection form `const p = reactCompilerPreset(); babel({ presets:
//! [p] })`, and a local re-export barrel that does not import directly from
//! `@vitejs/plugin-react`.

use super::config_parser;
use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, CallExpression, Expression,
    ImportDeclarationSpecifier, ObjectExpression, Program, Statement,
};

pub(super) const REACT_COMPILER_BABEL_PLUGIN: &str = "babel-plugin-react-compiler";
const VITE_REACT_PLUGIN_SOURCE: &str = "@vitejs/plugin-react";
const ROLLDOWN_BABEL_PLUGIN_SOURCE: &str = "@rolldown/plugin-babel";

#[derive(Default)]
#[expect(
    clippy::struct_field_names,
    reason = "each field is a distinct list of local binding names; the shared _calls suffix is the shared semantics, not noise"
)]
struct ReactCompilerLocals {
    /// Local names bound to the `@vitejs/plugin-react` plugin factory.
    react_calls: Vec<String>,
    /// Local names bound to the `@rolldown/plugin-babel` plugin factory.
    babel_calls: Vec<String>,
    /// Local names bound to the `reactCompilerPreset` named export of
    /// `@vitejs/plugin-react`.
    react_compiler_preset_calls: Vec<String>,
}

/// Credit `babel-plugin-react-compiler` when it is wired through a React /
/// Rolldown babel plugin call located at any of `plugin_array_paths`. Each path
/// is resolved against the config object via [`nested_array_expression`]: the
/// Vite plugin passes `&[&["plugins"]]` (top-level `plugins`), the Electron
/// plugin passes the per-section `<section>.plugins` paths.
pub(super) fn extract_dependencies(
    source: &str,
    path: &std::path::Path,
    plugin_array_paths: &[&[&str]],
) -> Vec<String> {
    config_parser::extract_from_source(source, path, |program| {
        let config = config_parser::find_config_object_pub(program)?;
        let locals = collect_locals(program);
        let mut deps = Vec::new();

        for plugins_path in plugin_array_paths {
            if let Some(plugins) = nested_array_expression(config, plugins_path) {
                collect_from_plugins_array(plugins, &locals, &mut deps);
            }
        }

        (!deps.is_empty()).then_some(deps)
    })
    .unwrap_or_default()
}

fn collect_locals(program: &Program<'_>) -> ReactCompilerLocals {
    let mut locals = ReactCompilerLocals::default();

    for stmt in &program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        let Some(specifiers) = &decl.specifiers else {
            continue;
        };

        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default)
                    if decl.source.value == VITE_REACT_PLUGIN_SOURCE =>
                {
                    push_unique(&mut locals.react_calls, default.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier)
                    if decl.source.value == VITE_REACT_PLUGIN_SOURCE
                        && specifier.imported.name() == "react" =>
                {
                    push_unique(&mut locals.react_calls, specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier)
                    if decl.source.value == VITE_REACT_PLUGIN_SOURCE
                        && specifier.imported.name() == "reactCompilerPreset" =>
                {
                    push_unique(
                        &mut locals.react_compiler_preset_calls,
                        specifier.local.name.to_string(),
                    );
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default)
                    if decl.source.value == ROLLDOWN_BABEL_PLUGIN_SOURCE =>
                {
                    push_unique(&mut locals.babel_calls, default.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier)
                    if decl.source.value == ROLLDOWN_BABEL_PLUGIN_SOURCE
                        && specifier.imported.name() == "babel" =>
                {
                    push_unique(&mut locals.babel_calls, specifier.local.name.to_string());
                }
                _ => {}
            }
        }
    }

    locals
}

fn collect_from_plugins_array(
    plugins: &ArrayExpression<'_>,
    locals: &ReactCompilerLocals,
    deps: &mut Vec<String>,
) {
    for element in &plugins.elements {
        let Some(Expression::CallExpression(call)) = element.as_expression() else {
            continue;
        };

        if is_local_call(call, &locals.react_calls) {
            // @vitejs/plugin-react: babel options live under the `babel` key.
            // Plugin-name strings in `babel.plugins` (#623); the preset helper
            // in `babel.presets` (#751).
            credit_plugin_name_strings(call, &["babel", "plugins"], deps);
            credit_preset_helper_calls(call, &["babel", "presets"], locals, deps);
        } else if is_local_call(call, &locals.babel_calls) {
            // @rolldown/plugin-babel: plugin-name strings in `plugins` /
            // `babel.plugins`; the preset helper in the top-level `presets` (the
            // canonical reactCompilerPreset shape) or nested `babel.presets`.
            credit_plugin_name_strings(call, &["plugins"], deps);
            credit_plugin_name_strings(call, &["babel", "plugins"], deps);
            credit_preset_helper_calls(call, &["presets"], locals, deps);
            credit_preset_helper_calls(call, &["babel", "presets"], locals, deps);
        }
    }
}

/// Credit react-compiler when it appears as a Babel plugin-name string or
/// `[name, opts]` tuple in a `plugins` array (the #623 shape). Restricted to
/// `plugins` paths: this resolves Babel PLUGIN names, and the `reactCompilerPreset`
/// helper is handled separately for `presets` paths.
fn credit_plugin_name_strings(
    call: &CallExpression<'_>,
    option_path: &[&str],
    deps: &mut Vec<String>,
) {
    let Some(entries) = call_option_array(call, option_path) else {
        return;
    };
    for plugin_name in collect_babel_plugin_names(entries) {
        if super::babel::resolve_babel_plugin_name(&plugin_name) == REACT_COMPILER_BABEL_PLUGIN {
            push_unique(deps, REACT_COMPILER_BABEL_PLUGIN.to_string());
        }
    }
}

/// Credit react-compiler when the `reactCompilerPreset()` helper appears in a
/// `presets` array. The callee must be a provenance-checked `reactCompilerPreset`
/// named import from `@vitejs/plugin-react`; the call is credited directly (a
/// preset call has no string name, so it must not be routed through
/// `resolve_babel_plugin_name`). Argument-agnostic: `reactCompilerPreset({ target:
/// "19" })` still credits. Restricted to `presets` paths: a preset helper inside a
/// `plugins` array is an invalid shape that does not enable React Compiler, so it
/// is not credited (see .claude/rules/detection.md).
fn credit_preset_helper_calls(
    call: &CallExpression<'_>,
    option_path: &[&str],
    locals: &ReactCompilerLocals,
    deps: &mut Vec<String>,
) {
    let Some(entries) = call_option_array(call, option_path) else {
        return;
    };
    for element in &entries.elements {
        if let Some(Expression::CallExpression(inner)) = element.as_expression()
            && is_local_call(inner, &locals.react_compiler_preset_calls)
        {
            push_unique(deps, REACT_COMPILER_BABEL_PLUGIN.to_string());
        }
    }
}

/// Resolve the array at `option_path` inside the plugin call's first (options)
/// object argument.
fn call_option_array<'a>(
    call: &'a CallExpression<'a>,
    option_path: &[&str],
) -> Option<&'a ArrayExpression<'a>> {
    let options = call
        .arguments
        .first()
        .and_then(Argument::as_expression)
        .and_then(config_parser::object_expression)?;
    nested_array_expression(options, option_path)
}

fn nested_array_expression<'a>(
    obj: &'a ObjectExpression<'a>,
    path: &[&str],
) -> Option<&'a ArrayExpression<'a>> {
    let mut current_obj = obj;
    for (index, key) in path.iter().enumerate() {
        let expr = config_parser::property_expr(current_obj, key)?;
        if index == path.len() - 1 {
            return config_parser::array_expression(expr);
        }
        current_obj = config_parser::object_expression(expr)?;
    }
    None
}

fn collect_babel_plugin_names(plugins: &ArrayExpression<'_>) -> Vec<String> {
    plugins
        .elements
        .iter()
        .filter_map(|element| {
            let expr = element.as_expression()?;
            config_parser::expression_to_string(expr).or_else(|| {
                let tuple = config_parser::array_expression(expr)?;
                tuple
                    .elements
                    .first()
                    .and_then(ArrayExpressionElement::as_expression)
                    .and_then(config_parser::expression_to_string)
            })
        })
        .collect()
}

fn is_local_call(call: &CallExpression<'_>, locals: &[String]) -> bool {
    matches!(
        &call.callee,
        Expression::Identifier(identifier)
            if locals.iter().any(|local| local == identifier.name.as_str())
    )
}

fn push_unique<T: Eq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vite top-level `plugins` (the only path the Vite plugin uses).
    fn vite_deps(source: &str) -> Vec<String> {
        extract_dependencies(
            source,
            std::path::Path::new("/project/vite.config.ts"),
            &[&["plugins"]],
        )
    }

    /// electron-vite per-section `<section>.plugins` paths.
    fn electron_deps(source: &str) -> Vec<String> {
        extract_dependencies(
            source,
            std::path::Path::new("/project/electron.vite.config.ts"),
            &[
                &["main", "plugins"],
                &["preload", "plugins"],
                &["renderer", "plugins"],
            ],
        )
    }

    fn credits_react_compiler(deps: &[String]) -> bool {
        deps.iter().any(|dep| dep == REACT_COMPILER_BABEL_PLUGIN)
    }

    #[test]
    fn rolldown_babel_preset_call_credits_react_compiler() {
        // The canonical @vitejs/plugin-react 6.x + @rolldown/plugin-babel shape
        // with a mixed default + named import on a single declaration.
        let source = r#"
            import { defineConfig } from "vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
            });
        "#;
        assert!(credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn aliased_preset_import_credits_react_compiler() {
        let source = r#"
            import { defineConfig } from "vite";
            import react, { reactCompilerPreset as rcp } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [react(), babel({ presets: [rcp()] })],
            });
        "#;
        assert!(credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn preset_call_with_options_credits_react_compiler() {
        let source = r#"
            import { defineConfig } from "vite";
            import { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [babel({ presets: [reactCompilerPreset({ target: "19" })] })],
            });
        "#;
        assert!(credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn react_plugin_babel_presets_call_credits_react_compiler() {
        let source = r#"
            import { defineConfig } from "vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [react({ babel: { presets: [reactCompilerPreset()] } })],
            });
        "#;
        assert!(credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn babel_plugins_string_credits_react_compiler() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [react({ babel: { plugins: ["babel-plugin-react-compiler"] } })],
            });
        "#;
        assert!(credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn preset_helper_call_in_babel_plugins_array_does_not_credit() {
        // A preset helper in a `plugins` slot is an invalid shape that does not
        // enable React Compiler; the preset-call scan is restricted to `presets`
        // arrays, so this must NOT credit (matches detection.md).
        let source = r#"
            import { defineConfig } from "vite";
            import { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [babel({ plugins: [reactCompilerPreset()] })],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn preset_helper_call_in_react_babel_plugins_array_does_not_credit() {
        let source = r#"
            import { defineConfig } from "vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [react({ babel: { plugins: [reactCompilerPreset()] } })],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn electron_renderer_preset_call_credits_react_compiler() {
        // electron-vite per-section nesting: plugins live under renderer.plugins,
        // and `defineConfig` is imported from electron-vite (the config-object
        // finder is import-source-agnostic).
        let source = r#"
            import { defineConfig } from "electron-vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                main: { build: { rollupOptions: { input: "src/main/index.ts" } } },
                renderer: {
                    plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
                },
            });
        "#;
        assert!(credits_react_compiler(&electron_deps(source)));
    }

    #[test]
    fn vite_paths_do_not_reach_electron_renderer_plugins() {
        // The Vite path set is top-level `plugins` only, so an electron-vite
        // renderer.plugins preset must NOT be credited when scanned as Vite.
        let source = r#"
            import { defineConfig } from "electron-vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                renderer: {
                    plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
                },
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn local_preset_function_does_not_credit() {
        // A local `reactCompilerPreset` not imported from @vitejs/plugin-react
        // must not credit (provenance gate).
        let source = r#"
            import { defineConfig } from "vite";
            import babel from "@rolldown/plugin-babel";

            function reactCompilerPreset() {
                return {};
            }

            export default defineConfig({
                plugins: [babel({ presets: [reactCompilerPreset()] })],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn variable_indirection_does_not_credit() {
        // Out of scope: the preset call is assigned to a variable rather than
        // appearing directly in the presets array.
        let source = r#"
            import { defineConfig } from "vite";
            import { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            const preset = reactCompilerPreset();

            export default defineConfig({
                plugins: [babel({ presets: [preset] })],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn namespace_import_preset_call_does_not_credit() {
        // Out of scope: namespace import; the callee is a member expression, not
        // a bare identifier, so the provenance gate does not match.
        let source = r#"
            import { defineConfig } from "vite";
            import * as vr from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [babel({ presets: [vr.reactCompilerPreset()] })],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }

    #[test]
    fn unrelated_string_does_not_credit() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        notes: "babel-plugin-react-compiler",
                        babel: {
                            plugins: [["other-plugin", { note: "babel-plugin-react-compiler" }]],
                        },
                    }),
                ],
            });
        "#;
        assert!(!credits_react_compiler(&vite_deps(source)));
    }
}
