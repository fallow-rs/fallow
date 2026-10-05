//! Astro framework plugin.
//!
//! Detects Astro projects and marks pages, layouts, content, and middleware
//! as entry points. Parses astro.config to extract referenced dependencies,
//! and the Starlight `components` overrides and `customCss` entries.

use std::path::Path;

use oxc_ast::ast::{
    Argument, CallExpression, Expression, ImportDeclarationSpecifier, ObjectExpression, Program,
    Statement,
};

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["astro"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/pages/**/*.{astro,ts,tsx,js,jsx,mts,mjs,cts,cjs,md,mdx}",
    "src/layouts/**/*.astro",
    "src/content/**/*.{ts,js,mts,mjs,cts,cjs,md,mdx}",
    "src/middleware.{js,ts,mjs,mts,cjs,cts}",
    "src/middleware/index.{js,ts,mjs,mts,cjs,cts}",
    "src/actions/index.{js,ts,mjs,mts,cjs,cts}",
];

const CONFIG_PATTERNS: &[&str] = &["astro.config.{ts,js,mjs}"];

const ALWAYS_USED: &[&str] = &[
    "astro.config.{ts,js,mjs}",
    "src/content/config.{js,ts,mjs,mts,cjs,cts}",
    "src/content.config.{js,ts,mjs,mts,cjs,cts}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["astro", "@astrojs/check", "@astrojs/ts-plugin"];

/// Virtual module prefixes provided by Astro at build time.
/// `astro:` provides built-in modules (content, transitions, env, actions, assets,
/// i18n, middleware, container, schema).
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["astro:"];

const PAGE_EXPORTS: &[&str] = &["getStaticPaths", "prerender", "partial"];
const COMPONENT_PAGE_EXPORTS: &[&str] = &["default", "getStaticPaths", "prerender", "partial"];
const ENDPOINT_EXPORTS: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "HEAD",
    "OPTIONS",
    "ALL",
    "getStaticPaths",
    "prerender",
];
const MIDDLEWARE_EXPORTS: &[&str] = &["onRequest"];
const CONTENT_EXPORTS: &[&str] = &["collections"];
const ACTION_EXPORTS: &[&str] = &["server"];

define_plugin! {
    struct AstroPlugin => "astro",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    virtual_module_prefixes: VIRTUAL_MODULE_PREFIXES,
    used_exports: [
        ("src/pages/**/*.{astro,md,mdx}", PAGE_EXPORTS),
        ("src/pages/**/*.{tsx,jsx}", COMPONENT_PAGE_EXPORTS),
        ("src/pages/**/*.{ts,js,mts,mjs,cts,cjs}", ENDPOINT_EXPORTS),
        ("src/middleware.{js,ts,mjs,mts,cjs,cts}", MIDDLEWARE_EXPORTS),
        ("src/middleware/index.{js,ts,mjs,mts,cjs,cts}", MIDDLEWARE_EXPORTS),
        (
            "src/content/config.{js,ts,mjs,mts,cjs,cts}",
            CONTENT_EXPORTS
        ),
        (
            "src/content.config.{js,ts,mjs,mts,cjs,cts}",
            CONTENT_EXPORTS
        ),
        ("src/actions/index.{js,ts,mjs,mts,cjs,cts}", ACTION_EXPORTS),
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        crate::plugins::add_import_referenced_dependencies(&mut result, source, config_path);
        for value in starlight_file_values(source, config_path) {
            add_starlight_file_value(&mut result, &value, config_path, root);
        }
        result
    },
}

const STARLIGHT_PACKAGE: &str = "@astrojs/starlight";

/// Read the `components` override values and the `customCss` items of each
/// Starlight integration call in the `integrations` array.
fn starlight_file_values(source: &str, config_path: &Path) -> Vec<String> {
    config_parser::extract_from_source(source, config_path, |program| {
        let callees = starlight_import_names(program);
        if callees.is_empty() {
            return None;
        }
        let config = config_parser::find_config_object(program)?;
        let integrations = config_parser::property_expr(config, "integrations")
            .and_then(config_parser::array_expression)?;
        let mut values = Vec::new();
        for element in &integrations.elements {
            let Some(Expression::CallExpression(call)) = element.as_expression() else {
                continue;
            };
            let Some(options) = starlight_call_options(call, &callees) else {
                continue;
            };
            collect_starlight_option_values(options, &mut values);
        }
        Some(values)
    })
    .unwrap_or_default()
}

