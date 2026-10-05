//! Vercel plugin.
//!
//! Vercel deploys each file under the `api/` directory as a serverless
//! function. No code imports these files. Vercel skips files and directories
//! whose name starts with `_`, so these stay ordinary modules.
//!
//! Many Vercel projects declare no `vercel` dependency and deploy from a
//! `vercel.json` only. The plugin also activates when `vercel.json` exists at
//! the package root.

use std::path::{Path, PathBuf};

use super::{PathRule, Plugin, PluginResult, UsedExportRule};

const ENABLERS: &[&str] = &["vercel", "@vercel/config"];

/// Project configuration file that Vercel reads from the package root.
const VERCEL_JSON: &str = "vercel.json";

const CONFIG_PATTERNS: &[&str] = &["vercel.{ts,js,mjs,cjs,mts}"];

const ENTRY_PATTERNS: &[&str] = &[API_FUNCTIONS_PATTERN];

const API_FUNCTIONS_PATTERN: &str = "api/**/*.{js,mjs,cjs,ts,mts,cts}";

/// Vercel does not deploy a file or a directory whose name starts with `_`.
const PRIVATE_API_GLOBS: &[&str] = &["api/**/_*", "api/**/_*/**"];

/// Handler exports that the Vercel runtime reads from a function file.
const FUNCTION_EXPORTS: &[&str] = &[
    "default", "config", "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS",
];

const ALWAYS_USED: &[&str] = &["vercel.{ts,js,mjs,cjs,mts}", VERCEL_JSON];

const TOOLING_DEPENDENCIES: &[&str] = &["vercel", "@vercel/config"];

pub struct VercelPlugin;

impl Plugin for VercelPlugin {
    fn name(&self) -> &'static str {
        "vercel"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        _discovered_files: &[PathBuf],
        _candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    ) -> bool {
        self.is_enabled_with_deps(deps, root) || root.join(VERCEL_JSON).is_file()
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        vec![
            PathRule::new(API_FUNCTIONS_PATTERN)
                .with_excluded_globs(PRIVATE_API_GLOBS.iter().copied()),
        ]
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        vec![
            UsedExportRule::new(API_FUNCTIONS_PATTERN, FUNCTION_EXPORTS.iter().copied())
                .with_excluded_globs(PRIVATE_API_GLOBS.iter().copied()),
        ]
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    #[test]
    fn activates_from_root_vercel_json_without_dependency() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("vercel.json"), "{}").unwrap();

        assert!(VercelPlugin.is_enabled_with_files(&[], temp.path(), &[], None));
    }

    #[test]
    fn stays_inactive_without_dependency_or_vercel_json() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("api")).unwrap();

        assert!(!VercelPlugin.is_enabled_with_files(&[], temp.path(), &[], None));
    }

    #[test]
    fn activates_from_vercel_dependency() {
        let deps = vec!["vercel".to_string()];

        assert!(VercelPlugin.is_enabled_with_files(&deps, Path::new("/project"), &[], None));
    }

    #[test]
    fn api_entry_rule_excludes_underscore_files_and_directories() {
        let rules = VercelPlugin.entry_pattern_rules();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, API_FUNCTIONS_PATTERN);
        assert_eq!(rules[0].exclude_globs, PRIVATE_API_GLOBS);
    }

    #[test]
    fn handler_exports_are_credited_on_function_files_only() {
        let rules = VercelPlugin.used_export_rules();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].path.pattern, API_FUNCTIONS_PATTERN);
        assert_eq!(rules[0].path.exclude_globs, PRIVATE_API_GLOBS);
        for export in ["default", "config", "GET", "POST"] {
            assert!(rules[0].exports.iter().any(|name| name == export));
        }
    }

    #[test]
    fn entry_point_role_is_runtime() {
        assert_eq!(VercelPlugin.entry_point_role(), EntryPointRole::Runtime);
    }
}
