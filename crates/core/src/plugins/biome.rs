//! Biome linter/formatter plugin.
//!
//! Detects Biome projects, marks config files as always used, and credits the
//! shared config packages that `extends` names.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@biomejs/biome"];

const CONFIG_PATTERNS: &[&str] = &["biome.json", "biome.jsonc"];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

const TOOLING_DEPENDENCIES: &[&str] = &["@biomejs/biome"];

/// In Biome 2, `"extends": "//"` makes a nested config inherit the root
/// config. It names no package.
const ROOT_CONFIG_MARKER: &str = "//";

define_plugin! {
    struct BiomePlugin => "biome",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        for entry in config_parser::extract_config_string_or_array(source, config_path, &["extends"])
        {
            if is_package_extends_entry(&entry) {
                result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(&entry));
            }
        }
        result
    }
}

/// Biome reads an `extends` entry as a file path when it starts with `.` or
/// `/`, or when it ends in `.json` or `.jsonc`. Biome resolves each other
/// entry as a package, for example `shared-biome/presets/core`.
fn is_package_extends_entry(entry: &str) -> bool {
    !entry.is_empty()
        && entry != ROOT_CONFIG_MARKER
        && !entry.starts_with('.')
        && !entry.starts_with('/')
        && !entry.contains('\\')
        && !std::path::Path::new(entry).extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("jsonc")
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn deps(config_name: &str, source: &str) -> Vec<String> {
        BiomePlugin
            .resolve_config(Path::new(config_name), source, Path::new("/project"))
            .referenced_dependencies
    }

    #[test]
    fn string_extends_credits_the_package() {
        assert_eq!(
            deps("biome.json", r#"{"extends": "shared-biome"}"#),
            ["shared-biome"]
        );
    }

    #[test]
    fn array_extends_credits_each_package() {
        let source = r#"{"extends": ["shared-biome", "@acme/biome-config"]}"#;
        assert_eq!(
            deps("biome.json", source),
            ["shared-biome", "@acme/biome-config"]
        );
    }

    #[test]
    fn package_subpath_credits_the_package_name() {
        let source = r#"{"extends": ["@acme/cfg/biome", "shared-biome/presets/core"]}"#;
        assert_eq!(deps("biome.json", source), ["@acme/cfg", "shared-biome"]);
    }

    #[test]
    fn file_paths_and_root_marker_credit_no_package() {
        let source = r#"{"extends": ["./base.json", "../shared/biome.jsonc", "/abs/biome.json", "common.json", "//"]}"#;
        assert!(deps("biome.json", source).is_empty());
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_parse() {
        let source = "// shared\n{\n  /* block */\n  \"extends\": [\"shared-biome\",],\n}";
        assert_eq!(deps("biome.jsonc", source), ["shared-biome"]);
    }

    #[test]
    fn config_without_extends_credits_nothing() {
        assert!(deps("biome.json", r#"{"formatter": {"enabled": true}}"#).is_empty());
    }
}
