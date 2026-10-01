//! Gatsby framework plugin.
//!
//! Detects Gatsby projects and marks pages, templates, and config files
//! as entry points. Parses gatsby-config to extract plugin dependencies.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["gatsby"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/pages/**/*.{ts,tsx,js,jsx}",
    "src/templates/**/*.{ts,tsx,js,jsx}",
    "src/api/**/*.{ts,js}",
];

const CONFIG_PATTERNS: &[&str] = &[
    "gatsby-config.{ts,js,mjs}",
    "gatsby-node.{ts,js,mjs}",
    "gatsby-browser.{ts,tsx,js,jsx}",
    "gatsby-ssr.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &[
    "gatsby-config.{ts,js,mjs}",
    "gatsby-node.{ts,js,mjs}",
    "gatsby-browser.{ts,tsx,js,jsx}",
    "gatsby-ssr.{ts,tsx,js,jsx}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["gatsby", "gatsby-cli"];

const PAGE_EXPORTS: &[&str] = &["default", "Head", "query", "config", "getServerData"];
const FUNCTION_EXPORTS: &[&str] = &["default", "config"];

define_plugin! {
    struct GatsbyPlugin => "gatsby",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    used_exports: [
        ("src/pages/**/*.{ts,tsx,js,jsx}", PAGE_EXPORTS),
        ("src/templates/**/*.{ts,tsx,js,jsx}", PAGE_EXPORTS),
        ("src/api/**/*.{ts,js}", FUNCTION_EXPORTS),
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let plugins = config_parser::extract_config_shallow_strings(source, config_path, "plugins");
        for plugin in &plugins {
            let dep = crate::resolve::extract_package_name(plugin);
            result.referenced_dependencies.push(dep);
        }

        let require_deps =
            config_parser::extract_config_require_strings(source, config_path, "plugins");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        extract_gatsby_plugin_resolves(source, config_path, &mut result);

        result
    }
}

/// Extract `resolve` string values from Gatsby plugin objects in the plugins array.
///
/// Handles: `plugins: [{ resolve: "gatsby-plugin-x", options: {} }]`
fn extract_gatsby_plugin_resolves(source: &str, path: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object(&parsed.program) else {
        return;
    };

    let Some(plugins_prop) = obj.properties.iter().find_map(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_match = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "plugins",
                PropertyKey::StringLiteral(s) => s.value == "plugins",
                _ => false,
            };
            if is_match {
                return Some(p);
            }
        }
        None
    }) else {
        return;
    };

    let Expression::ArrayExpression(arr) = &plugins_prop.value else {
        return;
    };

    for el in &arr.elements {
        if let Some(Expression::ObjectExpression(plugin_obj)) = el.as_expression() {
            for prop in &plugin_obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let is_resolve = match &p.key {
                        PropertyKey::StaticIdentifier(id) => id.name == "resolve",
                        PropertyKey::StringLiteral(s) => s.value == "resolve",
                        _ => false,
                    };
                    if is_resolve && let Expression::StringLiteral(s) = &p.value {
                        let dep = crate::resolve::extract_package_name(&s.value);
                        result.referenced_dependencies.push(dep);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_string_plugins() {
        let source = r#"
            module.exports = {
                plugins: ["gatsby-plugin-image", "gatsby-plugin-sharp"]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"gatsby-plugin-image".to_string()));
        assert!(deps.contains(&"gatsby-plugin-sharp".to_string()));
    }

    #[test]
    fn resolve_config_object_plugins() {
        let source = r#"
            module.exports = {
                plugins: [
                    {
                        resolve: "gatsby-source-filesystem",
                        options: { name: "images", path: "./src/images" }
                    },
                    "gatsby-plugin-sharp"
                ]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"gatsby-source-filesystem".to_string()));
        assert!(deps.contains(&"gatsby-plugin-sharp".to_string()));
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import type { GatsbyConfig } from "gatsby";
            export default {
                plugins: ["gatsby-plugin-postcss"]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"gatsby".to_string()));
        assert!(deps.contains(&"gatsby-plugin-postcss".to_string()));
    }

    #[test]
    fn resolve_config_require_plugins() {
        let source = r#"
            module.exports = {
                plugins: [require("gatsby-plugin-mdx")]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"gatsby-plugin-mdx".to_string())
        );
    }

    #[test]
    fn resolve_config_empty_no_plugins_property() {
        let source = r#"
            module.exports = {
                siteMetadata: { title: "My Site" }
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.referenced_dependencies, [] as [String; 0]);
    }

    #[test]
    fn resolve_config_empty_object() {
        let source = r"module.exports = {};";
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.referenced_dependencies, [] as [String; 0]);
    }

    #[test]
    fn resolve_config_no_config_object() {
        let source = r"
            const x = 42;
        ";
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.referenced_dependencies, [] as [String; 0]);
    }

    #[test]
    fn resolve_config_plugins_not_array() {
        let source = r#"
            module.exports = {
                plugins: "not-an-array"
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"not-an-array".to_string())
        );
    }

    #[test]
    fn resolve_config_object_plugin_with_string_literal_keys() {
        let source = r#"
            module.exports = {
                "plugins": [
                    {
                        "resolve": "gatsby-plugin-feed",
                        "options": {}
                    }
                ]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"gatsby-plugin-feed".to_string())
        );
    }

    #[test]
    fn resolve_config_plugin_object_non_resolve_properties_ignored() {
        let source = r#"
            module.exports = {
                plugins: [
                    {
                        resolve: "gatsby-plugin-manifest",
                        options: { name: "My App", icon: "src/images/icon.png" }
                    }
                ]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"gatsby-plugin-manifest".to_string()));
        assert!(!deps.iter().any(|d| d.contains("My App")));
    }

    #[test]
    fn resolve_config_scoped_package_extraction() {
        let source = r#"
            module.exports = {
                plugins: [
                    {
                        resolve: "@scope/gatsby-plugin-analytics/nested",
                        options: {}
                    }
                ]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@scope/gatsby-plugin-analytics".to_string())
        );
    }

    #[test]
    fn resolve_config_mixed_string_and_object_plugins() {
        let source = r#"
            import sharp from "sharp";
            module.exports = {
                plugins: [
                    "gatsby-plugin-image",
                    {
                        resolve: "gatsby-source-filesystem",
                        options: { name: "pages", path: "./src/pages" }
                    },
                    "gatsby-transformer-sharp",
                    {
                        resolve: "gatsby-plugin-manifest",
                        options: { name: "App" }
                    }
                ]
            };
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"sharp".to_string()));
        assert!(deps.contains(&"gatsby-plugin-image".to_string()));
        assert!(deps.contains(&"gatsby-transformer-sharp".to_string()));
        assert!(deps.contains(&"gatsby-source-filesystem".to_string()));
        assert!(deps.contains(&"gatsby-plugin-manifest".to_string()));
    }

    #[test]
    fn trait_accessors() {
        let plugin = GatsbyPlugin;
        assert_eq!(plugin.name(), "gatsby");
        assert_eq!(plugin.enablers(), &["gatsby"]);
        assert_ne!(plugin.entry_patterns(), [] as [&str; 0]);
        assert_ne!(plugin.config_patterns(), [] as [&str; 0]);
        assert_ne!(plugin.always_used(), [] as [&str; 0]);
        assert_eq!(plugin.tooling_dependencies(), &["gatsby", "gatsby-cli"]);
    }

    #[test]
    fn used_exports_covers_pages_and_templates() {
        let plugin = GatsbyPlugin;
        let exports = plugin.used_exports();
        assert_eq!(exports.len(), 3);
        let (pages_pattern, pages_exports) = &exports[0];
        assert!(pages_pattern.contains("src/pages"));
        assert!(pages_exports.contains(&"default"));
        assert!(pages_exports.contains(&"Head"));
        assert!(pages_exports.contains(&"query"));
        assert!(pages_exports.contains(&"config"));
        assert!(pages_exports.contains(&"getServerData"));

        let (templates_pattern, templates_exports) = &exports[1];
        assert!(templates_pattern.contains("src/templates"));
        assert_eq!(pages_exports, templates_exports);

        let (functions_pattern, function_exports) = &exports[2];
        assert!(functions_pattern.contains("src/api"));
        assert_eq!(function_exports, &FUNCTION_EXPORTS);
    }

    #[test]
    fn resolve_config_ts_with_typed_variable() {
        let source = r#"
            import type { GatsbyConfig } from "gatsby";

            const config: GatsbyConfig = {
                plugins: [
                    "gatsby-plugin-postcss",
                    {
                        resolve: "gatsby-source-contentful",
                        options: { spaceId: "abc" }
                    }
                ]
            };
            export default config;
        "#;
        let plugin = GatsbyPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("gatsby-config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"gatsby".to_string()));
        assert!(deps.contains(&"gatsby-plugin-postcss".to_string()));
        assert!(deps.contains(&"gatsby-source-contentful".to_string()));
    }
}
