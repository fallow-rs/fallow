//! TypeScript declaration tests using tsd or expect-type.
//!
//! Detects tsd projects and marks declaration test files as entry points.

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["tsd", "expect-type"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.test-d.{ts,tsx}",
    "test-d/**/*.test-d.{ts,tsx}",
    "test-d/**/*.{ts,tsx}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["tsd"];

define_plugin! {
    struct TsdPlugin => "tsd",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "tsd",
    resolve_config(_config_path, source, _root) {
        let mut result = PluginResult::default();

        if let Some(directory) = extract_tsd_directory(source) {
            result.push_entry_pattern(format!("{directory}/**/*.test-d.{{ts,tsx}}"));
        }

        result
    },
}

fn extract_tsd_directory(source: &str) -> Option<String> {
    let config = serde_json::from_str::<serde_json::Value>(source).ok()?;
    let directory = config.get("directory")?.as_str()?.trim();
    let directory = directory
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string();

    (!directory.is_empty() && !directory.starts_with("../")).then_some(directory)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn is_enabled_with_deps() {
        let plugin = TsdPlugin;
        assert!(plugin.is_enabled_with_deps(&["tsd".to_string()], Path::new("/project")));
        assert!(plugin.is_enabled_with_deps(&["expect-type".to_string()], Path::new("/project")));
        assert!(!plugin.is_enabled_with_deps(&["vitest".to_string()], Path::new("/project")));
    }

    #[test]
    fn entry_patterns_include_tsd_defaults() {
        let plugin = TsdPlugin;
        let patterns = plugin.entry_patterns();

        assert!(patterns.contains(&"**/*.test-d.{ts,tsx}"));
        assert!(patterns.contains(&"test-d/**/*.test-d.{ts,tsx}"));
        assert!(patterns.contains(&"test-d/**/*.{ts,tsx}"));
    }

    #[test]
    fn package_json_config_directory_adds_entry_pattern() {
        let plugin = TsdPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsd.config.json"),
            r#"{"directory":"test/types"}"#,
            Path::new("/project"),
        );

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "test/types/**/*.test-d.{ts,tsx}"),
            "expected package.json#tsd.directory to add test/types entry pattern, got {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn tooling_dependencies_include_tsd() {
        let plugin = TsdPlugin;
        assert!(plugin.tooling_dependencies().contains(&"tsd"));
    }
}
