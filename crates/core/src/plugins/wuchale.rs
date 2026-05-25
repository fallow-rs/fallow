//! Wuchale i18n plugin.
//!
//! Wuchale consumes `wuchale.config.js` directly. The Vite integration can also
//! point at a custom JavaScript config module through a static `configFile`
//! option passed to `@wuchale/vite-plugin`.

use std::fs;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, ImportDeclaration, ObjectExpression,
    VariableDeclaration,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["wuchale", "@wuchale/vite-plugin"];
const CONFIG_PATTERNS: &[&str] = &[
    "wuchale.config.js",
    "**/wuchale.config.js",
    "vite.config.{ts,js,mts,mjs}",
    "**/vite.config.{ts,js,mts,mjs}",
];
const ALWAYS_USED: &[&str] = &["wuchale.config.js", "**/wuchale.config.js"];
const TOOLING_DEPENDENCIES: &[&str] = &["wuchale", "@wuchale/vite-plugin"];
const VITE_PLUGIN_PACKAGE: &str = "@wuchale/vite-plugin";
const VITE_PLUGIN_EXPORTS: &[&str] = &["wuchale", "vitePlugin"];

pub struct WuchalePlugin;

impl Plugin for WuchalePlugin {
    fn name(&self) -> &'static str {
        "wuchale"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        ENABLERS
            .iter()
            .any(|enabler| deps.iter().any(|dep| dep == enabler))
            || root.join("wuchale.config.js").is_file()
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        if is_wuchale_config(config_path) {
            credit_config_imports(&mut result, source, config_path);
            return result;
        }

        if !is_vite_config(config_path) {
            return result;
        }

        for custom_config in extract_vite_custom_config_files(source, config_path, root) {
            result.always_used_files.push(custom_config.clone());
            let custom_path = root.join(&custom_config);
            if let Ok(custom_source) = fs::read_to_string(&custom_path) {
                credit_config_imports(&mut result, &custom_source, &custom_path);
            }
        }

        result
    }
}

fn is_wuchale_config(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "wuchale.config.js")
}

fn is_vite_config(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        "vite.config.ts" | "vite.config.js" | "vite.config.mts" | "vite.config.mjs"
    )
}

fn credit_config_imports(result: &mut PluginResult, source: &str, config_path: &Path) {
    let mut specifiers = config_parser::extract_imports_and_requires(source, config_path);
    for require in extract_require_sources(source, config_path) {
        push_unique(&mut specifiers, require);
    }

    for specifier in specifiers {
        if is_package_specifier(&specifier) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&specifier));
        }
    }
}

fn extract_require_sources(source: &str, config_path: &Path) -> Vec<String> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();
    let mut collector = RequireSourceCollector::default();
    collector.visit_program(&parsed.program);
    collector.sources
}

#[derive(Default)]
struct RequireSourceCollector {
    sources: Vec<String>,
}

impl<'a> Visit<'a> for RequireSourceCollector {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(source) = require_call_source(call) {
            push_unique(&mut self.sources, source);
        }

        walk::walk_call_expression(self, call);
    }
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.starts_with('.') && !specifier.starts_with('/') && !specifier.is_empty()
}

fn extract_vite_custom_config_files(source: &str, config_path: &Path, root: &Path) -> Vec<String> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();
    let mut collector = WuchaleVitePluginCollector {
        callable_names: Vec::new(),
        namespaces: Vec::new(),
        custom_config_files: Vec::new(),
        config_path,
        root,
    };
    collector.visit_program(&parsed.program);
    collector.custom_config_files
}

struct WuchaleVitePluginCollector<'a> {
    callable_names: Vec<String>,
    namespaces: Vec<String>,
    custom_config_files: Vec<String>,
    config_path: &'a Path,
    root: &'a Path,
}

