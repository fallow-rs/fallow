//! Obsidian plugin framework support.
//!
//! Obsidian loads plugin entry files and calls lifecycle overrides from the
//! host application, so local source code often has no static references to
//! those files or methods. The rules here are intentionally narrow: activation
//! requires the `obsidian` package or an Obsidian-shaped manifest, `cdp.js` is
//! only credited at the project root, and lifecycle member credit is scoped to
//! direct Obsidian API base classes.

use std::path::{Path, PathBuf};

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use serde_json::Value;

use super::{Plugin, manifest::has_matching_manifest_json};

const ENABLERS: &[&str] = &["obsidian"];
const ENTRY_PATTERNS: &[&str] = &["src/main.{ts,js}", "main.{ts,js}", "cdp.js"];
const CONFIG_PATTERNS: &[&str] = &["manifest.json"];
const ALWAYS_USED: &[&str] = &["manifest.json", "styles.css"];

const PLUGIN_MEMBERS: &[&str] = &["onload", "onunload"];
const MODAL_MEMBERS: &[&str] = &["onOpen", "onClose"];
const VIEW_MEMBERS: &[&str] = &[
    "getViewType",
    "getDisplayText",
    "getIcon",
    "onOpen",
    "onClose",
    "onPaneMenu",
];

pub struct ObsidianPlugin;

impl Plugin for ObsidianPlugin {
    fn name(&self) -> &'static str {
        "obsidian"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
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

        has_matching_manifest_json(
            root,
            discovered_files,
            candidate_index,
            is_obsidian_manifest,
        )
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![
            scoped_rule("Plugin", PLUGIN_MEMBERS),
            scoped_rule("Modal", MODAL_MEMBERS),
            scoped_rule("ItemView", VIEW_MEMBERS),
            scoped_rule("View", VIEW_MEMBERS),
        ]
    }
}

fn scoped_rule(extends: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some(extends.to_string()),
        implements: None,
        members: members.iter().map(|member| (*member).to_string()).collect(),
    })
}

fn is_obsidian_manifest(manifest: &Value) -> bool {
    let Some(object) = manifest.as_object() else {
        return false;
    };

    if object.contains_key("manifest_version") {
        return false;
    }

    ["id", "name", "version", "minAppVersion"]
        .iter()
        .all(|key| object.get(*key).and_then(Value::as_str).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_for<'a>(
        rules: &'a [UsedClassMemberRule],
        extends: &str,
    ) -> &'a ScopedUsedClassMemberRule {
        rules
            .iter()
            .find_map(|rule| match rule {
                UsedClassMemberRule::Scoped(scoped)
                    if scoped.extends.as_deref() == Some(extends) =>
                {
                    Some(scoped)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{extends}-scoped rule missing"))
    }

    #[test]
    fn lifecycle_rules_are_scoped_to_obsidian_base_classes() {
        let rules = ObsidianPlugin.used_class_member_rules();

        for member in ["onload", "onunload"] {
            assert!(
                rule_for(&rules, "Plugin")
                    .members
                    .iter()
                    .any(|m| m == member)
            );
        }
        for member in ["onOpen", "onClose"] {
            assert!(
                rule_for(&rules, "Modal")
                    .members
                    .iter()
                    .any(|m| m == member)
            );
        }
        for base in ["ItemView", "View"] {
            for member in [
                "getViewType",
                "getDisplayText",
                "getIcon",
                "onOpen",
                "onClose",
            ] {
                assert!(rule_for(&rules, base).members.iter().any(|m| m == member));
            }
        }
    }

    #[test]
    fn lifecycle_rules_match_only_direct_base_names() {
        let rules = ObsidianPlugin.used_class_member_rules();
        let plugin_rule = rule_for(&rules, "Plugin");

        assert!(plugin_rule.matches_heritage(Some("Plugin"), &[]));
        assert!(!plugin_rule.matches_heritage(Some("ObsidianPlugin"), &[]));
        assert!(!plugin_rule.matches_heritage(Some("LocalPluginBase"), &[]));
        assert!(!plugin_rule.matches_heritage(None, &[]));
    }

    #[test]
    fn activates_from_obsidian_manifest_without_dependency() {
        let plugin = ObsidianPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            tmp.path().join("manifest.json"),
            r#"{"id":"work-terminal","name":"Work Terminal","version":"1.0.0","minAppVersion":"1.5.0"}"#,
        )
        .expect("manifest");

        assert!(plugin.is_enabled_with_files(
            &[],
            tmp.path(),
            &[tmp.path().join("src/main.ts")],
            None
        ));
    }

    #[test]
    fn index_activation_matches_filesystem_when_manifest_is_captured() {
        // Mirrors the browser_extension parity test: the non-production fast path
        // (Some(index)) reaches the same verdict as the production filesystem path
        // (None) when the walk captured the manifest, and a manifest present on
        // disk but absent from the index does NOT activate on the index path.
        let plugin = ObsidianPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        let manifest = tmp.path().join("manifest.json");
        std::fs::write(
            &manifest,
            r#"{"id":"work-terminal","name":"Work Terminal","version":"1.0.0","minAppVersion":"1.5.0"}"#,
        )
        .expect("manifest");
        let discovered = [tmp.path().join("src/main.ts")];

        let index_with = crate::plugins::registry::ConfigCandidateIndex::build(std::iter::once(
            manifest.as_path(),
        ));
        let index_without =
            crate::plugins::registry::ConfigCandidateIndex::build(std::iter::empty());

        // Filesystem path activates (manifest exists on disk).
        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &discovered, None));
        // Index path with the manifest captured: same verdict.
        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &discovered, Some(&index_with)));
        // Index path WITHOUT the manifest (e.g. gitignored): does not activate.
        assert!(!plugin.is_enabled_with_files(&[], tmp.path(), &discovered, Some(&index_without)));
    }

    #[test]
    fn rejects_browser_extension_pwa_and_generic_manifests() {
        let browser_extension = serde_json::json!({
            "manifest_version": 3,
            "name": "Extension",
            "version": "1.0.0",
            "background": { "service_worker": "background.js" }
        });
        let pwa = serde_json::json!({
            "name": "PWA",
            "start_url": "/",
            "display": "standalone",
            "icons": []
        });
        let generic_package_style = serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "description": "not an Obsidian plugin"
        });

        assert!(!is_obsidian_manifest(&browser_extension));
        assert!(!is_obsidian_manifest(&pwa));
        assert!(!is_obsidian_manifest(&generic_package_style));
    }
}
