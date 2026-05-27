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
use rustc_hash::FxHashSet;
use serde_json::Value;

use super::Plugin;

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
    ) -> bool {
        if self.is_enabled_with_deps(deps, root) {
            return true;
        }

        manifest_candidates(root, discovered_files)
            .into_iter()
            .any(|path| {
                let Ok(source) = std::fs::read_to_string(path) else {
                    return false;
                };
                parse_manifest(&source).is_some_and(|manifest| is_obsidian_manifest(&manifest))
            })
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

fn manifest_candidates(root: &Path, discovered_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = FxHashSet::default();
    let mut candidates = Vec::new();
    push_manifest_candidate(root, &mut seen, &mut candidates);

    for file in discovered_files {
        let mut current = file.parent();
        while let Some(dir) = current {
            if !dir.starts_with(root) {
                break;
            }
            push_manifest_candidate(dir, &mut seen, &mut candidates);
            if dir == root {
                break;
            }
            current = dir.parent();
        }
    }

    candidates
}

fn push_manifest_candidate(
    dir: &Path,
    seen: &mut FxHashSet<PathBuf>,
    candidates: &mut Vec<PathBuf>,
) {
    let candidate = dir.join("manifest.json");
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

fn parse_manifest(source: &str) -> Option<Value> {
    serde_json::from_str(source).ok()
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
    use fallow_config::EntryPointRole;

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
    fn exposes_static_patterns_and_runtime_role() {
        let plugin = ObsidianPlugin;

        assert_eq!(plugin.enablers(), ENABLERS);
        assert_eq!(plugin.entry_patterns(), ENTRY_PATTERNS);
        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert_eq!(plugin.always_used(), ALWAYS_USED);
        assert_eq!(plugin.entry_point_role(), EntryPointRole::Runtime);
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

        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &[tmp.path().join("src/main.ts")]));
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
