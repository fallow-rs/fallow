//! `PostCSS` plugin.
//!
//! Detects `PostCSS` projects and marks config files as always used.
//! Parses config to extract plugin dependencies from object keys, `require()` calls,
//! and string array forms.
//!
//! A bundler such as Next.js loads `postcss.config.*` with its own copy of
//! `PostCSS`, so a project can have the config file without a declared `postcss`
//! package. The config file alone also activates the plugin.

use std::path::Path;

use super::config_parser;
use super::registry::find_config_file;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["postcss"];

const CONFIG_PATTERNS: &[&str] = &["postcss.config.{ts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["postcss.config.{ts,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["postcss", "postcss-cli"];

/// Built-in plugin for `PostCSS` configs.
pub struct PostCssPlugin;

impl Plugin for PostCssPlugin {
    fn name(&self) -> &'static str {
        "postcss"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep == enabler))
            || find_config_file(CONFIG_PATTERNS.iter().copied(), &[root]).is_some()
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

    fn resolve_config(&self, config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let plugin_keys =
            config_parser::extract_config_object_keys(source, config_path, &["plugins"]);
        for key in &plugin_keys {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(key));
        }

        let require_deps =
            config_parser::extract_config_require_strings(source, config_path, "plugins");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let plugin_strings =
            config_parser::extract_config_shallow_strings(source, config_path, "plugins");
        for plugin in &plugin_strings {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(plugin));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_string_array_plugins() {
        let source = r#"
            module.exports = {
                plugins: ["autoprefixer", ["postcss-preset-env", { stage: 3 }]]
            };
        "#;
        let plugin = PostCssPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("postcss.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"autoprefixer".to_string()));
        assert!(deps.contains(&"postcss-preset-env".to_string()));
    }

    #[test]
    fn resolve_config_object_and_require() {
        let source = r"
            module.exports = {
                plugins: {
                    autoprefixer: {},
                    tailwindcss: {}
                }
            };
        ";
        let plugin = PostCssPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("postcss.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"autoprefixer".to_string()));
        assert!(deps.contains(&"tailwindcss".to_string()));
    }

    #[test]
    fn config_file_enables_plugin_without_postcss_dependency() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plugin = PostCssPlugin;
        assert!(!plugin.is_enabled_with_deps(&["next".to_string()], dir.path()));
        assert!(plugin.is_enabled_with_deps(&["postcss".to_string()], dir.path()));

        std::fs::write(
            dir.path().join("postcss.config.mjs"),
            "export default {};\n",
        )
        .expect("write");
        assert!(plugin.is_enabled_with_deps(&["next".to_string()], dir.path()));
    }
}
