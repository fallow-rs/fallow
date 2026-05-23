use serde::{Deserialize, Deserializer, Serialize};

/// Type alias for standard `HashMap` used in serde-deserialized structs.
/// `rustc-hash` v2 does not have a `serde` feature, so fields deserialized
/// from JSON must use `std::collections::HashMap`.
#[expect(
    clippy::disallowed_types,
    reason = "rustc-hash v2 lacks serde feature — JSON deserialization needs std HashMap"
)]
type StdHashMap<K, V> = std::collections::HashMap<K, V>;

fn deserialize_optional_bool_lenient<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(value)) => Some(value),
        _ => None,
    })
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PeerDependencyMeta {
    #[serde(default)]
    pub optional: bool,
}

/// Parsed package.json with fields relevant to fallow.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PackageJson {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_bool_lenient")]
    pub private: Option<bool>,
    #[serde(default)]
    pub main: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub types: Option<String>,
    #[serde(default)]
    pub typings: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub browser: Option<serde_json::Value>,
    #[serde(default)]
    pub bin: Option<serde_json::Value>,
    #[serde(default)]
    pub exports: Option<serde_json::Value>,
    #[serde(default)]
    pub imports: Option<serde_json::Value>,
    #[serde(default)]
    pub dependencies: Option<StdHashMap<String, String>>,
    #[serde(default, rename = "devDependencies")]
    pub dev_dependencies: Option<StdHashMap<String, String>>,
    #[serde(default, rename = "peerDependencies")]
    pub peer_dependencies: Option<StdHashMap<String, String>>,
    #[serde(default, rename = "peerDependenciesMeta")]
    pub peer_dependencies_meta: Option<StdHashMap<String, PeerDependencyMeta>>,
    #[serde(default, rename = "optionalDependencies")]
    pub optional_dependencies: Option<StdHashMap<String, String>>,
    #[serde(default)]
    pub scripts: Option<StdHashMap<String, String>>,
    #[serde(default)]
    pub workspaces: Option<serde_json::Value>,
}

