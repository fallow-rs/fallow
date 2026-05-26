//! Mintlify documentation plugin.
//!
//! Mintlify sites are driven by a `docs.json` (or legacy `mint.json`)
//! configuration file and MDX content rendered by the `mint` / `mintlify`
//! CLI at runtime, not by application imports. This plugin keeps the config
//! file and the docs `{md,mdx}` content under its directory alive, and credits
//! the Mintlify CLI dependency as tooling.

use std::path::Path;

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["mint", "mintlify"];
const CONFIG_PATTERNS: &[&str] = &["docs.json", "mint.json"];
const ALWAYS_USED: &[&str] = &["docs.json", "mint.json"];
const TOOLING_DEPENDENCIES: &[&str] = &["mint", "mintlify"];
const CONTENT_EXTENSIONS: &str = "{md,mdx}";

/// Built-in plugin for Mintlify documentation sites.
pub struct MintlifyPlugin;

impl Plugin for MintlifyPlugin {
    fn name(&self) -> &'static str {
        "mintlify"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep == enabler))
            || root.join("docs.json").is_file()
            || root.join("mint.json").is_file()
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

    fn resolve_config(&self, config_path: &Path, _source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        // The docs content root is the directory that holds docs.json /
        // mint.json. Scope the entry pattern to that directory so non-Mintlify
        // MDX elsewhere stays governed by other plugins. Workspace runs prefix
        // the (workspace-relative) pattern back to the project root for us.
        let Some(docs_dir) = config_path.parent() else {
            return result;
        };
        let Ok(relative_dir) = docs_dir.strip_prefix(root) else {
            return result;
        };
        let relative_dir = relative_dir.to_string_lossy().replace('\\', "/");
        let relative_dir = relative_dir.trim_matches('/');

        let pattern = if relative_dir.is_empty() {
            format!("**/*.{CONTENT_EXTENSIONS}")
        } else {
            format!("{relative_dir}/**/*.{CONTENT_EXTENSIONS}")
        };
        result.push_entry_pattern(pattern);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_patterns(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn activates_from_cli_dependency() {
        let plugin = MintlifyPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["mint".to_string()], tmp.path()));
        assert!(plugin.is_enabled_with_deps(&["mintlify".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["next".to_string()], tmp.path()));
    }

    #[test]
    fn activates_from_docs_or_mint_config_file() {
        let plugin = MintlifyPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(!plugin.is_enabled_with_deps(&[], tmp.path()));

        std::fs::write(tmp.path().join("docs.json"), "{}\n").expect("docs config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));

        std::fs::remove_file(tmp.path().join("docs.json")).expect("remove docs config");
        std::fs::write(tmp.path().join("mint.json"), "{}\n").expect("mint config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn exposes_static_mintlify_conventions() {
        let plugin = MintlifyPlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert!(plugin.always_used().contains(&"docs.json"));
        assert!(plugin.always_used().contains(&"mint.json"));
        assert!(plugin.tooling_dependencies().contains(&"mint"));
        assert!(plugin.tooling_dependencies().contains(&"mintlify"));
    }

    #[test]
    fn resolve_config_scopes_content_to_nested_docs_root() {
        let plugin = MintlifyPlugin;
        let root = Path::new("/repo");
        let config_path = root.join("apps/docs/docs.json");

        let result = plugin.resolve_config(&config_path, "{}", root);

        assert_eq!(
            entry_patterns(&result),
            vec!["apps/docs/**/*.{md,mdx}".to_string()],
            "content pattern should be scoped to the docs.json directory"
        );
    }

    #[test]
    fn resolve_config_handles_root_level_config() {
        let plugin = MintlifyPlugin;
        let root = Path::new("/repo");
        let config_path = root.join("mint.json");

        let result = plugin.resolve_config(&config_path, "{}", root);

        assert_eq!(entry_patterns(&result), vec!["**/*.{md,mdx}".to_string()]);
    }

    #[test]
    fn resolve_config_does_not_emit_project_wide_pattern_for_nested_docs() {
        let plugin = MintlifyPlugin;
        let root = Path::new("/repo");
        let config_path = root.join("apps/docs/docs.json");

        let result = plugin.resolve_config(&config_path, "{}", root);

        assert!(
            !entry_patterns(&result).contains(&"**/*.{md,mdx}".to_string()),
            "a nested docs root must not credit MDX across the whole project"
        );
    }

    #[test]
    fn resolve_config_ignores_config_outside_root() {
        let plugin = MintlifyPlugin;
        let result = plugin.resolve_config(
            Path::new("/elsewhere/docs/docs.json"),
            "{}",
            Path::new("/repo"),
        );

        assert!(result.is_empty());
    }

    #[test]
    fn resolve_config_does_not_inspect_source_so_malformed_json_is_safe() {
        let plugin = MintlifyPlugin;
        let root = Path::new("/repo");
        let result = plugin.resolve_config(&root.join("docs.json"), "{ not valid json", root);

        // The plugin keys off the config file location, not its contents, so a
        // malformed body still yields the docs-root content pattern.
        assert_eq!(entry_patterns(&result), vec!["**/*.{md,mdx}".to_string()]);
    }
}
