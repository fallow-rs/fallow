use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::UsedClassMemberRule;

/// Supported plugin file extensions.
const PLUGIN_EXTENSIONS: &[&str] = &["toml", "json", "jsonc"];

/// How a plugin's discovered entry points contribute to coverage reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum EntryPointRole {
    /// Runtime/application roots that should count toward runtime reachability.
    Runtime,
    /// Test roots that should count toward test reachability.
    Test,
    /// Support/setup/config roots that should keep files alive but not count as runtime/test.
    #[default]
    Support,
}

/// How to detect if a plugin should be activated.
///
/// When set on an `ExternalPluginDef`, this takes priority over `enablers`.
/// Supports dependency checks, file existence checks, and boolean combinators.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginDetection {
    /// Plugin detected if this package is in dependencies.
    Dependency { package: String },
    /// Plugin detected if this file pattern matches.
    FileExists { pattern: String },
    /// All conditions must be true.
    All { conditions: Vec<Self> },
    /// Any condition must be true.
    Any { conditions: Vec<Self> },
}

/// A declarative plugin definition loaded from a standalone file or inline config.
///
/// External plugins provide the same static pattern capabilities as built-in
/// plugins (entry points, always-used files, used exports, tooling dependencies),
/// but are defined in standalone files or inline in the fallow config rather than
/// compiled Rust code.
///
/// They cannot do AST-based config parsing (`resolve_config()`), but cover the
/// vast majority of framework integration use cases.
///
/// Supports JSONC, JSON, and TOML formats. All use camelCase field names.
///
/// ```json
/// {
///   "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
///   "name": "my-framework",
///   "enablers": ["my-framework", "@my-framework/core"],
///   "entryPoints": ["src/routes/**/*.{ts,tsx}"],
///   "configPatterns": ["my-framework.config.{ts,js}"],
///   "alwaysUsed": ["src/setup.ts"],
///   "toolingDependencies": ["my-framework-cli"],
///   "usedExports": [
///     { "pattern": "src/routes/**/*.{ts,tsx}", "exports": ["default", "loader", "action"] }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPluginDef {
    /// JSON Schema reference (ignored during deserialization).
    #[serde(rename = "$schema", default, skip_serializing)]
    #[schemars(skip)]
    pub schema: Option<String>,

    /// Unique name for this plugin.
    pub name: String,

    /// Rich detection logic (dependency checks, file existence, boolean combinators).
    /// Takes priority over `enablers` when set.
    #[serde(default)]
    pub detection: Option<PluginDetection>,

    /// Package names that activate this plugin when found in package.json.
    /// Supports exact matches and prefix patterns (ending with `/`).
    /// Only used when `detection` is not set.
    #[serde(default)]
    pub enablers: Vec<String>,

    /// Glob patterns for entry point files.
    #[serde(default)]
    pub entry_points: Vec<String>,

    /// Coverage role for `entryPoints`.
    ///
    /// Defaults to `support`. Set to `runtime` for application entry points
    /// or `test` for test framework entry points.
    #[serde(default = "default_external_entry_point_role")]
    pub entry_point_role: EntryPointRole,

    /// Glob patterns for config files (marked as always-used when active).
    #[serde(default)]
    pub config_patterns: Vec<String>,

    /// Files that are always considered "used" when this plugin is active.
    #[serde(default)]
    pub always_used: Vec<String>,

    /// Dependencies that are tooling (used via CLI/config, not source imports).
    /// These should not be flagged as unused devDependencies.
    #[serde(default)]
    pub tooling_dependencies: Vec<String>,

    /// Exports that are always considered used for matching file patterns.
    #[serde(default)]
    pub used_exports: Vec<ExternalUsedExport>,

    /// Class member method/property rules the framework invokes at runtime.
    /// Supports plain member names for global suppression and scoped objects
    /// with `extends` / `implements` constraints when the method name is too
    /// common to suppress across the whole workspace.
    #[serde(default)]
    pub used_class_members: Vec<UsedClassMemberRule>,
}

/// Exports considered used for files matching a pattern.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ExternalUsedExport {
    /// Glob pattern for files.
    pub pattern: String,
    /// Export names always considered used.
    pub exports: Vec<String>,
}

fn default_external_entry_point_role() -> EntryPointRole {
    EntryPointRole::Support
}

impl ExternalPluginDef {
    /// Generate JSON Schema for the external plugin format.
    #[must_use]
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(ExternalPluginDef)).unwrap_or_default()
    }
}

