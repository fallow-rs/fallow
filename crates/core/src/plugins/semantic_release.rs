//! semantic-release plugin.
//!
//! Detects semantic-release projects and marks config files as always used.
//! Parses config to extract plugin references as dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["semantic-release"];

// The YAML forms and bare `.releaserc` (JSON or YAML, with no extension to tell
// them apart) stay activation-only: the extractor is a JS/JSON parser.
const CONFIG_PATTERNS: &[&str] = &["release.config.{js,cjs,mjs}", ".releaserc.{js,cjs,json}"];

const ALWAYS_USED: &[&str] = &[
    "release.config.{js,cjs,mjs}",
    ".releaserc.{json,yaml,yml,js,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "semantic-release",
    "@semantic-release/commit-analyzer",
    "@semantic-release/release-notes-generator",
    "@semantic-release/changelog",
    "@semantic-release/npm",
    "@semantic-release/github",
    "@semantic-release/git",
];

define_plugin! {
    struct SemanticReleasePlugin => "semantic-release",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let plugins = config_parser::extract_config_shallow_strings(source, config_path, "plugins");
        for plugin in &plugins {
            let dep = crate::resolve::extract_package_name(plugin);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_plugins_shallow_strings() {
        let source = r#"
            module.exports = {
                plugins: [
                    "@semantic-release/commit-analyzer",
                    "@semantic-release/release-notes-generator",
                    "@semantic-release/npm",
                    "@semantic-release/github"
                ]
            };
        "#;
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.js"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@semantic-release/commit-analyzer".to_string()));
        assert!(deps.contains(&"@semantic-release/release-notes-generator".to_string()));
        assert!(deps.contains(&"@semantic-release/npm".to_string()));
        assert!(deps.contains(&"@semantic-release/github".to_string()));
    }

    #[test]
    fn resolve_config_plugins_with_options_skipped() {
        let source = r#"
            module.exports = {
                plugins: [
                    "@semantic-release/commit-analyzer",
                    ["@semantic-release/release-notes-generator", { preset: "angular" }],
                    "@semantic-release/npm"
                ]
            };
        "#;
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.js"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@semantic-release/commit-analyzer".to_string()));
        assert!(deps.contains(&"@semantic-release/npm".to_string()));
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { createConfig } from 'semantic-release';
            module.exports = {
                plugins: ["@semantic-release/npm"]
            };
        "#;
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.mjs"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"semantic-release".to_string()));
        assert!(deps.contains(&"@semantic-release/npm".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"module.exports = {};";
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_no_plugins() {
        let source = r#"
            module.exports = {
                branches: ["main", "next"]
            };
        "#;
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_scoped_plugin_name() {
        let source = r#"
            module.exports = {
                plugins: ["@semantic-release/git"]
            };
        "#;
        let plugin = SemanticReleasePlugin;
        let result = plugin.resolve_config(
            Path::new("release.config.cjs"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@semantic-release/git".to_string())
        );
    }

    #[test]
    fn resolve_config_json_releaserc_credits_plugin_outside_name_prefix() {
        let source = r#"{
            "branches": ["main"],
            "plugins": [
                ["@semantic-release/commit-analyzer", { "preset": "conventionalcommits" }],
                "conventional-changelog-conventionalcommits"
            ]
        }"#;
        let plugin = SemanticReleasePlugin;
        let result =
            plugin.resolve_config(Path::new(".releaserc.json"), source, Path::new("/project"));
        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"conventional-changelog-conventionalcommits".to_string()),
            "a plugin outside the `semantic-release` name prefix is not covered by the \
             tooling.toml prefix exemption, so only config extraction can credit it: {deps:?}"
        );
    }

    #[test]
    fn config_patterns_cover_json_releaserc_but_not_yaml() {
        let patterns = SemanticReleasePlugin.config_patterns();
        assert!(patterns.contains(&".releaserc.{js,cjs,json}"));
        assert!(
            !patterns
                .iter()
                .any(|p| p.contains("yaml") || p.contains("yml")),
            "the extractor is a JS/JSON parser; the YAML forms stay activation-only"
        );
    }
}