/// The local names bound to the default export of the Starlight package.
fn starlight_import_names(program: &Program<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in &program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        if decl.source.value != STARLIGHT_PACKAGE {
            continue;
        }
        for specifier in decl.specifiers.iter().flatten() {
            match specifier {
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                    names.push(default.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(named)
                    if named.imported.name().as_ref() == "default" =>
                {
                    names.push(named.local.name.to_string());
                }
                _ => {}
            }
        }
    }
    names
}

fn starlight_call_options<'a>(
    call: &'a CallExpression<'a>,
    callees: &[String],
) -> Option<&'a ObjectExpression<'a>> {
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if !callees.iter().any(|name| name == callee.name.as_str()) {
        return None;
    }
    call.arguments
        .first()
        .and_then(Argument::as_expression)
        .and_then(config_parser::object_expression)
}

fn collect_starlight_option_values(options: &ObjectExpression<'_>, values: &mut Vec<String>) {
    if let Some(components) = config_parser::property_object(options, "components") {
        for property in &components.properties {
            if let Some(value) = property
                .as_property()
                .and_then(|prop| config_parser::expression_to_string(&prop.value))
            {
                values.push(value);
            }
        }
    }
    if let Some(custom_css) =
        config_parser::property_expr(options, "customCss").and_then(config_parser::array_expression)
    {
        values.extend(custom_css.elements.iter().filter_map(|item| {
            item.as_expression()
                .and_then(config_parser::expression_to_string)
        }));
    }
}

/// Credit a Starlight file value: a local path is a used file, and a bare
/// specifier is a used dependency.
fn add_starlight_file_value(
    result: &mut PluginResult,
    value: &str,
    config_path: &Path,
    root: &Path,
) {
    if value.starts_with('.') || value.starts_with('/') {
        if let Some(path) = config_parser::normalize_config_path(value, config_path, root) {
            result.always_used_files.push(path);
        }
    } else if config_parser::is_package_specifier(value) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_module_prefixes_includes_astro_builtins() {
        let plugin = AstroPlugin;
        let prefixes = plugin.virtual_module_prefixes();
        assert!(prefixes.contains(&"astro:"));
    }

    fn resolve(source: &str) -> PluginResult {
        let root = Path::new("/project");
        AstroPlugin.resolve_config(&root.join("astro.config.mjs"), source, root)
    }

    #[test]
    fn starlight_components_and_custom_css_are_credited() {
        let result = resolve(
            r#"
            import { defineConfig } from "astro/config";
            import { default as docs } from "@astrojs/starlight";
            export default defineConfig({
                integrations: [
                    docs({
                        components: { Footer: "./src/overrides/Footer.astro" },
                        customCss: ["./src/theme.css", "@fontsource/inter/400.css"],
                    }),
                ],
            });
            "#,
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/overrides/Footer.astro".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/theme.css".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@fontsource/inter".to_string())
        );
    }

    #[test]
    fn options_of_other_integrations_are_not_credited() {
        let result = resolve(
            r#"
            import other from "some-integration";
            export default {
                integrations: [other({ customCss: ["./src/theme.css"] })],
            };
            "#,
        );
        assert!(result.always_used_files.is_empty());
    }

    #[test]
    fn used_exports_cover_current_astro_conventions() {
        let plugin = AstroPlugin;
        let exports = plugin.used_exports();

        assert!(exports.iter().any(|(pattern, names)| {
            pattern.contains("src/actions/index") && names.contains(&"server")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern.contains("src/content/config") && names.contains(&"collections")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern.contains("src/middleware/index") && names.contains(&"onRequest")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern.contains("src/pages") && names.contains(&"getStaticPaths")
        }));
    }
}
