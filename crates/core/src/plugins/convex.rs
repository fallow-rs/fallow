//! Convex backend platform plugin.
//!
//! Detects Convex projects and marks all files in the convex/ directory as
//! entry points, since Convex deploys every exported function.

use std::path::Path;

use super::config_parser::{extract_config_string, normalize_config_path};
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["convex"];
const CONFIG_PATTERNS: &[&str] = &["convex.json"];

const ENTRY_PATTERNS: &[&str] = &["convex/**/*.{ts,js}"];

const ALWAYS_USED: &[&str] = &[
    "convex/_generated/**/*",
    "convex/schema.{ts,js}",
    "convex/auth.config.{ts,js}",
    "convex/auth.{ts,js}",
    "convex/http.{ts,js}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["convex"];

pub struct ConvexPlugin;

impl Plugin for ConvexPlugin {
    fn name(&self) -> &'static str {
        "convex"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let Some(functions_dir) = extract_config_string(source, config_path, &["functions"])
            .as_deref()
            .and_then(|raw| normalize_config_path(raw, config_path, root))
        else {
            return PluginResult::default();
        };

        let mut result = PluginResult {
            replace_entry_patterns: true,
            ..PluginResult::default()
        };
        result.push_entry_pattern(format!("{functions_dir}/**/*.{{ts,js}}"));
        result.always_used_files.extend(
            [
                "_generated/**/*",
                "schema.{ts,js}",
                "auth.config.{ts,js}",
                "auth.{ts,js}",
                "http.{ts,js}",
            ]
            .map(|suffix| format!("{functions_dir}/{suffix}")),
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_patterns_cover_convex_directory() {
        let plugin = ConvexPlugin;
        assert!(plugin.entry_patterns().contains(&"convex/**/*.{ts,js}"));
    }

    #[test]
    fn always_used_protects_generated_files() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/_generated/**/*"));
    }

    #[test]
    fn always_used_protects_schema() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/schema.{ts,js}"));
    }

    #[test]
    fn always_used_protects_auth_config() {
        let plugin = ConvexPlugin;
        let used = plugin.always_used();
        assert!(used.contains(&"convex/auth.config.{ts,js}"));
        assert!(used.contains(&"convex/auth.{ts,js}"));
    }

    #[test]
    fn always_used_protects_http_router() {
        let plugin = ConvexPlugin;
        assert!(plugin.always_used().contains(&"convex/http.{ts,js}"));
    }

    #[test]
    fn enabled_with_convex_dep() {
        let plugin = ConvexPlugin;
        let deps = vec!["convex".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, std::path::Path::new("/project")));
    }

    #[test]
    fn not_enabled_without_convex_dep() {
        let plugin = ConvexPlugin;
        let deps = vec!["firebase".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, std::path::Path::new("/project")));
    }

    #[test]
    fn tooling_dependencies_include_convex() {
        let plugin = ConvexPlugin;
        assert!(plugin.tooling_dependencies().contains(&"convex"));
    }

    #[test]
    fn custom_functions_directory_replaces_default_entry_root() {
        let plugin = ConvexPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/convex.json"),
            r#"{ "functions": "src/convex/", }"#,
            std::path::Path::new("/project"),
        );

        assert!(plugin.config_patterns().contains(&"convex.json"));
        assert!(result.replace_entry_patterns);
        assert_eq!(result.entry_patterns, vec!["src/convex/**/*.{ts,js}"]);
        assert!(
            result
                .always_used_files
                .contains(&"src/convex/_generated/**/*".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/convex/http.{ts,js}".to_string())
        );
    }

    #[test]
    fn custom_functions_directory_is_relative_to_nested_config() {
        let result = ConvexPlugin.resolve_config(
            std::path::Path::new("/repo/apps/web/convex.json"),
            r#"{ "functions": "src/convex" }"#,
            std::path::Path::new("/repo"),
        );

        assert_eq!(
            result.entry_patterns,
            vec!["apps/web/src/convex/**/*.{ts,js}"]
        );
        assert!(
            result
                .always_used_files
                .contains(&"apps/web/src/convex/_generated/**/*".to_string())
        );
    }

    #[test]
    fn invalid_custom_functions_directory_keeps_static_defaults() {
        for source in [
            r#"{ "functions": "" }"#,
            r#"{ "functions": "../../outside" }"#,
            r#"{ "functions": 42 }"#,
            "{",
        ] {
            let result = ConvexPlugin.resolve_config(
                std::path::Path::new("/project/convex.json"),
                source,
                std::path::Path::new("/project"),
            );

            assert!(!result.replace_entry_patterns, "source: {source}");
            assert!(result.entry_patterns.is_empty(), "source: {source}");
            assert!(result.always_used_files.is_empty(), "source: {source}");
        }
    }
}
