//! release-it plugin.
//!
//! Detects release-it projects and marks config files as always used.
//! release-it loads each key of the `plugins` object at runtime, as a package
//! or as a local module, and it loads an `extends` value as a shared config.
//! These references have no import in the project, so the plugin credits them.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["release-it"];

// The YAML and TOML forms stay activation-only: the extractor is a JS/JSON
// parser. JSON5 is also not parsed, because the parser reads it as a script.
const CONFIG_PATTERNS: &[&str] = &[".release-it.{json,jsonc,js,cjs,mjs,ts,cts,mts}"];

const ALWAYS_USED: &[&str] =
    &[".release-it.{json,jsonc,json5,js,cjs,mjs,ts,cts,mts,yaml,yml,toml}"];

const TOOLING_DEPENDENCIES: &[&str] = &["release-it"];

define_plugin! {
    struct ReleaseItPlugin => "release-it",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "release-it",
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let plugins = config_parser::extract_config_object_keys(source, config_path, &["plugins"]);
        let extends =
            config_parser::extract_config_string_or_array(source, config_path, &["extends"]);
        for specifier in plugins.iter().chain(&extends) {
            credit_specifier(specifier, config_path, root, &mut result);
        }

        result
    }
}

/// Credit one module that release-it loads from its config.
///
/// A package specifier credits the package. A relative path names a local
/// module, so the plugin keeps that file reachable. A value with a scheme,
/// such as `github:owner/repo`, names neither and is skipped.
fn credit_specifier(specifier: &str, config_path: &Path, root: &Path, result: &mut PluginResult) {
    let specifier = specifier.trim();
    if config_parser::is_package_specifier(specifier) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(specifier));
        return;
    }
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return;
    }
    if let Some(relative) =
        config_parser::normalize_filesystem_config_path_buf(specifier, config_path, root)
    {
        result.setup_files.push(root.join(relative));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Build a platform-absolute path from a logical `/`-rooted string, so
    /// path resolution behaves the same on Windows.
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

    fn resolve(config_path: &str, source: &str) -> PluginResult {
        ReleaseItPlugin.resolve_config(&abs(config_path), source, &abs("/project"))
    }

    #[test]
    fn credits_plugin_keys_as_dependencies() {
        let source = r#"
            export default {
                git: { requireCleanWorkingDir: false },
                plugins: {
                    "@release-it/conventional-changelog": { preset: "angular" },
                    "release-it-sample-plugin": {},
                    "@scope/release-plugin/subpath": {}
                }
            };
        "#;
        let result = resolve("/project/.release-it.mjs", source);
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@release-it/conventional-changelog".to_string()));
        assert!(deps.contains(&"release-it-sample-plugin".to_string()));
        assert!(deps.contains(&"@scope/release-plugin".to_string()));
    }

    #[test]
    fn credits_plugin_keys_from_json_config() {
        let source = r#"{
            "plugins": { "release-it-sample-plugin": { "enabled": true } }
        }"#;
        let result = resolve("/project/.release-it.json", source);
        assert_eq!(
            result.referenced_dependencies,
            vec!["release-it-sample-plugin".to_string()]
        );
    }

    #[test]
    fn keeps_relative_plugin_keys_as_files() {
        let source = r#"
            module.exports = {
                plugins: {
                    "./scripts/release-plugin.js": {},
                    "../shared/plugin.mjs": {}
                }
            };
        "#;
        let result = resolve("/project/packages/app/.release-it.cjs", source);
        assert!(result.referenced_dependencies.is_empty());
        assert!(
            result
                .setup_files
                .contains(&abs("/project/packages/app/scripts/release-plugin.js"))
        );
        assert!(
            result
                .setup_files
                .contains(&abs("/project/packages/shared/plugin.mjs"))
        );
    }

    #[test]
    fn credits_extends_package_and_skips_remote_source() {
        let package = resolve(
            "/project/.release-it.json",
            r#"{ "extends": "release-it-shared-config" }"#,
        );
        assert_eq!(
            package.referenced_dependencies,
            vec!["release-it-shared-config".to_string()]
        );

        let remote = resolve(
            "/project/.release-it.json",
            r#"{ "extends": "github:owner/release-config" }"#,
        );
        assert!(remote.referenced_dependencies.is_empty());
        assert!(remote.setup_files.is_empty());
    }

    #[test]
    fn credits_config_imports() {
        let source = r#"
            import { helper } from "release-config-helper";
            export default { plugins: {} };
        "#;
        let result = resolve("/project/.release-it.ts", source);
        assert_eq!(
            result.referenced_dependencies,
            vec!["release-config-helper".to_string()]
        );
    }

    #[test]
    fn config_without_plugins_credits_nothing() {
        let result = resolve("/project/.release-it.json", r#"{ "git": { "tag": true } }"#);
        assert!(result.referenced_dependencies.is_empty());
        assert!(result.setup_files.is_empty());
    }

    #[test]
    fn reads_package_json_config_key() {
        assert_eq!(
            ReleaseItPlugin.package_json_config_key(),
            Some("release-it")
        );
    }
}
