//! Browser extension manifest plugin.
//!
//! WebExtension and Chrome Extension projects declare runtime entry files in
//! `manifest.json`, often without any package dependency that can activate a
//! framework plugin. This plugin keeps those manifest-declared local resources
//! reachable while avoiding ordinary PWA/web-app manifests.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::{
    Plugin, PluginResult, config_parser,
    manifest::{has_matching_manifest_json, parse_manifest_json},
};

const ENABLERS: &[&str] = &[
    "webextension-polyfill",
    "@types/chrome",
    "@types/firefox-webext-browser",
];
const CONFIG_PATTERNS: &[&str] = &["manifest.json"];
const ALWAYS_USED: &[&str] = &["manifest.json"];
const EXTENSION_RUNTIME_KEYS: &[&str] = &[
    "background",
    "content_scripts",
    "action",
    "browser_action",
    "page_action",
    "options_page",
    "options_ui",
    "side_panel",
    "devtools_page",
    "web_accessible_resources",
];

/// Built-in plugin for browser extension manifests.
pub struct BrowserExtensionPlugin;

impl Plugin for BrowserExtensionPlugin {
    fn name(&self) -> &'static str {
        "browser-extension"
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
            is_extension_manifest,
        )
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(manifest) = parse_manifest_json(source) else {
            return result;
        };
        if !is_extension_manifest(&manifest) {
            return result;
        }

        let mut entries = collect_manifest_entries(&manifest)
            .into_iter()
            .filter_map(|entry| normalize_manifest_path(entry, config_path, root))
            .collect::<Vec<_>>();
        entries.sort();
        entries.dedup();
        result.extend_entry_patterns(entries);
        result
    }
}

fn is_extension_manifest(manifest: &Value) -> bool {
    let Some(object) = manifest.as_object() else {
        return false;
    };
    let manifest_version = object
        .get("manifest_version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version == 2 || version == 3);

    manifest_version
        && EXTENSION_RUNTIME_KEYS
            .iter()
            .any(|key| object.contains_key(*key))
}

fn collect_manifest_entries(manifest: &Value) -> Vec<&str> {
    let mut entries = Vec::new();
    collect_background_entries(manifest, &mut entries);
    collect_content_script_entries(manifest, &mut entries);
    collect_html_entries(manifest, &mut entries);
    collect_web_accessible_resources(manifest, &mut entries);
    entries
}

fn collect_background_entries<'a>(manifest: &'a Value, entries: &mut Vec<&'a str>) {
    let Some(background) = manifest.get("background").and_then(Value::as_object) else {
        return;
    };

    push_string_field(background.get("service_worker"), entries);
    push_string_array_field(background.get("scripts"), entries);
}

fn collect_content_script_entries<'a>(manifest: &'a Value, entries: &mut Vec<&'a str>) {
    let Some(content_scripts) = manifest.get("content_scripts").and_then(Value::as_array) else {
        return;
    };

    for script in content_scripts {
        let Some(object) = script.as_object() else {
            continue;
        };
        push_string_array_field(object.get("js"), entries);
        push_string_array_field(object.get("css"), entries);
    }
}

fn collect_html_entries<'a>(manifest: &'a Value, entries: &mut Vec<&'a str>) {
    push_string_field(manifest.get("options_page"), entries);
    push_nested_string_field(manifest, &["action", "default_popup"], entries);
    push_nested_string_field(manifest, &["browser_action", "default_popup"], entries);
    push_nested_string_field(manifest, &["page_action", "default_popup"], entries);
    push_nested_string_field(manifest, &["options_ui", "page"], entries);
    push_nested_string_field(manifest, &["side_panel", "default_path"], entries);
    push_string_field(manifest.get("devtools_page"), entries);
}

