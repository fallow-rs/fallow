//! OpenCode plugin.
//!
//! OpenCode loads project-local plugins from `.opencode/plugins/` and npm
//! plugins declared in `opencode.json`, so those surfaces need to be credited
//! even when application source never imports them.

use std::path::Path;

use serde_json::Value;

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@opencode-ai/"];
const CONFIG_PATTERNS: &[&str] = &["opencode.json"];
const ENTRY_PATTERNS: &[&str] = &[".opencode/plugins/**/*.{js,ts,mjs,cjs,mts,cts}"];
const ALWAYS_USED: &[&str] = &[
    "opencode.json",
    ".opencode/package.json",
    ".opencode/plugins/**/*.{js,ts,mjs,cjs,mts,cts}",
];
const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".opencode"];
const TOOLING_DEPENDENCIES: &[&str] = &["@opencode-ai/plugin"];

pub struct OpenCodePlugin;

impl Plugin for OpenCodePlugin {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep.starts_with(enabler)))
            || root.join("opencode.json").is_file()
            || root.join(".opencode").is_dir()
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

    fn discovery_hidden_dirs(&self) -> &'static [&'static str] {
        DISCOVERY_HIDDEN_DIRS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, _config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        PluginResult {
            referenced_dependencies: extract_opencode_plugin_dependencies(source),
            ..PluginResult::default()
        }
    }
}

fn extract_opencode_plugin_dependencies(source: &str) -> Vec<String> {
    let Ok(config) = serde_json::from_str::<Value>(source) else {
        return Vec::new();
    };
    let Some(plugins) = config.get("plugin").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut deps: Vec<String> = plugins
        .iter()
        .filter_map(plugin_specifier)
        .filter_map(package_name_for_specifier)
        .filter(|package_name| !package_name.is_empty())
        .collect();
    deps.sort();
    deps.dedup();
    deps
}

fn plugin_specifier(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.as_array()?.first()?.as_str())
}

fn package_name_for_specifier(specifier: &str) -> Option<String> {
    let specifier = specifier.trim();
    is_package_specifier(specifier).then(|| crate::resolve::extract_package_name(specifier))
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && specifier != "."
        && specifier != ".."
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !specifier.contains(':')
        && !specifier.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_from_opencode_dependency_prefix() {
        let plugin = OpenCodePlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["@opencode-ai/plugin".to_string()], tmp.path()));
        assert!(plugin.is_enabled_with_deps(&["@opencode-ai/sdk".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["@opencodeish/plugin".to_string()], tmp.path()));
    }

    #[test]
    fn activates_from_project_config_or_directory() {
        let plugin = OpenCodePlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(!plugin.is_enabled_with_deps(&[], tmp.path()));

        std::fs::write(tmp.path().join("opencode.json"), "{}\n").expect("opencode config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));

        std::fs::remove_file(tmp.path().join("opencode.json")).expect("remove opencode config");
        std::fs::create_dir(tmp.path().join(".opencode")).expect("opencode dir");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn exposes_static_opencode_conventions() {
        let plugin = OpenCodePlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert_eq!(plugin.entry_patterns(), ENTRY_PATTERNS);
        assert_eq!(plugin.discovery_hidden_dirs(), DISCOVERY_HIDDEN_DIRS);
        assert_eq!(plugin.tooling_dependencies(), TOOLING_DEPENDENCIES);
        assert!(
            plugin
                .always_used()
                .contains(&".opencode/plugins/**/*.{js,ts,mjs,cjs,mts,cts}")
        );
    }

    #[test]
    fn resolve_config_credits_string_tuple_scoped_and_subpath_plugins() {
        let source = r#"{
            "plugin": [
                "opencode-wakatime",
                "@scope/opencode-plugin",
                ["@acme/opencode-plugin/subpath", { "enabled": true }]
            ]
        }"#;
        let plugin = OpenCodePlugin;
        let result = plugin.resolve_config(Path::new("opencode.json"), source, Path::new("/repo"));

        assert_eq!(
            result.referenced_dependencies,
            vec![
                "@acme/opencode-plugin".to_string(),
                "@scope/opencode-plugin".to_string(),
                "opencode-wakatime".to_string()
            ]
        );
    }

    #[test]
    fn resolve_config_ignores_local_protocol_backslash_empty_and_non_plugin_values() {
        let source = r#"{
            "plugin": [
                "",
                "./plugins/local.js",
                "../shared/plugin.js",
                "/absolute/plugin.js",
                "https://example.com/plugin.js",
                "file:./plugin.js",
                "github:owner/plugin",
                "bad\\path",
                " . ",
                " .. ",
                { "name": "not-supported" },
                ["", {}],
                ["./local.js", {}],
                [" opencode-valid/subpath ", {}]
            ]
        }"#;
        let plugin = OpenCodePlugin;
        let result = plugin.resolve_config(Path::new("opencode.json"), source, Path::new("/repo"));

        assert_eq!(
            result.referenced_dependencies,
            vec!["opencode-valid".to_string()]
        );
    }

    #[test]
    fn resolve_config_dedups_and_gracefully_ignores_malformed_json() {
        let plugin = OpenCodePlugin;
        let result = plugin.resolve_config(
            Path::new("opencode.json"),
            r#"{ "plugin": ["opencode-wakatime", "opencode-wakatime"] }"#,
            Path::new("/repo"),
        );
        assert_eq!(
            result.referenced_dependencies,
            vec!["opencode-wakatime".to_string()]
        );

        let malformed = plugin.resolve_config(Path::new("opencode.json"), "{", Path::new("/repo"));
        assert!(malformed.referenced_dependencies.is_empty());
    }
}
