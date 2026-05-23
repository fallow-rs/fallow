//! Stryker JS mutation testing plugin.
//!
//! Stryker loads its config files from the CLI. This keeps default config files
//! alive and credits statically named runner, checker, and plugin dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@stryker-mutator/core", "stryker"];

const CONFIG_PATTERNS: &[&str] = &[
    "stryker.conf.{json,js,mjs,cjs,jsonc,ts}",
    "**/stryker.conf.{json,js,mjs,cjs,jsonc,ts}",
    ".stryker.conf.{json,js,mjs,cjs,jsonc,ts}",
    "**/.stryker.conf.{json,js,mjs,cjs,jsonc,ts}",
    "stryker.config.{json,js,mjs,cjs,jsonc,ts}",
    "**/stryker.config.{json,js,mjs,cjs,jsonc,ts}",
    ".stryker.config.{json,js,mjs,cjs,jsonc,ts}",
    "**/.stryker.config.{json,js,mjs,cjs,jsonc,ts}",
];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

const TOOLING_DEPENDENCIES: &[&str] = &["@stryker-mutator/core", "stryker"];

define_plugin! {
    struct StrykerPlugin => "stryker",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        for specifier in config_parser::extract_imports_and_requires(source, config_path) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&specifier));
        }

        for key in ["plugins", "appendPlugins"] {
            for plugin in config_parser::extract_config_shallow_strings(source, config_path, key) {
                if is_explicit_package_name(&plugin) {
                    result
                        .referenced_dependencies
                        .push(crate::resolve::extract_package_name(&plugin));
                }
            }
        }

        for checker in config_parser::extract_config_shallow_strings(source, config_path, "checkers") {
            if checker == "typescript" {
                result
                    .referenced_dependencies
                    .push("@stryker-mutator/typescript-checker".to_string());
            } else if is_explicit_package_name(&checker) {
                result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(&checker));
            }
        }

        if let Some(test_runner) =
            config_parser::extract_config_string(source, config_path, &["testRunner"])
            && let Some(dep) = runner_package_for(&test_runner)
        {
            result.referenced_dependencies.push(dep.to_string());
        }

        result
    }
}

fn is_explicit_package_name(value: &str) -> bool {
    !value.starts_with('.')
        && !value.starts_with('/')
        && (value.starts_with('@') || value.contains('/') || value.contains('-'))
}

fn runner_package_for(value: &str) -> Option<&'static str> {
    match value {
        "jasmine" => Some("@stryker-mutator/jasmine-runner"),
        "jest" => Some("@stryker-mutator/jest-runner"),
        "karma" => Some("@stryker-mutator/karma-runner"),
        "mocha" => Some("@stryker-mutator/mocha-runner"),
        "vitest" => Some("@stryker-mutator/vitest-runner"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_extracts_imports_and_requires() {
        let source = r#"
            require("@stryker-mutator/dashboard-reporter");
            import "@stryker-mutator/html-reporter";
            module.exports = { testRunner: "command" };
        "#;
        let plugin = StrykerPlugin;
        let result =
            plugin.resolve_config(Path::new("stryker.conf.cjs"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&"@stryker-mutator/dashboard-reporter".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@stryker-mutator/html-reporter".to_string())
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"@stryker-mutator/command-runner".to_string())
        );
    }

    #[test]
    fn resolve_config_maps_known_runner_and_checker_short_names() {
        let source = r#"{
            "testRunner": "mocha",
            "checkers": ["typescript"]
        }"#;
        let plugin = StrykerPlugin;
        let result = plugin.resolve_config(
            Path::new("stryker.conf.json"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@stryker-mutator/mocha-runner".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@stryker-mutator/typescript-checker".to_string())
        );
    }

    #[test]
    fn resolve_config_credits_package_like_plugins_only() {
        let source = r#"export default {
            plugins: ["@stryker-mutator/jest-runner", "dashboard", "custom-stryker-plugin"],
            appendPlugins: ["@org/stryker-plugin"]
        };"#;
        let plugin = StrykerPlugin;
        let result = plugin.resolve_config(
            Path::new("stryker.config.mjs"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@stryker-mutator/jest-runner".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"custom-stryker-plugin".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@org/stryker-plugin".to_string())
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"dashboard".to_string())
        );
    }

    #[test]
    fn resolve_config_does_not_credit_relative_or_absolute_plugin_paths() {
        let source = r#"export default {
            plugins: ["./local-plugin.js", "/absolute/plugin.js", "../parent/plugin.js"]
        };"#;
        let plugin = StrykerPlugin;
        let result = plugin.resolve_config(
            Path::new("stryker.config.mjs"),
            source,
            Path::new("/project"),
        );

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn always_used_includes_documented_names_and_fallow_extensions() {
        let plugin = StrykerPlugin;
        let patterns = plugin.always_used();
        assert!(patterns.contains(&"stryker.conf.{json,js,mjs,cjs,jsonc,ts}"));
        assert!(patterns.contains(&"**/stryker.conf.{json,js,mjs,cjs,jsonc,ts}"));
        assert!(patterns.contains(&".stryker.config.{json,js,mjs,cjs,jsonc,ts}"));
        assert!(patterns.contains(&"**/.stryker.config.{json,js,mjs,cjs,jsonc,ts}"));
    }
}
