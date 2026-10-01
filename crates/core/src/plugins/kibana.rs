//! Kibana plugin.
//!
//! Each Kibana plugin and package declares itself in a `kibana.jsonc`
//! manifest. The Kibana platform loads a plugin from its `public` and `server`
//! index files, which no source file imports. This plugin reads each manifest
//! with the shared `manifestEntries` engine and seeds those files as entry
//! points. It also keeps the Scout Playwright files that the Scout runner
//! loads by convention.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_config::{
    ManifestEntryRule, ManifestFormat, ScopedUsedClassMemberRule, UsedClassMemberRule,
};

use super::manifest_entries::{parse_manifest, seed_parsed_manifest};
use super::{Plugin, PluginResult};

const MANIFEST_FILE_NAME: &str = "kibana.jsonc";
const CONFIG_PATTERNS: &[&str] = &[MANIFEST_FILE_NAME];
const ALWAYS_USED: &[&str] = &[MANIFEST_FILE_NAME, "**/kibana.jsonc"];

/// The Kibana platform calls these members on the object that a plugin entry
/// returns. Plugin classes declare them through the `Plugin`, `PrebootPlugin`
/// or `AsyncPlugin` interface of `@kbn/core/public` and `@kbn/core/server`.
const PLUGIN_INTERFACES: &[&str] = &["Plugin", "PrebootPlugin", "AsyncPlugin"];
const PLUGIN_LIFECYCLE_MEMBERS: &[&str] = &["setup", "start", "stop"];

/// Entry rules for the files the Kibana platform loads.
///
/// - `plugin.browser` and `plugin.server` select the `public` and `server`
///   plugin entries.
/// - `common/index` is the third public surface of a plugin, which other
///   plugins import as `@kbn/<id>/common`.
/// - `plugin.extraPublicDirs` lists more directories that other plugins can
///   import.
const RUNTIME_RULES: &str = r#"[
  {
    "manifests": "**/kibana.jsonc",
    "when": { "type": "plugin" },
    "entries": [
      { "path": "public/index.{ts,tsx}", "when": { "plugin.browser": true } },
      { "path": "server/index.{ts,tsx}", "when": { "plugin.server": true } },
      { "path": "common/index.{ts,tsx}" },
      { "path": "${plugin.extraPublicDirs}/index.{ts,tsx}" }
    ]
  }
]"#;

/// Support rules for the Scout test runner. A Scout config calls
/// `createPlaywrightConfig`, and that config runs `global.setup.ts` and
/// `global.teardown.ts` from the test directory by file name.
const SUPPORT_RULES: &str = r#"[
  {
    "manifests": "**/kibana.jsonc",
    "entries": [
      { "path": "test/scout/**/*playwright.config.ts" },
      { "path": "test/scout/**/global.{setup,teardown}.ts" }
    ]
  }
]"#;

fn parse_rules(source: &'static str) -> Vec<ManifestEntryRule> {
    serde_json::from_str(source).unwrap_or_default()
}

fn runtime_rules() -> &'static [ManifestEntryRule] {
    static RULES: OnceLock<Vec<ManifestEntryRule>> = OnceLock::new();
    RULES.get_or_init(|| parse_rules(RUNTIME_RULES))
}

fn support_rules() -> &'static [ManifestEntryRule] {
    static RULES: OnceLock<Vec<ManifestEntryRule>> = OnceLock::new();
    RULES.get_or_init(|| parse_rules(SUPPORT_RULES))
}

/// Built-in plugin for Kibana `kibana.jsonc` manifests.
pub struct KibanaPlugin;