/// Detect plugin format from file extension.
enum PluginFormat {
    Toml,
    Json,
    Jsonc,
}

impl PluginFormat {
    fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => Some(Self::Toml),
            Some("json") => Some(Self::Json),
            Some("jsonc") => Some(Self::Jsonc),
            _ => None,
        }
    }
}

/// Check if a file has a supported plugin extension.
fn is_plugin_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| PLUGIN_EXTENSIONS.contains(&ext))
}

/// Parse a plugin definition from file content based on format.
fn parse_plugin(content: &str, format: &PluginFormat, path: &Path) -> Option<ExternalPluginDef> {
    match format {
        PluginFormat::Toml => match toml::from_str::<ExternalPluginDef>(content) {
            Ok(plugin) => Some(plugin),
            Err(e) => {
                tracing::warn!("failed to parse external plugin {}: {e}", path.display());
                None
            }
        },
        PluginFormat::Json => match serde_json::from_str::<ExternalPluginDef>(content) {
            Ok(plugin) => Some(plugin),
            Err(e) => {
                tracing::warn!("failed to parse external plugin {}: {e}", path.display());
                None
            }
        },
        PluginFormat::Jsonc => match crate::jsonc::parse_to_value::<ExternalPluginDef>(content) {
            Ok(plugin) => Some(plugin),
            Err(e) => {
                tracing::warn!("failed to parse external plugin {}: {e}", path.display());
                None
            }
        },
    }
}

/// Discover and load external plugin definitions for a project.
///
/// Discovery order (first occurrence of a plugin name wins):
/// 1. Paths from the `plugins` config field (files or directories)
/// 2. `.fallow/plugins/` directory (auto-discover `*.toml`, `*.json`, `*.jsonc` files)
/// 3. Project root `fallow-plugin-*` files (`.toml`, `.json`, `.jsonc`)
pub fn discover_external_plugins(
    root: &Path,
    config_plugin_paths: &[String],
) -> Vec<ExternalPluginDef> {
    let mut plugins = Vec::new();
    let mut seen_names = rustc_hash::FxHashSet::default();

    // All paths are checked against the canonical root to prevent symlink escapes
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    // 1. Explicit paths from config
    for path_str in config_plugin_paths {
        let path = root.join(path_str);
        if !is_within_root(&path, &canonical_root) {
            tracing::warn!("plugin path '{path_str}' resolves outside project root, skipping");
            continue;
        }
        if path.is_dir() {
            load_plugins_from_dir(&path, &canonical_root, &mut plugins, &mut seen_names);
        } else if path.is_file() {
            load_plugin_file(&path, &canonical_root, &mut plugins, &mut seen_names);
        }
    }

    // 2. .fallow/plugins/ directory
    let plugins_dir = root.join(".fallow").join("plugins");
    if plugins_dir.is_dir() && is_within_root(&plugins_dir, &canonical_root) {
        load_plugins_from_dir(&plugins_dir, &canonical_root, &mut plugins, &mut seen_names);
    }

    // 3. Project root fallow-plugin-* files (.toml, .json, .jsonc)
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut plugin_files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.starts_with("fallow-plugin-") && is_plugin_file(Path::new(n))
                    })
            })
            .collect();
        plugin_files.sort();
        for path in plugin_files {
            load_plugin_file(&path, &canonical_root, &mut plugins, &mut seen_names);
        }
    }

    plugins
}

/// Check if a path resolves within the canonical root (follows symlinks).
fn is_within_root(path: &Path, canonical_root: &Path) -> bool {
    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical.starts_with(canonical_root)
}

fn load_plugins_from_dir(
    dir: &Path,
    canonical_root: &Path,
    plugins: &mut Vec<ExternalPluginDef>,
    seen: &mut rustc_hash::FxHashSet<String>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut plugin_files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_plugin_file(p))
            .collect();
        plugin_files.sort();
        for path in plugin_files {
            load_plugin_file(&path, canonical_root, plugins, seen);
        }
    }
}