impl PackageJson {
    /// Load from a package.json file.
    ///
    /// # Errors
    ///
    /// Returns an error string when the file cannot be read or parsed as JSON.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        // Strip UTF-8 BOM if present (common in Windows-authored
        // package.json files, and a deliberate vite test fixture).
        // `parse_tsconfig_references` already does the same; without this
        // symmetric step, a BOM-prefixed package.json surfaces as a
        // false-positive `malformed-package-json` diagnostic on workspaces
        // that pnpm/npm/yarn happily install.
        let content = content.trim_start_matches('\u{FEFF}');
        serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }

    /// Get all dependency names (production + dev + peer + optional).
    #[must_use]
    pub fn all_dependency_names(&self) -> Vec<String> {
        let mut deps = Vec::new();
        if let Some(d) = &self.dependencies {
            deps.extend(d.keys().cloned());
        }
        if let Some(d) = &self.dev_dependencies {
            deps.extend(d.keys().cloned());
        }
        if let Some(d) = &self.peer_dependencies {
            deps.extend(d.keys().cloned());
        }
        if let Some(d) = &self.optional_dependencies {
            deps.extend(d.keys().cloned());
        }
        deps
    }

    /// Get production dependency names only.
    #[must_use]
    pub fn production_dependency_names(&self) -> Vec<String> {
        self.dependencies
            .as_ref()
            .map(|d| d.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get dev dependency names only.
    #[must_use]
    pub fn dev_dependency_names(&self) -> Vec<String> {
        self.dev_dependencies
            .as_ref()
            .map(|d| d.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get optional dependency names only.
    #[must_use]
    pub fn optional_dependency_names(&self) -> Vec<String> {
        self.optional_dependencies
            .as_ref()
            .map(|d| d.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get required peer dependency names only.
    #[must_use]
    pub fn required_peer_dependency_names(&self) -> Vec<String> {
        self.peer_dependencies
            .as_ref()
            .map(|deps| {
                deps.keys()
                    .filter(|dep| !self.peer_dependency_is_optional(dep))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn peer_dependency_is_optional(&self, dep: &str) -> bool {
        self.peer_dependencies_meta
            .as_ref()
            .and_then(|meta| meta.get(dep))
            .is_some_and(|meta| meta.optional)
    }

    /// Extract entry points from package.json fields.
    #[must_use]
    pub fn entry_points(&self) -> Vec<String> {
        let mut entries = Vec::new();

        if let Some(main) = &self.main {
            entries.push(main.clone());
        }
        if let Some(module) = &self.module {
            entries.push(module.clone());
        }
        if let Some(types) = &self.types {
            entries.push(types.clone());
        }
        if let Some(typings) = &self.typings {
            entries.push(typings.clone());
        }
        if let Some(source) = &self.source {
            entries.push(source.clone());
        }

        // Handle browser field (string or object with path values)
        if let Some(browser) = &self.browser {
            match browser {
                serde_json::Value::String(s) => entries.push(s.clone()),
                serde_json::Value::Object(map) => {
                    for v in map.values() {
                        if let serde_json::Value::String(s) = v
                            && (s.starts_with("./") || s.starts_with("../"))
                        {
                            entries.push(s.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle bin field (string or object)
        if let Some(bin) = &self.bin {
            match bin {
                serde_json::Value::String(s) => entries.push(s.clone()),
                serde_json::Value::Object(map) => {
                    for v in map.values() {
                        if let serde_json::Value::String(s) = v {
                            entries.push(s.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle exports field (recursive)
        if let Some(exports) = &self.exports {
            extract_exports_entries(exports, &mut entries);
        }

        entries
    }

    /// Extract unique subdirectory names referenced by the `exports` field keys.
    ///
    /// For exports like `"./compat": { ... }`, `"./hooks/client": { ... }`,
    /// this returns `["compat", "hooks"]`. Used to discover sub-packages in
    /// projects that define their internal package structure via the exports map
    /// (e.g., preact with `compat/`, `hooks/`, `debug/` sub-packages).
    #[must_use]
    pub fn exports_subdirectories(&self) -> Vec<String> {
        self.exports
            .as_ref()
            .map_or_else(Vec::new, extract_exports_subdirectories)
    }

    /// Extract workspace patterns from package.json.
    #[must_use]
    pub fn workspace_patterns(&self) -> Vec<String> {
        match &self.workspaces {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::Object(obj)) => obj
                .get("packages")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

/// Extract unique subdirectory names referenced by the `exports` field keys.
///
/// For exports like `"./compat": { ... }`, `"./hooks/client": { ... }`,
/// this returns `["compat", "hooks"]`. Used to discover sub-packages in
/// projects that use the exports map to define their internal package structure
/// (e.g., preact with `compat/`, `hooks/`, `debug/` sub-packages).
fn extract_exports_subdirectories(exports: &serde_json::Value) -> Vec<String> {
    let Some(map) = exports.as_object() else {
        return Vec::new();
    };

    let skip_dirs = ["dist", "build", "out", "esm", "cjs", "lib", "node_modules"];
    let mut dirs = rustc_hash::FxHashSet::default();

    for key in map.keys() {
        // Keys are like "./compat", "./hooks/client", "."
        let stripped = key.strip_prefix("./").unwrap_or(key);
        if let Some(first_segment) = stripped.split('/').next()
            && !first_segment.is_empty()
            && first_segment != "."
            && first_segment != "package.json"
            && !skip_dirs.contains(&first_segment)
        {
            dirs.insert(first_segment.to_owned());
        }
    }

    dirs.into_iter().collect()
}

/// Recursively extract file paths from package.json exports field.
fn extract_exports_entries(value: &serde_json::Value, entries: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) if s.starts_with("./") || s.starts_with("../") => {
            entries.push(s.clone());
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                extract_exports_entries(v, entries);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                extract_exports_entries(v, entries);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_json_workspace_patterns_array() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"workspaces": ["packages/*", "apps/*"]}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn package_json_workspace_patterns_object() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"workspaces": {"packages": ["packages/*"]}}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert_eq!(patterns, vec!["packages/*"]);
    }

    #[test]
    fn package_json_workspace_patterns_none() {
        let pkg: PackageJson = serde_json::from_str(r#"{"name": "test"}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert!(patterns.is_empty());
    }

    #[test]
    fn package_json_workspace_patterns_empty_array() {
        let pkg: PackageJson = serde_json::from_str(r#"{"workspaces": []}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert!(patterns.is_empty());
    }

    #[test]
    fn package_json_load_valid() {
        let temp_dir = std::env::temp_dir().join("fallow-test-pkg-json");
        let _ = std::fs::create_dir_all(&temp_dir);
        let pkg_path = temp_dir.join("package.json");
        std::fs::write(&pkg_path, r#"{"name": "test", "main": "index.js"}"#).unwrap();

        let pkg = PackageJson::load(&pkg_path).unwrap();
        assert_eq!(pkg.name, Some("test".to_string()));
        assert_eq!(pkg.main, Some("index.js".to_string()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn package_json_private_non_bool_values_are_ignored() {
        for raw in [
            r#"{"private": "true"}"#,
            r#"{"private": 1}"#,
            r#"{"private": null}"#,
        ] {
            let pkg: PackageJson = serde_json::from_str(raw).unwrap();
            assert_eq!(pkg.private, None);
        }

        let pkg: PackageJson = serde_json::from_str(r#"{"private": true}"#).unwrap();
        assert_eq!(pkg.private, Some(true));

        let pkg: PackageJson = serde_json::from_str(r#"{"private": false}"#).unwrap();
        assert_eq!(pkg.private, Some(false));
    }

    #[test]
    fn package_json_load_missing_file() {
        let result = PackageJson::load(std::path::Path::new("/nonexistent/package.json"));
        assert!(result.is_err());
    }

    #[test]
    fn package_json_entry_points_combined() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "main": "dist/index.js",
            "module": "dist/index.mjs",
            "types": "dist/index.d.ts",
            "typings": "dist/types.d.ts"
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert_eq!(entries.len(), 4);
        assert!(entries.contains(&"dist/index.js".to_string()));
        assert!(entries.contains(&"dist/index.mjs".to_string()));
        assert!(entries.contains(&"dist/index.d.ts".to_string()));
        assert!(entries.contains(&"dist/types.d.ts".to_string()));
    }

    #[test]
    fn package_json_exports_nested() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                ".": {
                    "import": "./dist/index.mjs",
                    "require": "./dist/index.cjs"
                },
                "./utils": {
                    "import": "./dist/utils.mjs"
                }
            }
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./dist/index.mjs".to_string()));
        assert!(entries.contains(&"./dist/index.cjs".to_string()));
        assert!(entries.contains(&"./dist/utils.mjs".to_string()));
    }

    #[test]
    fn package_json_exports_array() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                ".": ["./dist/index.mjs", "./dist/index.cjs"]
            }
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./dist/index.mjs".to_string()));
        assert!(entries.contains(&"./dist/index.cjs".to_string()));
    }

    #[test]
    fn extract_exports_ignores_non_relative() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                ".": "not-a-relative-path"
            }
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        // "not-a-relative-path" doesn't start with "./" so should be excluded
        assert!(entries.is_empty());
    }

    #[test]
    fn package_json_source_field() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "main": "dist/index.js",
            "source": "src/index.ts"
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"src/index.ts".to_string()));
        assert!(entries.contains(&"dist/index.js".to_string()));
    }

    #[test]
    fn package_json_browser_field_string() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "browser": "./dist/browser.js"
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./dist/browser.js".to_string()));
    }

    #[test]
    fn package_json_browser_field_object() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "browser": {
                "./server.js": "./browser.js",
                "module-name": false
            }
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./browser.js".to_string()));
        // non-relative paths and false values should be excluded
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn package_json_exports_string() {
        let pkg: PackageJson = serde_json::from_str(r#"{"exports": "./dist/index.js"}"#).unwrap();
        let entries = pkg.entry_points();
        assert_eq!(entries, vec!["./dist/index.js"]);
    }

    #[test]
    fn package_json_workspace_patterns_object_with_nohoist() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "workspaces": {
                "packages": ["packages/*", "apps/*"],
                "nohoist": ["**/react-native"]
            }
        }"#,
        )
        .unwrap();
        let patterns = pkg.workspace_patterns();
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn package_json_missing_optional_fields() {
        let pkg: PackageJson = serde_json::from_str(r"{}").unwrap();
        assert!(pkg.name.is_none());
        assert!(pkg.main.is_none());
        assert!(pkg.module.is_none());
        assert!(pkg.types.is_none());
        assert!(pkg.typings.is_none());
        assert!(pkg.source.is_none());
        assert!(pkg.browser.is_none());
        assert!(pkg.bin.is_none());
        assert!(pkg.exports.is_none());
        assert!(pkg.dependencies.is_none());
        assert!(pkg.dev_dependencies.is_none());
        assert!(pkg.peer_dependencies.is_none());
        assert!(pkg.peer_dependencies_meta.is_none());
        assert!(pkg.optional_dependencies.is_none());
        assert!(pkg.scripts.is_none());
        assert!(pkg.workspaces.is_none());
        assert!(pkg.entry_points().is_empty());
        assert!(pkg.workspace_patterns().is_empty());
        assert!(pkg.all_dependency_names().is_empty());
    }

    #[test]
    fn package_json_all_dependency_names() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "dependencies": {"react": "^18", "react-dom": "^18"},
            "devDependencies": {"typescript": "^5"},
            "peerDependencies": {"node": ">=18"},
            "optionalDependencies": {"fsevents": "^2"}
        }"#,
        )
        .unwrap();
        let deps = pkg.all_dependency_names();
        assert_eq!(deps.len(), 5);
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"react-dom".to_string()));
        assert!(deps.contains(&"typescript".to_string()));
        assert!(deps.contains(&"node".to_string()));
        assert!(deps.contains(&"fsevents".to_string()));
    }

    #[test]
    fn package_json_production_dependency_names() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "dependencies": {"react": "^18"},
            "devDependencies": {"typescript": "^5"}
        }"#,
        )
        .unwrap();
        let prod = pkg.production_dependency_names();
        assert_eq!(prod, vec!["react"]);
        let dev = pkg.dev_dependency_names();
        assert_eq!(dev, vec!["typescript"]);
    }

    #[test]
    fn package_json_bin_field_string() {
        let pkg: PackageJson = serde_json::from_str(r#"{"bin": "./cli.js"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./cli.js".to_string()));
    }

    #[test]
    fn package_json_bin_field_object() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{"bin": {"my-cli": "./bin/cli.js", "my-tool": "./bin/tool.js"}}"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./bin/cli.js".to_string()));
        assert!(entries.contains(&"./bin/tool.js".to_string()));
    }

    #[test]
    fn package_json_exports_deeply_nested() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                ".": {
                    "node": {
                        "import": "./dist/node.mjs",
                        "require": "./dist/node.cjs"
                    },
                    "browser": {
                        "import": "./dist/browser.mjs"
                    }
                }
            }
        }"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert_eq!(entries.len(), 3);
        assert!(entries.contains(&"./dist/node.mjs".to_string()));
        assert!(entries.contains(&"./dist/node.cjs".to_string()));
        assert!(entries.contains(&"./dist/browser.mjs".to_string()));
    }

    // ── Peer dependency names ───────────────────────────────────────

    #[test]
    fn package_json_peer_deps_only() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"peerDependencies": {"react": "^18", "react-dom": "^18"}}"#)
                .unwrap();
        let all = pkg.all_dependency_names();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&"react".to_string()));
        assert!(all.contains(&"react-dom".to_string()));

        // No production or dev deps
        assert!(pkg.production_dependency_names().is_empty());
        assert!(pkg.dev_dependency_names().is_empty());
    }

    #[test]
    fn package_json_required_peer_dependency_names_excludes_optional_peers() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "peerDependencies": {"react": "^18", "typescript": "^5"},
            "peerDependenciesMeta": {"typescript": {"optional": true}}
        }"#,
        )
        .unwrap();
        assert_eq!(pkg.required_peer_dependency_names(), vec!["react"]);
    }

    // ── Optional dependencies ───────────────────────────────────────

    #[test]
    fn package_json_optional_deps_in_all_names() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"optionalDependencies": {"fsevents": "^2"}}"#).unwrap();
        let all = pkg.all_dependency_names();
        assert!(all.contains(&"fsevents".to_string()));
    }

    // ── Browser field edge cases ────────────────────────────────────

    #[test]
    fn package_json_browser_array_ignored() {
        // Browser field as array is not supported -- should not crash
        let pkg: PackageJson =
            serde_json::from_str(r#"{"browser": ["./a.js", "./b.js"]}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(
            entries.is_empty(),
            "array browser field should not produce entries"
        );
    }

    #[test]
    fn package_json_browser_object_non_relative_skipped() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{"browser": {"crypto": false, "./local.js": "./browser-local.js"}}"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        // false is not a string, "crypto" is not relative
        // only "./browser-local.js" starts with "./"
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"./browser-local.js".to_string()));
    }

    // ── Exports field edge cases ────────────────────────────────────

    #[test]
    fn package_json_exports_null_value() {
        // Some packages use null for subpath exclusions
        let pkg: PackageJson =
            serde_json::from_str(r#"{"exports": {".": "./dist/index.js", "./internal": null}}"#)
                .unwrap();
        let entries = pkg.entry_points();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"./dist/index.js".to_string()));
    }

    #[test]
    fn package_json_exports_empty_object() {
        let pkg: PackageJson = serde_json::from_str(r#"{"exports": {}}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.is_empty());
    }

    // ── Workspace patterns edge cases ───────────────────────────────

    #[test]
    fn package_json_workspace_patterns_string_value_ignored() {
        // workspaces as a string is not a valid format
        let pkg: PackageJson = serde_json::from_str(r#"{"workspaces": "packages/*"}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert!(patterns.is_empty());
    }

    #[test]
    fn package_json_workspace_patterns_object_missing_packages() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"workspaces": {"nohoist": ["**/react-native"]}}"#).unwrap();
        let patterns = pkg.workspace_patterns();
        assert!(patterns.is_empty());
    }

    // ── Load from invalid JSON ──────────────────────────────────────

    #[test]
    fn package_json_load_invalid_json() {
        let temp_dir = std::env::temp_dir().join("fallow-test-invalid-pkg-json");
        let _ = std::fs::create_dir_all(&temp_dir);
        let pkg_path = temp_dir.join("package.json");
        std::fs::write(&pkg_path, "{ not valid json }").unwrap();

        let result = PackageJson::load(&pkg_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Failed to parse"), "got: {err}");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── Bin field with non-string value ─────────────────────────────

    #[test]
    fn package_json_bin_object_non_string_values_skipped() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"bin": {"cli": "./bin/cli.js", "bad": 42}}"#).unwrap();
        let entries = pkg.entry_points();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains(&"./bin/cli.js".to_string()));
    }

    // ── Default trait ───────────────────────────────────────────────

    #[test]
    fn package_json_default() {
        let pkg = PackageJson::default();
        assert!(pkg.name.is_none());
        assert!(pkg.main.is_none());
        assert!(pkg.entry_points().is_empty());
        assert!(pkg.all_dependency_names().is_empty());
        assert!(pkg.workspace_patterns().is_empty());
    }

    // ── Exports subdirectories ─────────────────────────────────────

    #[test]
    fn exports_subdirectories_preact_style() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                ".": "./dist/index.js",
                "./compat": { "import": "./compat/dist/compat.mjs" },
                "./hooks": { "import": "./hooks/dist/hooks.mjs" },
                "./debug": { "import": "./debug/dist/debug.mjs" },
                "./jsx-runtime": { "import": "./jsx-runtime/dist/jsx.mjs" },
                "./package.json": "./package.json"
            }
        }"#,
        )
        .unwrap();
        let mut dirs = pkg.exports_subdirectories();
        dirs.sort();
        assert_eq!(dirs, vec!["compat", "debug", "hooks", "jsx-runtime"]);
    }

    #[test]
    fn exports_subdirectories_skips_dist_dirs() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "exports": {
                "./dist/index.js": "./dist/index.js",
                "./build/bundle.js": "./build/bundle.js",
                "./lib/utils": "./lib/utils.js",
                "./compat": "./compat/index.js"
            }
        }"#,
        )
        .unwrap();
        let dirs = pkg.exports_subdirectories();
        // dist, build, lib are skipped
        assert_eq!(dirs, vec!["compat"]);
    }

    #[test]
    fn exports_subdirectories_no_exports() {
        let pkg: PackageJson = serde_json::from_str(r#"{"main": "index.js"}"#).unwrap();
        assert!(pkg.exports_subdirectories().is_empty());
    }

    #[test]
    fn exports_subdirectories_dot_only() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"exports": {".": "./dist/index.js"}}"#).unwrap();
        assert!(pkg.exports_subdirectories().is_empty());
    }
}
