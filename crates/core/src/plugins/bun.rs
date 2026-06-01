//! Bun plugin.
//!
//! Detects Bun runtime projects and marks config files as always used.
//! Parses `bunfig.toml` to seed `[test] preload` and top-level `preload`
//! entries as support entry points so they are not reported unused.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["bun-types", "@types/bun"];

const CONFIG_PATTERNS: &[&str] = &["bunfig.toml"];

const ALWAYS_USED: &[&str] = &["bunfig.toml"];

const TOOLING_DEPENDENCIES: &[&str] = &["bun-types", "@types/bun"];

define_plugin! {
    struct BunPlugin => "bun",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        for path in extract_preload_entries(config_path, source, root) {
            result.push_entry_pattern(path);
        }
        result
    },
}

fn extract_preload_entries(config_path: &Path, source: &str, root: &Path) -> Vec<String> {
    let Ok(value) = source.parse::<toml::Table>() else {
        return Vec::new();
    };

    let mut entries = Vec::new();

    // Top-level `preload = [...]`
    if let Some(arr) = value.get("preload").and_then(toml::Value::as_array) {
        for item in arr {
            if let Some(raw) = item.as_str()
                && let Some(path) = config_parser::normalize_config_path(raw, config_path, root)
            {
                entries.push(path);
            }
        }
    }

    // `[test] preload = [...]`
    if let Some(test) = value.get("test").and_then(toml::Value::as_table)
        && let Some(arr) = test.get("preload").and_then(toml::Value::as_array)
    {
        for item in arr {
            if let Some(raw) = item.as_str()
                && let Some(path) = config_parser::normalize_config_path(raw, config_path, root)
            {
                entries.push(path);
            }
        }
    }

    entries.sort();
    entries.dedup();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_bun_activates_plugin() {
        // @types/bun should be sufficient to enable the bun plugin
        let plugin = BunPlugin;
        assert!(
            plugin.enablers().contains(&"@types/bun"),
            "enablers must include @types/bun, got: {:?}",
            plugin.enablers()
        );
    }

    #[test]
    fn bun_types_still_activates_plugin() {
        let plugin = BunPlugin;
        assert!(
            plugin.enablers().contains(&"bun-types"),
            "enablers must still include bun-types"
        );
    }

    #[test]
    fn test_section_preload_entries_are_entry_patterns() {
        let plugin = BunPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/bunfig.toml"),
            r#"
[test]
preload = ["./src/test-preload.ts"]
"#,
            Path::new("/repo"),
        );

        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|e| e.pattern.as_str())
            .collect();

        assert!(
            entries.contains(&"src/test-preload.ts"),
            "test preload must be an entry pattern, got: {entries:?}"
        );
    }

    #[test]
    fn top_level_preload_entries_are_entry_patterns() {
        let plugin = BunPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/bunfig.toml"),
            r#"
preload = ["./src/setup.ts", "./src/polyfills.ts"]
"#,
            Path::new("/repo"),
        );

        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|e| e.pattern.as_str())
            .collect();

        assert!(
            entries.contains(&"src/setup.ts"),
            "top-level preload must be an entry pattern, got: {entries:?}"
        );
        assert!(
            entries.contains(&"src/polyfills.ts"),
            "all top-level preload entries must be seeded, got: {entries:?}"
        );
    }

    #[test]
    fn empty_bunfig_produces_no_entry_patterns() {
        let plugin = BunPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/bunfig.toml"),
            r"
[install]
exact = true
",
            Path::new("/repo"),
        );

        assert!(
            result.entry_patterns.is_empty(),
            "no preload sections must yield no entry patterns, got: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn both_preload_sections_are_combined() {
        let plugin = BunPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/bunfig.toml"),
            r#"
preload = ["./src/global-setup.ts"]

[test]
preload = ["./src/test-setup.ts"]
"#,
            Path::new("/repo"),
        );

        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|e| e.pattern.as_str())
            .collect();

        assert!(
            entries.contains(&"src/global-setup.ts"),
            "top-level preload must be present, got: {entries:?}"
        );
        assert!(
            entries.contains(&"src/test-setup.ts"),
            "test preload must be present, got: {entries:?}"
        );
    }

    #[test]
    fn invalid_toml_produces_no_entry_patterns() {
        let plugin = BunPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/bunfig.toml"),
            "not valid [[[ toml",
            Path::new("/repo"),
        );

        assert!(
            result.entry_patterns.is_empty(),
            "invalid TOML must produce no entry patterns"
        );
    }

    #[test]
    fn config_patterns_includes_bunfig_toml() {
        let plugin = BunPlugin;
        assert!(
            plugin.config_patterns().contains(&"bunfig.toml"),
            "bunfig.toml must be a config pattern to trigger resolve_config"
        );
    }
}
