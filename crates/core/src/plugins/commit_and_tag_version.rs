//! commit-and-tag-version (and legacy standard-version) plugin.
//!
//! commit-and-tag-version bumps the version in arbitrary files via `bumpFiles`
//! and `packageFiles` entries. Each entry may name a custom `updater` JS module
//! (with `readVersion` / `writeVersion`) that the tool loads at runtime. That
//! updater is a real source file consumed only by the tool, so it has no static
//! importer and would otherwise surface as `unused-files`. We credit each
//! entry's `updater` (the load-bearing custom module) and `filename` (the bump
//! target) as reachable support files. Crediting is gated downstream on the
//! file existing on disk, so non-source targets (gradle, plist, version.txt)
//! and phantom paths are never over-credited.
//!
//! See issue #1640.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["commit-and-tag-version", "standard-version"];

const CONFIG_PATTERNS: &[&str] = &[".versionrc", ".versionrc.{json,js,cjs}"];

const ALWAYS_USED: &[&str] = &[".versionrc", ".versionrc.{json,js,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["commit-and-tag-version", "standard-version"];

/// Array fields whose object entries carry version-bump file references.
const FILE_ARRAYS: &[&str] = &["bumpFiles", "packageFiles"];

/// Object keys inside a bump entry that point at a local file path. `updater`
/// is the custom JS updater module; `filename` is the file whose version is
/// rewritten.
const FILE_KEYS: &[&str] = &["updater", "filename"];

define_plugin! {
    struct CommitAndTagVersionPlugin => "commit-and-tag-version",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "commit-and-tag-version",
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let config_dir = config_path
            .parent()
            .filter(|parent| parent.is_absolute())
            .unwrap_or(root);

        for array in FILE_ARRAYS {
            for key in FILE_KEYS {
                for file in
                    config_parser::extract_config_array_object_strings(source, config_path, &[array], key)
                {
                    let trimmed = file.trim_start_matches("./");
                    if trimmed.is_empty() {
                        continue;
                    }
                    result.setup_files.push(config_dir.join(trimmed));
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Build a platform-absolute path from a logical `/`-rooted string so the
    /// `config_path.parent().filter(is_absolute)` resolution behaves the same on
    /// Windows (where a bare `/project` is not absolute). Mirrors `playwright.rs`.
    fn abs(logical: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{}", logical.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(logical)
        }
    }

    fn resolve(config_path: &str, source: &str) -> Vec<PathBuf> {
        let plugin = CommitAndTagVersionPlugin;
        plugin
            .resolve_config(&abs(config_path), source, &abs("/project"))
            .setup_files
    }

    #[test]
    fn credits_updater_and_filename_from_bump_files() {
        let source = r#"{
            "bumpFiles": [
                {
                    "filename": "app/android/app/build.gradle",
                    "updater": "app/scripts/gradle-updater.cjs"
                }
            ]
        }"#;
        let files = resolve("/project/commit-and-tag-version.config.json", source);
        assert!(files.contains(&abs("/project/app/scripts/gradle-updater.cjs")));
        assert!(files.contains(&abs("/project/app/android/app/build.gradle")));
    }

    #[test]
    fn credits_package_files_entries() {
        let source = r#"{
            "packageFiles": [
                { "filename": "manifest.json", "updater": "scripts/manifest-updater.js" }
            ]
        }"#;
        let files = resolve("/project/commit-and-tag-version.config.json", source);
        assert!(files.contains(&abs("/project/scripts/manifest-updater.js")));
        assert!(files.contains(&abs("/project/manifest.json")));
    }

    #[test]
    fn strips_leading_dot_slash() {
        let source = r#"{
            "bumpFiles": [
                { "filename": "VERSION", "updater": "./version-updater.cjs" }
            ]
        }"#;
        let files = resolve("/project/commit-and-tag-version.config.json", source);
        assert!(files.contains(&abs("/project/version-updater.cjs")));
    }

    #[test]
    fn resolves_relative_to_config_directory() {
        let source = r#"{
            "bumpFiles": [
                { "filename": "pkg.json", "updater": "u.cjs" }
            ]
        }"#;
        let files = resolve("/project/sub/.versionrc.json", source);
        assert!(files.contains(&abs("/project/sub/u.cjs")));
        assert!(files.contains(&abs("/project/sub/pkg.json")));
    }

    #[test]
    fn type_only_entries_credit_filename_only() {
        // Built-in `type` updaters have no custom module; only `filename` is a
        // local file reference.
        let source = r#"{
            "bumpFiles": [
                { "filename": "package.json", "type": "json" }
            ]
        }"#;
        let files = resolve("/project/commit-and-tag-version.config.json", source);
        assert_eq!(files, vec![abs("/project/package.json")]);
    }

    #[test]
    fn empty_config_credits_nothing() {
        let source = r#"{"header": "intro text"}"#;
        let files = resolve("/project/commit-and-tag-version.config.json", source);
        assert!(files.is_empty());
    }
}