fn load_plugin_file(
    path: &Path,
    canonical_root: &Path,
    plugins: &mut Vec<ExternalPluginDef>,
    seen: &mut rustc_hash::FxHashSet<String>,
) {
    // Verify symlinks don't escape the project root
    if !is_within_root(path, canonical_root) {
        tracing::warn!(
            "plugin file '{}' resolves outside project root (symlink?), skipping",
            path.display()
        );
        return;
    }

    let Some(format) = PluginFormat::from_path(path) else {
        tracing::warn!(
            "unsupported plugin file extension for {}, expected .toml, .json, or .jsonc",
            path.display()
        );
        return;
    };

    match std::fs::read_to_string(path) {
        Ok(content) => {
            if let Some(plugin) = parse_plugin(&content, &format, path) {
                if plugin.name.is_empty() {
                    tracing::warn!(
                        "external plugin in {} has an empty name, skipping",
                        path.display()
                    );
                    return;
                }
                if seen.insert(plugin.name.clone()) {
                    plugins.push(plugin);
                } else {
                    tracing::warn!(
                        "duplicate external plugin '{}' in {}, skipping",
                        plugin.name,
                        path.display()
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "failed to read external plugin file {}: {e}",
                path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScopedUsedClassMemberRule;

    #[test]
    fn deserialize_minimal_plugin() {
        let toml_str = r#"
name = "my-plugin"
enablers = ["my-pkg"]
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert_eq!(plugin.name, "my-plugin");
        assert_eq!(plugin.enablers, vec!["my-pkg"]);
        assert!(plugin.entry_points.is_empty());
        assert!(plugin.always_used.is_empty());
        assert!(plugin.config_patterns.is_empty());
        assert!(plugin.tooling_dependencies.is_empty());
        assert!(plugin.used_exports.is_empty());
        assert!(plugin.used_class_members.is_empty());
    }

    #[test]
    fn deserialize_plugin_with_used_class_members_json() {
        let json_str = r#"{
            "name": "ag-grid",
            "enablers": ["ag-grid-angular"],
            "usedClassMembers": ["agInit", "refresh"]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(plugin.name, "ag-grid");
        assert_eq!(
            plugin.used_class_members,
            vec![
                UsedClassMemberRule::from("agInit"),
                UsedClassMemberRule::from("refresh"),
            ]
        );
    }

    #[test]
    fn deserialize_plugin_with_scoped_used_class_members_json() {
        let json_str = r#"{
            "name": "ag-grid",
            "enablers": ["ag-grid-angular"],
            "usedClassMembers": [
                "agInit",
                { "implements": "ICellRendererAngularComp", "members": ["refresh"] },
                { "extends": "BaseCommand", "members": ["execute"] }
            ]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(
            plugin.used_class_members,
            vec![
                UsedClassMemberRule::from("agInit"),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: None,
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_plugin_with_used_class_members_toml() {
        let toml_str = r#"
name = "ag-grid"
enablers = ["ag-grid-angular"]
usedClassMembers = ["agInit", "refresh"]
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert_eq!(
            plugin.used_class_members,
            vec![
                UsedClassMemberRule::from("agInit"),
                UsedClassMemberRule::from("refresh"),
            ]
        );
    }

    #[test]
    fn deserialize_plugin_with_scoped_used_class_members_toml() {
        let toml_str = r#"
name = "ag-grid"
enablers = ["ag-grid-angular"]
usedClassMembers = [
  { implements = "ICellRendererAngularComp", members = ["refresh"] },
  { extends = "BaseCommand", members = ["execute"] }
]
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert_eq!(
            plugin.used_class_members,
            vec![
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: None,
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_plugin_rejects_unconstrained_scoped_used_class_members() {
        let result = serde_json::from_str::<ExternalPluginDef>(
            r#"{
                "name": "ag-grid",
                "enablers": ["ag-grid-angular"],
                "usedClassMembers": [{ "members": ["refresh"] }]
            }"#,
        );
        assert!(
            result.is_err(),
            "unconstrained scoped rule should be rejected"
        );
    }

    #[test]
    fn deserialize_full_plugin() {
        let toml_str = r#"
name = "my-framework"
enablers = ["my-framework", "@my-framework/core"]
entryPoints = ["src/routes/**/*.{ts,tsx}", "src/middleware.ts"]
configPatterns = ["my-framework.config.{ts,js,mjs}"]
alwaysUsed = ["src/setup.ts", "public/**/*"]
toolingDependencies = ["my-framework-cli"]

[[usedExports]]
pattern = "src/routes/**/*.{ts,tsx}"
exports = ["default", "loader", "action"]

[[usedExports]]
pattern = "src/middleware.ts"
exports = ["default"]
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert_eq!(plugin.name, "my-framework");
        assert_eq!(plugin.enablers.len(), 2);
        assert_eq!(plugin.entry_points.len(), 2);
        assert_eq!(
            plugin.config_patterns,
            vec!["my-framework.config.{ts,js,mjs}"]
        );
        assert_eq!(plugin.always_used.len(), 2);
        assert_eq!(plugin.tooling_dependencies, vec!["my-framework-cli"]);
        assert_eq!(plugin.used_exports.len(), 2);
        assert_eq!(plugin.used_exports[0].pattern, "src/routes/**/*.{ts,tsx}");
        assert_eq!(
            plugin.used_exports[0].exports,
            vec!["default", "loader", "action"]
        );
    }

    #[test]
    fn deserialize_json_plugin() {
        let json_str = r#"{
            "name": "my-json-plugin",
            "enablers": ["my-pkg"],
            "entryPoints": ["src/**/*.ts"],
            "configPatterns": ["my-plugin.config.js"],
            "alwaysUsed": ["src/setup.ts"],
            "toolingDependencies": ["my-cli"],
            "usedExports": [
                { "pattern": "src/**/*.ts", "exports": ["default"] }
            ]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(plugin.name, "my-json-plugin");
        assert_eq!(plugin.enablers, vec!["my-pkg"]);
        assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
        assert_eq!(plugin.config_patterns, vec!["my-plugin.config.js"]);
        assert_eq!(plugin.always_used, vec!["src/setup.ts"]);
        assert_eq!(plugin.tooling_dependencies, vec!["my-cli"]);
        assert_eq!(plugin.used_exports.len(), 1);
        assert_eq!(plugin.used_exports[0].exports, vec!["default"]);
    }

    #[test]
    fn deserialize_jsonc_plugin() {
        let jsonc_str = r#"{
            // This is a JSONC plugin
            "name": "my-jsonc-plugin",
            "enablers": ["my-pkg"],
            /* Block comment */
            "entryPoints": ["src/**/*.ts"]
        }"#;
        let plugin: ExternalPluginDef = crate::jsonc::parse_to_value(jsonc_str).unwrap();
        assert_eq!(plugin.name, "my-jsonc-plugin");
        assert_eq!(plugin.enablers, vec!["my-pkg"]);
        assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
    }

    #[test]
    fn deserialize_json_with_schema_field() {
        let json_str = r#"{
            "$schema": "https://fallow.dev/plugin-schema.json",
            "name": "schema-plugin",
            "enablers": ["my-pkg"]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(plugin.name, "schema-plugin");
        assert_eq!(plugin.enablers, vec!["my-pkg"]);
    }

    #[test]
    fn plugin_json_schema_generation() {
        let schema = ExternalPluginDef::json_schema();
        assert!(schema.is_object());
        let obj = schema.as_object().unwrap();
        assert!(obj.contains_key("properties"));
    }

    #[test]
    fn discover_plugins_from_fallow_plugins_dir() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-ext-plugins-{}", std::process::id()));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        std::fs::write(
            plugins_dir.join("my-plugin.toml"),
            r#"
name = "my-plugin"
enablers = ["my-pkg"]
entryPoints = ["src/**/*.ts"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "my-plugin");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_json_plugins_from_fallow_plugins_dir() {
        let dir = std::env::temp_dir().join(format!(
            "fallow-test-ext-json-plugins-{}",
            std::process::id()
        ));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        std::fs::write(
            plugins_dir.join("my-plugin.json"),
            r#"{"name": "json-plugin", "enablers": ["json-pkg"]}"#,
        )
        .unwrap();

        std::fs::write(
            plugins_dir.join("my-plugin.jsonc"),
            r#"{
                // JSONC plugin
                "name": "jsonc-plugin",
                "enablers": ["jsonc-pkg"]
            }"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 2);
        // Sorted: json before jsonc
        assert_eq!(plugins[0].name, "json-plugin");
        assert_eq!(plugins[1].name, "jsonc-plugin");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_fallow_plugin_files_in_root() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-root-plugins-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("fallow-plugin-custom.toml"),
            r#"
name = "custom"
enablers = ["custom-pkg"]
"#,
        )
        .unwrap();

        // Non-matching file should be ignored
        std::fs::write(dir.join("some-other-file.toml"), r#"name = "ignored""#).unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "custom");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_fallow_plugin_json_files_in_root() {
        let dir = std::env::temp_dir().join(format!(
            "fallow-test-root-json-plugins-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("fallow-plugin-custom.json"),
            r#"{"name": "json-root", "enablers": ["json-pkg"]}"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("fallow-plugin-custom2.jsonc"),
            r#"{
                // JSONC root plugin
                "name": "jsonc-root",
                "enablers": ["jsonc-pkg"]
            }"#,
        )
        .unwrap();

        // Non-matching extension should be ignored
        std::fs::write(
            dir.join("fallow-plugin-bad.yaml"),
            "name: ignored\nenablers:\n  - pkg\n",
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_mixed_formats_in_dir() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-mixed-plugins-{}", std::process::id()));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        std::fs::write(
            plugins_dir.join("a-plugin.toml"),
            r#"
name = "toml-plugin"
enablers = ["toml-pkg"]
"#,
        )
        .unwrap();

        std::fs::write(
            plugins_dir.join("b-plugin.json"),
            r#"{"name": "json-plugin", "enablers": ["json-pkg"]}"#,
        )
        .unwrap();

        std::fs::write(
            plugins_dir.join("c-plugin.jsonc"),
            r#"{
                // JSONC plugin
                "name": "jsonc-plugin",
                "enablers": ["jsonc-pkg"]
            }"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 3);
        assert_eq!(plugins[0].name, "toml-plugin");
        assert_eq!(plugins[1].name, "json-plugin");
        assert_eq!(plugins[2].name, "jsonc-plugin");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deduplicates_by_name() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-dedup-plugins-{}", std::process::id()));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        // Same name in .fallow/plugins/ and root
        std::fs::write(
            plugins_dir.join("my-plugin.toml"),
            r#"
name = "my-plugin"
enablers = ["pkg-a"]
"#,
        )
        .unwrap();

        std::fs::write(
            dir.join("fallow-plugin-my-plugin.toml"),
            r#"
name = "my-plugin"
enablers = ["pkg-b"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 1);
        // First one wins (.fallow/plugins/ before root)
        assert_eq!(plugins[0].enablers, vec!["pkg-a"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_plugin_paths_take_priority() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-config-paths-{}", std::process::id()));
        let custom_dir = dir.join("custom-plugins");
        let _ = std::fs::create_dir_all(&custom_dir);

        std::fs::write(
            custom_dir.join("explicit.toml"),
            r#"
name = "explicit"
enablers = ["explicit-pkg"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &["custom-plugins".to_string()]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "explicit");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_plugin_path_to_single_file() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-single-file-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("my-plugin.toml"),
            r#"
name = "single-file"
enablers = ["single-pkg"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &["my-plugin.toml".to_string()]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "single-file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_plugin_path_to_single_json_file() {
        let dir = std::env::temp_dir().join(format!(
            "fallow-test-single-json-file-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(
            dir.join("my-plugin.json"),
            r#"{"name": "json-single", "enablers": ["json-pkg"]}"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &["my-plugin.json".to_string()]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "json-single");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_invalid_toml() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-invalid-plugin-{}", std::process::id()));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        // Invalid: missing required `name` field
        std::fs::write(plugins_dir.join("bad.toml"), r#"enablers = ["pkg"]"#).unwrap();

        // Valid
        std::fs::write(
            plugins_dir.join("good.toml"),
            r#"
name = "good"
enablers = ["good-pkg"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "good");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_invalid_json() {
        let dir = std::env::temp_dir().join(format!(
            "fallow-test-invalid-json-plugin-{}",
            std::process::id()
        ));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        // Invalid JSON: missing name
        std::fs::write(plugins_dir.join("bad.json"), r#"{"enablers": ["pkg"]}"#).unwrap();

        // Valid JSON
        std::fs::write(
            plugins_dir.join("good.json"),
            r#"{"name": "good-json", "enablers": ["good-pkg"]}"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "good-json");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefix_enablers() {
        let toml_str = r#"
name = "scoped"
enablers = ["@myorg/"]
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert_eq!(plugin.enablers, vec!["@myorg/"]);
    }

    #[test]
    fn skips_empty_name() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-empty-name-{}", std::process::id()));
        let plugins_dir = dir.join(".fallow").join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);

        std::fs::write(
            plugins_dir.join("empty.toml"),
            r#"
name = ""
enablers = ["pkg"]
"#,
        )
        .unwrap();

        let plugins = discover_external_plugins(&dir, &[]);
        assert!(plugins.is_empty(), "empty-name plugin should be skipped");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_paths_outside_root() {
        let dir =
            std::env::temp_dir().join(format!("fallow-test-path-escape-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        // Attempt to load a plugin from outside the project root
        let plugins = discover_external_plugins(&dir, &["../../../etc".to_string()]);
        assert!(plugins.is_empty(), "paths outside root should be rejected");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plugin_format_detection() {
        assert!(matches!(
            PluginFormat::from_path(Path::new("plugin.toml")),
            Some(PluginFormat::Toml)
        ));
        assert!(matches!(
            PluginFormat::from_path(Path::new("plugin.json")),
            Some(PluginFormat::Json)
        ));
        assert!(matches!(
            PluginFormat::from_path(Path::new("plugin.jsonc")),
            Some(PluginFormat::Jsonc)
        ));
        assert!(PluginFormat::from_path(Path::new("plugin.yaml")).is_none());
        assert!(PluginFormat::from_path(Path::new("plugin")).is_none());
    }

    #[test]
    fn is_plugin_file_checks_extensions() {
        assert!(is_plugin_file(Path::new("plugin.toml")));
        assert!(is_plugin_file(Path::new("plugin.json")));
        assert!(is_plugin_file(Path::new("plugin.jsonc")));
        assert!(!is_plugin_file(Path::new("plugin.yaml")));
        assert!(!is_plugin_file(Path::new("plugin.txt")));
        assert!(!is_plugin_file(Path::new("plugin")));
    }

    // ── PluginDetection tests ────────────────────────────────────

    #[test]
    fn detection_deserialize_dependency() {
        let json = r#"{"type": "dependency", "package": "next"}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(matches!(detection, PluginDetection::Dependency { package } if package == "next"));
    }

    #[test]
    fn detection_deserialize_file_exists() {
        let json = r#"{"type": "fileExists", "pattern": "tsconfig.json"}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(
            matches!(detection, PluginDetection::FileExists { pattern } if pattern == "tsconfig.json")
        );
    }

    #[test]
    fn detection_deserialize_all() {
        let json = r#"{"type": "all", "conditions": [{"type": "dependency", "package": "a"}, {"type": "dependency", "package": "b"}]}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(matches!(detection, PluginDetection::All { conditions } if conditions.len() == 2));
    }

    #[test]
    fn detection_deserialize_any() {
        let json = r#"{"type": "any", "conditions": [{"type": "dependency", "package": "a"}]}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(matches!(detection, PluginDetection::Any { conditions } if conditions.len() == 1));
    }

    #[test]
    fn plugin_with_detection_field() {
        let json = r#"{
            "name": "my-plugin",
            "detection": {"type": "dependency", "package": "my-pkg"},
            "entryPoints": ["src/**/*.ts"]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
        assert_eq!(plugin.name, "my-plugin");
        assert!(plugin.detection.is_some());
        assert!(plugin.enablers.is_empty());
        assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
    }

    #[test]
    fn plugin_without_detection_uses_enablers() {
        let json = r#"{
            "name": "my-plugin",
            "enablers": ["my-pkg"]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
        assert!(plugin.detection.is_none());
        assert_eq!(plugin.enablers, vec!["my-pkg"]);
    }

    // ── Nested detection combinators ────────────────────────────────

    #[test]
    fn detection_nested_all_with_any() {
        let json = r#"{
            "type": "all",
            "conditions": [
                {"type": "dependency", "package": "react"},
                {"type": "any", "conditions": [
                    {"type": "fileExists", "pattern": "next.config.js"},
                    {"type": "fileExists", "pattern": "next.config.mjs"}
                ]}
            ]
        }"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        match detection {
            PluginDetection::All { conditions } => {
                assert_eq!(conditions.len(), 2);
                assert!(matches!(
                    &conditions[0],
                    PluginDetection::Dependency { package } if package == "react"
                ));
                match &conditions[1] {
                    PluginDetection::Any { conditions: inner } => {
                        assert_eq!(inner.len(), 2);
                    }
                    other => panic!("expected Any, got: {other:?}"),
                }
            }
            other => panic!("expected All, got: {other:?}"),
        }
    }

    #[test]
    fn detection_empty_all_conditions() {
        let json = r#"{"type": "all", "conditions": []}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(matches!(
            detection,
            PluginDetection::All { conditions } if conditions.is_empty()
        ));
    }

    #[test]
    fn detection_empty_any_conditions() {
        let json = r#"{"type": "any", "conditions": []}"#;
        let detection: PluginDetection = serde_json::from_str(json).unwrap();
        assert!(matches!(
            detection,
            PluginDetection::Any { conditions } if conditions.is_empty()
        ));
    }

    // ── TOML with detection field ───────────────────────────────────

    #[test]
    fn detection_toml_dependency() {
        let toml_str = r#"
name = "my-plugin"

[detection]
type = "dependency"
package = "next"
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert!(plugin.detection.is_some());
        assert!(matches!(
            plugin.detection.unwrap(),
            PluginDetection::Dependency { package } if package == "next"
        ));
    }

    #[test]
    fn detection_toml_file_exists() {
        let toml_str = r#"
name = "my-plugin"

[detection]
type = "fileExists"
pattern = "next.config.js"
"#;
        let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            plugin.detection.unwrap(),
            PluginDetection::FileExists { pattern } if pattern == "next.config.js"
        ));
    }

    // ── Plugin with all fields set ──────────────────────────────────

    #[test]
    fn plugin_all_fields_json() {
        let json = r#"{
            "$schema": "https://fallow.dev/plugin-schema.json",
            "name": "full-plugin",
            "detection": {"type": "dependency", "package": "my-pkg"},
            "enablers": ["fallback-enabler"],
            "entryPoints": ["src/entry.ts"],
            "configPatterns": ["config.js"],
            "alwaysUsed": ["src/polyfills.ts"],
            "toolingDependencies": ["my-cli"],
            "usedExports": [{"pattern": "src/**", "exports": ["default", "setup"]}]
        }"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
        assert_eq!(plugin.name, "full-plugin");
        assert!(plugin.detection.is_some());
        assert_eq!(plugin.enablers, vec!["fallback-enabler"]);
        assert_eq!(plugin.entry_points, vec!["src/entry.ts"]);
        assert_eq!(plugin.config_patterns, vec!["config.js"]);
        assert_eq!(plugin.always_used, vec!["src/polyfills.ts"]);
        assert_eq!(plugin.tooling_dependencies, vec!["my-cli"]);
        assert_eq!(plugin.used_exports.len(), 1);
        assert_eq!(plugin.used_exports[0].pattern, "src/**");
        assert_eq!(plugin.used_exports[0].exports, vec!["default", "setup"]);
    }

    // ── Plugin name validation edge case ────────────────────────────

    #[test]
    fn plugin_with_special_chars_in_name() {
        let json = r#"{"name": "@scope/my-plugin-v2.0", "enablers": ["pkg"]}"#;
        let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
        assert_eq!(plugin.name, "@scope/my-plugin-v2.0");
    }

    // ── parse_plugin with various formats ───────────────────────────

    #[test]
    fn parse_plugin_toml_format() {
        let content = r#"
name = "test-plugin"
enablers = ["test-pkg"]
entryPoints = ["src/**/*.ts"]
"#;
        let result = parse_plugin(content, &PluginFormat::Toml, Path::new("test.toml"));
        assert!(result.is_some());
        let plugin = result.unwrap();
        assert_eq!(plugin.name, "test-plugin");
    }

    #[test]
    fn parse_plugin_json_format() {
        let content = r#"{"name": "json-test", "enablers": ["pkg"]}"#;
        let result = parse_plugin(content, &PluginFormat::Json, Path::new("test.json"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "json-test");
    }

    #[test]
    fn parse_plugin_jsonc_format() {
        let content = r#"{
            // A comment
            "name": "jsonc-test",
            "enablers": ["pkg"]
        }"#;
        let result = parse_plugin(content, &PluginFormat::Jsonc, Path::new("test.jsonc"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "jsonc-test");
    }

    #[test]
    fn parse_plugin_invalid_toml_returns_none() {
        let content = "not valid toml [[[";
        let result = parse_plugin(content, &PluginFormat::Toml, Path::new("bad.toml"));
        assert!(result.is_none());
    }

    #[test]
    fn parse_plugin_invalid_json_returns_none() {
        let content = "{ not valid json }";
        let result = parse_plugin(content, &PluginFormat::Json, Path::new("bad.json"));
        assert!(result.is_none());
    }

    #[test]
    fn parse_plugin_invalid_jsonc_returns_none() {
        // Missing required `name` field
        let content = r#"{"enablers": ["pkg"]}"#;
        let result = parse_plugin(content, &PluginFormat::Jsonc, Path::new("bad.jsonc"));
        assert!(result.is_none());
    }
}