fn collect_web_accessible_resources<'a>(manifest: &'a Value, entries: &mut Vec<&'a str>) {
    let Some(resources) = manifest
        .get("web_accessible_resources")
        .and_then(Value::as_array)
    else {
        return;
    };

    for resource in resources {
        match resource {
            Value::String(raw) => entries.push(raw.as_str()),
            Value::Object(object) => push_string_array_field(object.get("resources"), entries),
            _ => {}
        }
    }
}

fn push_nested_string_field<'a>(manifest: &'a Value, path: &[&str], entries: &mut Vec<&'a str>) {
    let mut current = manifest;
    for segment in path {
        let Some(next) = current.get(*segment) else {
            return;
        };
        current = next;
    }
    push_string_field(Some(current), entries);
}

fn push_string_field<'a>(value: Option<&'a Value>, entries: &mut Vec<&'a str>) {
    if let Some(raw) = value.and_then(Value::as_str) {
        entries.push(raw);
    }
}

fn push_string_array_field<'a>(value: Option<&'a Value>, entries: &mut Vec<&'a str>) {
    let Some(array) = value.and_then(Value::as_array) else {
        return;
    };
    entries.extend(array.iter().filter_map(Value::as_str));
}

fn normalize_manifest_path(raw: &str, config_path: &Path, root: &Path) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || has_scheme(trimmed)
        || has_parent_component(trimmed)
    {
        return None;
    }

    let manifest_relative = trimmed.strip_prefix('/').unwrap_or(trimmed);
    config_parser::normalize_config_path(manifest_relative, config_path, root)
}