impl<'a> Visit<'a> for WuchaleVitePluginCollector<'_> {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if decl.source.value != VITE_PLUGIN_PACKAGE {
            return;
        }

        if let Some(specifiers) = &decl.specifiers {
            for specifier in specifiers {
                match specifier {
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier)
                        if VITE_PLUGIN_EXPORTS.contains(&specifier.imported.name().as_ref()) =>
                    {
                        push_unique(&mut self.callable_names, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                        push_unique(&mut self.callable_names, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                        specifier,
                    ) => {
                        push_unique(&mut self.namespaces, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(_) => {}
                }
            }
        }
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            let Some(source) = require_source(init) else {
                continue;
            };
            if source != VITE_PLUGIN_PACKAGE {
                continue;
            }

            match &declarator.id {
                BindingPattern::BindingIdentifier(identifier) => {
                    let name = identifier.name.to_string();
                    push_unique(&mut self.callable_names, name.clone());
                    push_unique(&mut self.namespaces, name);
                }
                BindingPattern::ObjectPattern(object) => {
                    for prop in &object.properties {
                        let Some(exported) = prop.key.static_name() else {
                            continue;
                        };
                        if VITE_PLUGIN_EXPORTS.contains(&exported.as_ref())
                            && let BindingPattern::BindingIdentifier(identifier) = &prop.value
                        {
                            push_unique(&mut self.callable_names, identifier.name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        walk::walk_variable_declaration(self, decl);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.is_wuchale_vite_plugin_call(call)
            && let Some(Expression::ObjectExpression(options)) =
                call.arguments.first().and_then(Argument::as_expression)
            && let Some(custom_config) = custom_config_file(options, self.config_path, self.root)
        {
            push_unique(&mut self.custom_config_files, custom_config);
        }

        walk::walk_call_expression(self, call);
    }
}

impl WuchaleVitePluginCollector<'_> {
    fn is_wuchale_vite_plugin_call(&self, call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(identifier) => self
                .callable_names
                .iter()
                .any(|name| name == identifier.name.as_str()),
            Expression::StaticMemberExpression(member) if matches!(&member.object, Expression::Identifier(object) if self.namespaces.iter().any(|name| name == object.name.as_str())) => {
                VITE_PLUGIN_EXPORTS.contains(&member.property.name.as_str())
            }
            _ => false,
        }
    }
}

fn custom_config_file(
    options: &ObjectExpression<'_>,
    config_path: &Path,
    root: &Path,
) -> Option<String> {
    let raw = config_parser::property_expr(options, "configFile")
        .and_then(config_parser::expression_to_path_string)?;
    let normalized = config_parser::normalize_config_path(&raw, config_path, root)?;
    Path::new(&normalized)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
        .then_some(normalized)
}

fn require_source(expr: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    require_call_source(call)
}

fn require_call_source(call: &CallExpression<'_>) -> Option<String> {
    if !matches!(&call.callee, Expression::Identifier(id) if id.name == "require") {
        return None;
    }
    call.arguments.first().and_then(|arg| {
        if let Argument::StringLiteral(source) = arg {
            Some(source.value.to_string())
        } else {
            None
        }
    })
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.contains(&value) {
        items.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_used_includes_documented_js_config_only() {
        let plugin = WuchalePlugin;

        assert!(plugin.always_used().contains(&"wuchale.config.js"));
        assert!(plugin.always_used().contains(&"**/wuchale.config.js"));
        assert!(!plugin.always_used().contains(&"wuchale.config.ts"));
        assert!(!plugin.config_patterns().contains(&"wuchale.config.ts"));
    }

    #[test]
    fn activates_from_documented_js_config_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let plugin = WuchalePlugin;
        let deps = Vec::new();

        assert!(!plugin.is_enabled_with_deps(&deps, tmp.path()));

        std::fs::write(tmp.path().join("wuchale.config.js"), "export default {};\n")
            .expect("wuchale config");
        assert!(plugin.is_enabled_with_deps(&deps, tmp.path()));
    }

    #[test]
    fn resolve_config_credits_wuchale_config_imports_and_requires() {
        let source = r#"
            import { defineConfig } from "wuchale";
            import { adapter as svelte } from "@wuchale/svelte";
            const custom = require("@acme/wuchale-adapter");
            export default defineConfig({ adapters: { main: svelte(), custom: custom() } });
        "#;
        let plugin = WuchalePlugin;
        let result = plugin.resolve_config(
            Path::new("wuchale.config.js"),
            source,
            Path::new("/project"),
        );

        for dep in ["wuchale", "@wuchale/svelte", "@acme/wuchale-adapter"] {
            assert!(
                result.referenced_dependencies.contains(&dep.to_string()),
                "{dep} should be credited from Wuchale config imports: {:?}",
                result.referenced_dependencies
            );
        }
    }

    #[test]
    fn resolve_config_extracts_vite_config_file_from_named_import() {
        let source = r#"
            import { defineConfig } from "vite";
            import { wuchale } from "@wuchale/vite-plugin";
            export default defineConfig({
                plugins: [wuchale({ configFile: "./config/custom-wuchale.config.js" })],
            });
        "#;
        let plugin = WuchalePlugin;
        let result = plugin.resolve_config(
            Path::new("/project/apps/web/vite.config.ts"),
            source,
            Path::new("/project"),
        );

        assert_eq!(
            result.always_used_files,
            vec!["apps/web/config/custom-wuchale.config.js".to_string()]
        );
    }

    #[test]
    fn resolve_config_extracts_vite_config_file_from_require_binding() {
        let source = r#"
            const { vitePlugin: wuchale } = require("@wuchale/vite-plugin");
            module.exports = {
                plugins: [wuchale({ configFile: "./wuchale.custom.js" })],
            };
        "#;
        let plugin = WuchalePlugin;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.js"),
            source,
            Path::new("/project"),
        );

        assert_eq!(
            result.always_used_files,
            vec!["wuchale.custom.js".to_string()]
        );
    }

    #[test]
    fn resolve_config_ignores_dynamic_config_file() {
        let source = r#"
            import { wuchale } from "@wuchale/vite-plugin";
            export default { plugins: [wuchale({ configFile: process.env.WUCHALE_CONFIG })] };
        "#;
        let plugin = WuchalePlugin;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.js"),
            source,
            Path::new("/project"),
        );

        assert!(result.always_used_files.is_empty());
    }

    #[test]
    fn resolve_config_ignores_unsupported_ts_custom_config_file() {
        let source = r#"
            import { wuchale } from "@wuchale/vite-plugin";
            export default { plugins: [wuchale({ configFile: "./wuchale.config.ts" })] };
        "#;
        let plugin = WuchalePlugin;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.js"),
            source,
            Path::new("/project"),
        );

        assert!(result.always_used_files.is_empty());
    }
}
