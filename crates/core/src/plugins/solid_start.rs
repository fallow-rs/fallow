//! SolidStart framework plugin.
//!
//! SolidStart loads `<appRoot>/app.{tsx,jsx}` and the client and server
//! entries next to it, and its file router loads every module under
//! `<appRoot>/<routeDir>`. A route file exports `default` (the page), `route`
//! (the route config) and HTTP method handlers for API routes. `appRoot`
//! defaults to `./src` and `routeDir` to `./routes`. Version 2 reads these
//! options from the `solidStart()` Vite plugin call, version 1 from the
//! `defineConfig()` call in `app.config.*`. Both versions take the module
//! that the `middleware` option names.

use std::path::Path;

use oxc_ast::ast::{
    Argument, CallExpression, Expression, ImportDeclaration, ImportDeclarationSpecifier,
    ObjectExpression,
};
use oxc_ast_visit::{Visit, walk};

use super::{PathRule, Plugin, PluginResult, UsedExportRule, config_parser};

const ENABLERS: &[&str] = &["@solidjs/start"];

/// The module that exports `solidStart` (version 2) and `defineConfig`
/// (version 1).
const CONFIG_MODULE: &str = "@solidjs/start/config";

const CONFIG_PATTERNS: &[&str] = &["vite.config.{ts,js,mts,mjs}", "app.config.{ts,js,mts,mjs}"];

/// The version 1 config file. Vinxi loads it, so nothing imports it.
const ALWAYS_USED: &[&str] = &["app.config.{ts,js,mts,mjs}"];

const DEFAULT_APP_ROOT: &str = "src";
const DEFAULT_ROUTE_DIR: &str = "routes";

/// SolidStart's `DEFAULT_EXTENSIONS` for route files.
const ROUTE_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx"];

/// `app` must be JSX. The client and server entries use the same extension.
const ENTRY_EXTENSIONS: &str = "{tsx,jsx}";

const ROUTE_EXPORTS: &[&str] = &[
    "default", "route", "GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS",
];

const DEFAULT_EXPORTS: &[&str] = &["default"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/app.{tsx,jsx}",
    "src/entry-client.{tsx,jsx}",
    "src/entry-server.{tsx,jsx}",
    "src/routes/**/*.{js,jsx,ts,tsx}",
];

pub struct SolidStartPlugin;

impl Plugin for SolidStartPlugin {
    fn name(&self) -> &'static str {
        "solid-start"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        Layout::default().entry_rules()
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        Layout::default().used_export_rules()
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(options) = read_options(source, config_path) else {
            return result;
        };

        let mut layout = Layout::default();
        if let Some(app_root) = options
            .app_root
            .and_then(|raw| config_parser::normalize_config_path(raw.trim(), config_path, root))
        {
            layout.app_root = globset::escape(&app_root);
        }
        if let Some(route_dir) = options.route_dir {
            let route_dir = route_dir.trim().trim_start_matches("./").trim_matches('/');
            if !route_dir.is_empty() && !route_dir.starts_with("..") {
                layout.route_dir = globset::escape(route_dir);
            }
        }
        layout.extra_extensions = options
            .extensions
            .into_iter()
            .map(|ext| ext.trim().trim_start_matches('.').to_string())
            .filter(|ext| !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
            .collect();
        if let Some(middleware) = options
            .middleware
            .and_then(|raw| config_parser::normalize_config_path(raw.trim(), config_path, root))
        {
            layout.middleware = Some(globset::escape(&middleware));
        }

        if layout != Layout::default() {
            result.replace_entry_patterns = true;
            result.replace_used_export_rules = true;
            result.entry_patterns = layout.entry_rules();
            result.used_exports = layout.used_export_rules();
        }
        result
    }
}

/// Where SolidStart finds the files it loads. Each path is relative to the
/// plugin root and escaped for use in a glob.
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    app_root: String,
    route_dir: String,
    extra_extensions: Vec<String>,
    middleware: Option<String>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            app_root: DEFAULT_APP_ROOT.to_string(),
            route_dir: DEFAULT_ROUTE_DIR.to_string(),
            extra_extensions: Vec::new(),
            middleware: None,
        }
    }
}

impl Layout {
    fn entry_rules(&self) -> Vec<PathRule> {
        let mut rules: Vec<PathRule> = self.entry_files().into_iter().map(PathRule::new).collect();
        rules.push(PathRule::new(self.routes_pattern()));
        if let Some(middleware) = &self.middleware {
            rules.push(PathRule::new(middleware.clone()));
        }
        rules
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        let mut rules = vec![UsedExportRule::new(
            self.routes_pattern(),
            ROUTE_EXPORTS.iter().copied(),
        )];
        let [app, _client, server] = self.entry_files();
        rules.push(UsedExportRule::new(app, DEFAULT_EXPORTS.iter().copied()));
        rules.push(UsedExportRule::new(server, DEFAULT_EXPORTS.iter().copied()));
        if let Some(middleware) = &self.middleware {
            rules.push(UsedExportRule::new(
                middleware.clone(),
                DEFAULT_EXPORTS.iter().copied(),
            ));
        }
        rules
    }

