//! GNOME Shell extension plugin.
//!
//! GNOME Shell loads an extension from a folder that holds `metadata.json`.
//! It imports `extension.js` from that folder, and `prefs.js` for the
//! preferences window. No project file imports them. The plugin reads each
//! `metadata.json` and seeds the two files only when the manifest has the
//! `uuid` and `shell-version` keys that GNOME Shell requires.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{Plugin, PluginResult};

const MANIFEST_FILE_NAME: &str = "metadata.json";
const EXTENSION_FILE_NAME: &str = "extension.js";
const CONFIG_PATTERNS: &[&str] = &[MANIFEST_FILE_NAME];
const ALWAYS_USED: &[&str] = &[MANIFEST_FILE_NAME, "**/metadata.json"];
const ENTRY_FILES: &[&str] = &[EXTENSION_FILE_NAME, "prefs.js"];

pub struct GnomeShellExtensionPlugin;

impl Plugin for GnomeShellExtensionPlugin {
    fn name(&self) -> &'static str {
        "gnome-shell-extension"
    }

    fn is_enabled_with_files(
        &self,
        _deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
        candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    ) -> bool {
        discovered_files.iter().any(|file| {
            if file.file_name() != Some(OsStr::new(EXTENSION_FILE_NAME)) || !file.starts_with(root)
            {
                return false;
            }
            let Some(dir) = file.parent() else {
                return false;
            };
            match candidate_index {
                Some(index) => index.dir_contains(dir, OsStr::new(MANIFEST_FILE_NAME)),
                None => dir.join(MANIFEST_FILE_NAME).is_file(),
            }
        })
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let is_extension = serde_json::from_str::<Value>(source)
            .is_ok_and(|manifest| is_extension_manifest(&manifest));
        if is_extension {
            result.extend_config_dir_entry_patterns(
                ENTRY_FILES.iter().map(|file| (*file).to_string()).collect(),
                config_path,
                root,
            );
        }
        result
    }
}

/// Whether `manifest` has the keys that GNOME Shell requires of an extension.
fn is_extension_manifest(manifest: &Value) -> bool {
    manifest.get("uuid").is_some_and(Value::is_string)
        && manifest.get("shell-version").is_some_and(Value::is_array)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_manifest_needs_a_uuid_and_a_shell_version_list() {
        assert!(is_extension_manifest(&json!({
            "uuid": "capture@example.org",
            "name": "Capture",
            "shell-version": ["46", "47"],
        })));
        assert!(!is_extension_manifest(
            &json!({ "uuid": "capture@example.org" })
        ));
        assert!(!is_extension_manifest(
            &json!({ "name": "data", "version": 2 })
        ));
    }

    #[test]
    fn a_metadata_file_of_another_tool_seeds_nothing() {
        let result = GnomeShellExtensionPlugin.resolve_config(
            Path::new("/project/data/metadata.json"),
            r#"{ "name": "dataset", "rows": 12 }"#,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn an_extension_manifest_seeds_the_files_of_its_folder() {
        let result = GnomeShellExtensionPlugin.resolve_config(
            Path::new("/project/apps/desktop/gnome-extension/metadata.json"),
            r#"{ "uuid": "capture@example.org", "shell-version": ["47"] }"#,
            Path::new("/project"),
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(
            patterns,
            [
                "apps/desktop/gnome-extension/extension.js",
                "apps/desktop/gnome-extension/prefs.js"
            ]
        );
    }
}
