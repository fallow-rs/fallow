//! Babel transpiler plugin.
//!
//! Detects Babel projects and marks config files as always used.
//! Parses babel config to extract presets, plugins, and imports as referenced dependencies.
//! Supports Babel short name resolution for presets/plugins and JSON config files.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@babel/core"];

const CONFIG_PATTERNS: &[&str] = &[
    "babel.config.{js,cjs,mjs,ts,cts}",
    ".babelrc",
    ".babelrc.{js,cjs,mjs,json}",
];

const ALWAYS_USED: &[&str] = &[
    "babel.config.{js,cjs,mjs,ts,cts}",
    ".babelrc",
    ".babelrc.{js,cjs,mjs,json}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["@babel/core", "@babel/cli", "@babel/runtime"];

define_plugin! {
    struct BabelPlugin => "babel",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "babel",
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        // Handle JSON configs (.babelrc, .babelrc.json)
        let is_json = config_path.extension().is_some_and(|ext| ext == "json")
            || config_path
                .file_name()
                .is_some_and(|name| name == ".babelrc");
        let (parse_source, parse_path_buf) = if is_json {
            (format!("({source})"), config_path.with_extension("js"))
        } else {
            (source.to_string(), config_path.to_path_buf())
        };
        let parse_path: &std::path::Path = &parse_path_buf;

        let imports = config_parser::extract_imports(&parse_source, parse_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // presets -> referenced dependencies (shallow to avoid options objects)
        // Babel short name resolution: "env" -> "@babel/preset-env"
        let presets =
            config_parser::extract_config_shallow_strings(&parse_source, parse_path, "presets");
        for preset in &presets {
            result
                .referenced_dependencies
                .push(resolve_babel_preset_name(preset));
        }

        // plugins -> referenced dependencies (shallow to avoid options objects)
        // Babel short name resolution: "transform-runtime" -> "@babel/plugin-transform-runtime"
        let plugins =
            config_parser::extract_config_shallow_strings(&parse_source, parse_path, "plugins");
        for plugin in &plugins {
            result
                .referenced_dependencies
                .push(resolve_babel_plugin_name(plugin));
        }

        // extends -> referenced dependency or config file
        if let Some(extends) =
            config_parser::extract_config_string(&parse_source, parse_path, &["extends"])
        {
            let dep = crate::resolve::extract_package_name(&extends);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

/// Resolve Babel preset short name to full package name.
///
/// - `"env"` → `"babel-preset-env"` (third-party short name)
/// - `"@babel/env"` → `"@babel/preset-env"`
/// - `"@babel/preset-env"` → `"@babel/preset-env"` (already full)
/// - `"babel-preset-foo"` → `"babel-preset-foo"` (already full)
/// - `"module:my-preset"` → `"my-preset"` (module: prefix)
fn resolve_babel_preset_name(name: &str) -> String {
    let name = name.strip_prefix("module:").unwrap_or(name);

    if name.starts_with("babel-preset-") || name.contains("/preset-") {
        name.to_string()
    } else if let Some(rest) = name.strip_prefix("@babel/") {
        if rest.starts_with("preset-") {
            format!("@babel/{rest}")
        } else {
            format!("@babel/preset-{rest}")
        }
    } else if name.starts_with('@') {
        name.to_string()
    } else {
        format!("babel-preset-{name}")
    }
}

/// Resolve Babel plugin short name to full package name.
///
/// - `"transform-runtime"` → `"babel-plugin-transform-runtime"` (third-party short name)
/// - `"@babel/transform-runtime"` → `"@babel/plugin-transform-runtime"`
/// - `"@babel/plugin-transform-runtime"` → `"@babel/plugin-transform-runtime"` (already full)
/// - `"babel-plugin-foo"` → `"babel-plugin-foo"` (already full)
/// - `"module:my-plugin"` → `"my-plugin"` (module: prefix)
pub(super) fn resolve_babel_plugin_name(name: &str) -> String {
    let name = name.strip_prefix("module:").unwrap_or(name);

    if name.starts_with("babel-plugin-") || name.contains("/plugin-") {
        name.to_string()
    } else if let Some(rest) = name.strip_prefix("@babel/") {
        if rest.starts_with("plugin-") {
            format!("@babel/{rest}")
        } else {
            format!("@babel/plugin-{rest}")
        }
    } else if name.starts_with('@') {
        name.to_string()
    } else {
        format!("babel-plugin-{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Babel preset name resolution ────────────────────────────────

    #[test]
    fn preset_short_name() {
        assert_eq!(resolve_babel_preset_name("env"), "babel-preset-env");
    }

    #[test]
    fn preset_babel_scoped_short() {
        assert_eq!(resolve_babel_preset_name("@babel/env"), "@babel/preset-env");
    }

    #[test]
    fn preset_already_full() {
        assert_eq!(
            resolve_babel_preset_name("@babel/preset-env"),
            "@babel/preset-env"
        );
    }

    #[test]
    fn preset_module_prefix() {
        assert_eq!(
            resolve_babel_preset_name("module:my-preset"),
            "babel-preset-my-preset"
        );
    }

    // ── Babel plugin name resolution ────────────────────────────────

    #[test]
    fn plugin_short_name() {
        assert_eq!(
            resolve_babel_plugin_name("transform-runtime"),
            "babel-plugin-transform-runtime"
        );
    }

    #[test]
    fn plugin_babel_scoped_short() {
        assert_eq!(
            resolve_babel_plugin_name("@babel/transform-runtime"),
            "@babel/plugin-transform-runtime"
        );
    }

    #[test]
    fn plugin_already_full() {
        assert_eq!(
            resolve_babel_plugin_name("@babel/plugin-transform-runtime"),
            "@babel/plugin-transform-runtime"
        );
    }

    // ── resolve_config integration ──────────────────────────────────

    #[test]
    fn resolve_config_presets_and_plugins() {
        let source = r#"
            module.exports = {
                presets: ["@babel/env", ["@babel/react", { runtime: "automatic" }]],
                plugins: ["@babel/transform-runtime"]
            };
        "#;
        let plugin = BabelPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("babel.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@babel/preset-env".to_string()));
        assert!(deps.contains(&"@babel/preset-react".to_string()));
        assert!(deps.contains(&"@babel/plugin-transform-runtime".to_string()));
    }

    #[test]
    fn resolve_config_json_babelrc() {
        let source = r#"{"presets": ["@babel/env"], "plugins": ["@babel/transform-runtime"]}"#;
        let plugin = BabelPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".babelrc"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@babel/preset-env".to_string()));
        assert!(deps.contains(&"@babel/plugin-transform-runtime".to_string()));
    }

    #[test]
    fn resolve_config_extends() {
        let source = r#"module.exports = { extends: "./base.config.js" };"#;
        let plugin = BabelPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("babel.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(!result.referenced_dependencies.is_empty());
    }
}