fn has_scheme(raw: &str) -> bool {
    let Some(colon) = raw.find(':') else {
        return false;
    };
    raw[..colon]
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn has_parent_component(raw: &str) -> bool {
    Path::new(raw)
        .components()
        .any(|component| component == Component::ParentDir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_strings(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn exposes_config_and_always_used_manifest_pattern() {
        let plugin = BrowserExtensionPlugin;

        assert!(plugin.always_used().contains(&"manifest.json"));
        assert!(plugin.enablers().contains(&"webextension-polyfill"));
    }

    #[test]
    fn activates_from_extension_manifest_without_dependency() {
        let plugin = BrowserExtensionPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        let extension = tmp.path().join("extension");
        std::fs::create_dir_all(&extension).expect("extension dir");
        std::fs::write(
            extension.join("manifest.json"),
            r#"{"manifest_version":3,"background":{"service_worker":"background.js"}}"#,
        )
        .expect("manifest");

        assert!(plugin.is_enabled_with_files(
            &[],
            tmp.path(),
            &[extension.join("background.js")],
            None
        ));
    }

    #[test]
    fn index_activation_matches_filesystem_when_manifest_is_captured() {
        // The non-production fast path (Some(index)) must reach the same verdict
        // as the production filesystem path (None) when the discovery walk
        // captured the manifest. The divergence direction is also pinned: a
        // manifest present on disk but ABSENT from the index (gitignored /
        // ignorePatterns / non-traversed hidden dir) does NOT activate on the
        // index path, matching "config discovery follows source traversal rules".
        let plugin = BrowserExtensionPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        let extension = tmp.path().join("extension");
        std::fs::create_dir_all(&extension).expect("extension dir");
        let manifest = extension.join("manifest.json");
        std::fs::write(
            &manifest,
            r#"{"manifest_version":3,"background":{"service_worker":"background.js"}}"#,
        )
        .expect("manifest");
        let discovered = [extension.join("background.js")];

        let index_with = crate::plugins::registry::ConfigCandidateIndex::build(std::iter::once(
            manifest.as_path(),
        ));
        let index_without =
            crate::plugins::registry::ConfigCandidateIndex::build(std::iter::empty());

        // Filesystem path activates (manifest exists on disk).
        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &discovered, None));
        // Index path with the manifest captured: same verdict.
        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &discovered, Some(&index_with)));
        // Index path WITHOUT the manifest (e.g. gitignored): does not activate,
        // even though the file is on disk. Documents the accepted refinement.
        assert!(!plugin.is_enabled_with_files(&[], tmp.path(), &discovered, Some(&index_without)));
    }

    #[test]
    fn does_not_activate_from_pwa_manifest() {
        let plugin = BrowserExtensionPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            tmp.path().join("manifest.json"),
            r#"{"name":"App","start_url":"/","display":"standalone","icons":[]}"#,
        )
        .expect("manifest");

        assert!(!plugin.is_enabled_with_files(
            &[],
            tmp.path(),
            &[tmp.path().join("src/app.js")],
            None
        ));
    }

    #[test]
    fn does_not_activate_without_supported_manifest_version() {
        let manifest = serde_json::json!({
            "manifest_version": 1,
            "background": { "service_worker": "background.js" }
        });

        assert!(!is_extension_manifest(&manifest));
    }

    #[test]
    fn collects_mv3_and_html_entries() {
        let source = r#"{
            "manifest_version": 3,
            "background": { "service_worker": "background.js" },
            "content_scripts": [
                { "matches": ["https://example.com/*"], "js": ["content.js"], "css": ["styles/content.css"] }
            ],
            "action": { "default_popup": "popup/index.html" },
            "options_ui": { "page": "options.html" },
            "side_panel": { "default_path": "side.html" },
            "devtools_page": "devtools.html",
            "web_accessible_resources": [
                { "resources": ["web-accessible.js", "images/*.png"], "matches": ["https://example.com/*"] }
            ]
        }"#;

        let result = BrowserExtensionPlugin.resolve_config(
            Path::new("/repo/extension/manifest.json"),
            source,
            Path::new("/repo"),
        );

        assert_eq!(
            entry_strings(&result),
            vec![
                "extension/background.js",
                "extension/content.js",
                "extension/devtools.html",
                "extension/images/*.png",
                "extension/options.html",
                "extension/popup/index.html",
                "extension/side.html",
                "extension/styles/content.css",
                "extension/web-accessible.js",
            ]
        );
    }

    #[test]
    fn collects_mv2_background_and_web_accessible_resources() {
        let source = r#"{
            "manifest_version": 2,
            "background": { "scripts": ["background-a.js", "background-b.js"] },
            "browser_action": { "default_popup": "popup.html" },
            "page_action": { "default_popup": "page.html" },
            "options_page": "options.html",
            "web_accessible_resources": ["asset.js"]
        }"#;

        let result = BrowserExtensionPlugin.resolve_config(
            Path::new("/repo/manifest.json"),
            source,
            Path::new("/repo"),
        );

        assert_eq!(
            entry_strings(&result),
            vec![
                "asset.js",
                "background-a.js",
                "background-b.js",
                "options.html",
                "page.html",
                "popup.html",
            ]
        );
    }

    #[test]
    fn normalizes_manifest_relative_paths_and_rejects_remote_or_escaping_values() {
        let source = r#"{
            "manifest_version": 3,
            "background": { "service_worker": "./background.js" },
            "content_scripts": [
                {
                    "js": [
                        "/absolute-from-extension-root.js",
                        "https://example.com/remote.js",
                        "//cdn.example.com/remote.js",
                        "data:text/javascript,alert(1)",
                        "../escape.js",
                        ""
                    ]
                }
            ]
        }"#;

        let result = BrowserExtensionPlugin.resolve_config(
            Path::new("/repo/extension/manifest.json"),
            source,
            Path::new("/repo"),
        );

        assert_eq!(
            entry_strings(&result),
            vec![
                "extension/absolute-from-extension-root.js",
                "extension/background.js",
            ]
        );
    }

    #[test]
    fn ignores_non_extension_manifest_during_config_resolution() {
        let result = BrowserExtensionPlugin.resolve_config(
            Path::new("/repo/manifest.json"),
            r#"{"name":"App","start_url":"/"}"#,
            Path::new("/repo"),
        );

        assert!(result.is_empty());
    }
}