impl Plugin for KibanaPlugin {
    fn name(&self) -> &'static str {
        "kibana"
    }

    fn is_enabled_with_deps(&self, _deps: &[String], root: &Path) -> bool {
        root.join(MANIFEST_FILE_NAME).is_file()
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
        candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    ) -> bool {
        if self.is_enabled_with_deps(deps, root) {
            return true;
        }
        // A Kibana repository has no root manifest: every plugin and package
        // holds its own. The manifest is not a source file, so look for it in
        // the config candidates of the discovery walk, or on disk next to the
        // discovered files when no candidate index exists (production mode).
        let name = OsStr::new(MANIFEST_FILE_NAME);
        match candidate_index {
            Some(index) => index.any_descendant_contains(root, name),
            None => has_manifest_above_discovered_file(root, discovered_files),
        }
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        PLUGIN_INTERFACES
            .iter()
            .map(|interface| {
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some((*interface).to_string()),
                    members: PLUGIN_LIFECYCLE_MEMBERS
                        .iter()
                        .map(|member| (*member).to_string())
                        .collect(),
                })
            })
            .collect()
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        if config_path.file_name() != Some(OsStr::new(MANIFEST_FILE_NAME)) {
            return result;
        }
        let Some(manifest) = parse_manifest(source, ManifestFormat::Jsonc) else {
            return result;
        };
        result.extend_entry_patterns(seed_rules(runtime_rules(), &manifest, config_path, root));
        result
            .always_used_files
            .extend(seed_rules(support_rules(), &manifest, config_path, root));
        result
    }
}

fn seed_rules(
    rules: &[ManifestEntryRule],
    manifest: &serde_json::Value,
    manifest_path: &Path,
    root: &Path,
) -> Vec<String> {
    let mut seeded: Vec<String> = rules
        .iter()
        .flat_map(|rule| seed_parsed_manifest(rule, manifest, manifest_path, root))
        .collect();
    seeded.sort();
    seeded.dedup();
    seeded
}

