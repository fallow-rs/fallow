//! Deno runtime plugin.
//!
//! Activates when a project root has `deno.json` / `deno.jsonc`. Marks Deno
//! config as always-used and seeds default Deno test entry patterns.

use std::path::Path;

use super::Plugin;

const ALWAYS_USED: &[&str] = &["deno.json", "deno.jsonc"];

const DEFAULT_TEST_ENTRY_PATTERNS: &[&str] = &[
    "**/*_test.{ts,tsx,js,jsx,mts,mjs}",
    "**/*.test.{ts,tsx,js,jsx,mts,mjs}",
];

pub struct DenoPlugin;

impl Plugin for DenoPlugin {
    fn name(&self) -> &'static str {
        "deno"
    }

    fn is_enabled_with_deps(&self, _deps: &[String], root: &Path) -> bool {
        root.join("deno.json").is_file() || root.join("deno.jsonc").is_file()
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        DEFAULT_TEST_ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    #[test]
    fn activates_from_root_deno_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), r#"{"workspace":[]}"#).unwrap();
        assert!(DenoPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn inactive_without_deno_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!DenoPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn inactive_when_deno_config_is_only_in_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("deno.json"), "{}").unwrap();

        assert!(!DenoPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn is_test_entry_role() {
        assert_eq!(DenoPlugin.entry_point_role(), EntryPointRole::Test);
    }

    #[test]
    fn includes_esm_module_test_extensions() {
        assert!(
            DenoPlugin
                .entry_patterns()
                .iter()
                .all(|pattern| pattern.contains("mts,mjs"))
        );
    }
}