    fn entry_files(&self) -> [String; 3] {
        let app_root = &self.app_root;
        ["app", "entry-client", "entry-server"]
            .map(|name| format!("{app_root}/{name}.{ENTRY_EXTENSIONS}"))
    }

    fn routes_pattern(&self) -> String {
        let extensions: Vec<&str> = ROUTE_EXTENSIONS
            .iter()
            .copied()
            .chain(self.extra_extensions.iter().map(String::as_str))
            .collect();
        format!(
            "{}/{}/**/*.{{{}}}",
            self.app_root,
            self.route_dir,
            extensions.join(",")
        )
    }
}

/// The SolidStart options that change which files it loads.
#[derive(Default)]
struct StartOptions {
    app_root: Option<String>,
    route_dir: Option<String>,
    middleware: Option<String>,
    extensions: Vec<String>,
}

/// Read the options object of the last call to a binding that the config
/// imports from `@solidjs/start/config`. `None` when the config makes no such
/// call, so a plain Vite config keeps the defaults.
fn read_options(source: &str, config_path: &Path) -> Option<StartOptions> {
    config_parser::extract_from_source(source, config_path, |program| {
        let mut collector = StartCallCollector::default();
        collector.visit_program(program);
        collector.options
    })
}

#[derive(Default)]
struct StartCallCollector {
    bindings: Vec<String>,
    options: Option<StartOptions>,
}

impl<'a> Visit<'a> for StartCallCollector {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if decl.source.value != CONFIG_MODULE {
            return;
        }
        for specifier in decl.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier {
                self.bindings.push(specifier.local.name.to_string());
            }
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::Identifier(callee) = &call.callee
            && self
                .bindings
                .iter()
                .any(|name| name == callee.name.as_str())
        {
            let options = call
                .arguments
                .first()
                .and_then(Argument::as_expression)
                .and_then(config_parser::object_expression)
                .map(start_options)
                .unwrap_or_default();
            self.options = Some(options);
        }
        walk::walk_call_expression(self, call);
    }
}

fn start_options(obj: &ObjectExpression<'_>) -> StartOptions {
    StartOptions {
        app_root: config_parser::property_string(obj, "appRoot"),
        route_dir: config_parser::property_string(obj, "routeDir"),
        middleware: config_parser::property_string(obj, "middleware"),
        extensions: config_parser::property_expr(obj, "extensions")
            .map(config_parser::expression_to_string_or_array)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(rules: &[PathRule]) -> Vec<&str> {
        rules.iter().map(|rule| rule.pattern.as_str()).collect()
    }

    #[test]
    fn default_rules_use_src_and_routes() {
        let rules = SolidStartPlugin.entry_pattern_rules();
        assert_eq!(patterns(&rules), ENTRY_PATTERNS);
    }

    #[test]
    fn vite_plugin_options_replace_the_defaults() {
        let source = r#"
            import { defineConfig } from "vite";
            import { solidStart as start } from "@solidjs/start/config";
            export default defineConfig({
                plugins: [
                    start({
                        appRoot: "./app",
                        routeDir: "pages",
                        middleware: "./app/middleware.ts",
                        extensions: ["mdx"],
                    }) as PluginOption,
                ],
            });
        "#;
        let result = SolidStartPlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.replace_entry_patterns);
        assert_eq!(
            patterns(&result.entry_patterns),
            vec![
                "app/app.{tsx,jsx}",
                "app/entry-client.{tsx,jsx}",
                "app/entry-server.{tsx,jsx}",
                "app/pages/**/*.{js,jsx,ts,tsx,mdx}",
                "app/middleware.ts",
            ]
        );
    }

    #[test]
    fn plain_vite_config_keeps_the_defaults() {
        let source = r#"
            import { defineConfig } from "vite";
            export default defineConfig({ plugins: [] });
        "#;
        let result = SolidStartPlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn call_without_options_keeps_the_defaults() {
        let source = r#"
            import { solidStart } from "@solidjs/start/config";
            export default { plugins: [solidStart()] };
        "#;
        let result = SolidStartPlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn unrelated_function_with_the_same_name_is_ignored() {
        let source = r#"
            import { solidStart } from "./local";
            export default { plugins: [solidStart({ appRoot: "./other" })] };
        "#;
        let result = SolidStartPlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn route_dir_outside_the_app_root_is_ignored() {
        let source = r#"
            import { solidStart } from "@solidjs/start/config";
            export default { plugins: [solidStart({ routeDir: "../escape" })] };
        "#;
        let result = SolidStartPlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.is_empty());
    }
}