/// Probe the directories that hold discovered files, and their ancestors up to
/// `root`, for a manifest. Each directory is probed once.
fn has_manifest_above_discovered_file(root: &Path, discovered_files: &[PathBuf]) -> bool {
    let mut seen: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    for file in discovered_files {
        let mut current = file.parent();
        while let Some(dir) = current {
            if !dir.starts_with(root) || !seen.insert(dir) {
                break;
            }
            if dir.join(MANIFEST_FILE_NAME).is_file() {
                return true;
            }
            current = dir.parent();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    fn resolve(manifest_rel: &str, source: &str) -> PluginResult {
        let root = Path::new("/repo");
        KibanaPlugin.resolve_config(&root.join(manifest_rel), source, root)
    }

    fn entries(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn embedded_rules_parse() {
        assert_eq!(runtime_rules().len(), 1);
        assert_eq!(support_rules().len(), 1);
    }

    #[test]
    fn plugin_manifest_seeds_browser_server_and_common_entries() {
        let result = resolve(
            "x-pack/plugins/alpha/kibana.jsonc",
            r#"{
              "type": "plugin",
              "id": "@kbn/alpha-plugin",
              // comment
              "plugin": { "id": "alpha", "server": true, "browser": true, }
            }"#,
        );
        assert_eq!(
            entries(&result),
            vec![
                "x-pack/plugins/alpha/common/index.{ts,tsx}",
                "x-pack/plugins/alpha/public/index.{ts,tsx}",
                "x-pack/plugins/alpha/server/index.{ts,tsx}",
            ]
        );
    }

    #[test]
    fn plugin_flags_gate_public_and_server_entries() {
        let result = resolve(
            "plugins/beta/kibana.jsonc",
            r#"{ "type": "plugin", "plugin": { "id": "beta", "server": false, "browser": true } }"#,
        );
        assert_eq!(
            entries(&result),
            vec![
                "plugins/beta/common/index.{ts,tsx}",
                "plugins/beta/public/index.{ts,tsx}",
            ]
        );
    }

    #[test]
    fn extra_public_dirs_seed_their_index_files() {
        let result = resolve(
            "plugins/data/kibana.jsonc",
            r#"{
              "type": "plugin",
              "plugin": { "id": "data", "browser": true, "extraPublicDirs": ["common", "common/api"] }
            }"#,
        );
        assert_eq!(
            entries(&result),
            vec![
                "plugins/data/common/api/index.{ts,tsx}",
                "plugins/data/common/index.{ts,tsx}",
                "plugins/data/public/index.{ts,tsx}",
            ]
        );
    }

    #[test]
    fn package_manifest_seeds_no_plugin_entries() {
        let result = resolve(
            "packages/kbn-shared/kibana.jsonc",
            r#"{ "type": "shared-common", "id": "@kbn/shared" }"#,
        );
        assert!(entries(&result).is_empty());
    }

    #[test]
    fn every_manifest_keeps_scout_playwright_files() {
        let result = resolve(
            "packages/kbn-shared/kibana.jsonc",
            r#"{ "type": "shared-common", "id": "@kbn/shared" }"#,
        );
        assert_eq!(
            result.always_used_files,
            vec![
                "packages/kbn-shared/test/scout/**/*playwright.config.ts",
                "packages/kbn-shared/test/scout/**/global.{setup,teardown}.ts",
            ]
        );
    }

    #[test]
    fn unreadable_or_foreign_manifest_seeds_nothing() {
        assert!(resolve("plugins/x/kibana.jsonc", "{ not json").is_empty());
        let root = Path::new("/repo");
        let other = KibanaPlugin.resolve_config(
            &root.join("plugins/x/other.jsonc"),
            r#"{ "type": "plugin", "plugin": { "browser": true } }"#,
            root,
        );
        assert!(other.is_empty());
    }

    #[test]
    fn extra_public_dir_outside_the_root_is_skipped() {
        let result = resolve(
            "kibana.jsonc",
            r#"{ "type": "plugin", "plugin": { "extraPublicDirs": ["../outside"] } }"#,
        );
        assert_eq!(entries(&result), vec!["common/index.{ts,tsx}"]);
    }

    #[test]
    fn activates_from_a_nested_manifest_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("x-pack/plugins/alpha");
        std::fs::create_dir_all(plugin.join("public")).unwrap();
        std::fs::write(plugin.join(MANIFEST_FILE_NAME), r#"{"type":"plugin"}"#).unwrap();
        let file = plugin.join("public/index.ts");
        std::fs::write(&file, "").unwrap();

        assert!(KibanaPlugin.is_enabled_with_files(&[], dir.path(), &[file], None));
    }

    #[test]
    fn activates_from_a_nested_manifest_in_the_candidate_index() {
        let root = Path::new("/repo");
        let manifest = root.join("x-pack/plugins/alpha/kibana.jsonc");
        let index = super::super::registry::ConfigCandidateIndex::build(std::iter::once(
            manifest.as_path(),
        ));
        assert!(KibanaPlugin.is_enabled_with_files(&[], root, &[], Some(&index)));
    }

    #[test]
    fn inactive_without_a_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("src/index.ts");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        assert!(!KibanaPlugin.is_enabled_with_files(&[], dir.path(), &[file], None));

        let index = super::super::registry::ConfigCandidateIndex::build(std::iter::empty());
        assert!(!KibanaPlugin.is_enabled_with_files(&[], dir.path(), &[], Some(&index)));
    }

    #[test]
    fn plugin_class_lifecycle_members_are_used() {
        let rules = KibanaPlugin.used_class_member_rules();
        let mut interfaces = Vec::new();
        for rule in &rules {
            let fallow_config::UsedClassMemberRule::Scoped(rule) = rule else {
                panic!("each lifecycle rule must be scoped to a plugin interface");
            };
            assert_eq!(rule.extends, None);
            assert_eq!(rule.members, vec!["setup", "start", "stop"]);
            interfaces.push(rule.implements.clone().unwrap_or_default());
        }
        assert_eq!(interfaces, vec!["Plugin", "PrebootPlugin", "AsyncPlugin"]);
    }

    #[test]
    fn is_runtime_entry_role() {
        assert_eq!(KibanaPlugin.entry_point_role(), EntryPointRole::Runtime);
    }
}
