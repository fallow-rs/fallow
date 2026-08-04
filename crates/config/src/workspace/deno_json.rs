//! Deno `deno.json` / `deno.jsonc` parsing for workspace discovery and package manifests.
//!
//! Deno monorepos declare members via root `workspace` and package identity via
//! member `name` + `exports`. Import maps live in root (and optionally member)
//! `imports`. Fallow consumes these the same way it consumes npm `package.json`
//! workspaces + exports, so Deno projects do not need bridge manifests.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde::Deserialize;

use super::package_json::PackageJson;

/// Candidate Deno config filenames, in preference order.
const DENO_JSON_NAMES: &[&str] = &["deno.json", "deno.jsonc"];

/// Parsed Deno config fields fallow needs for discovery and resolution.
#[derive(Debug, Clone, Default, Deserialize)]
struct DenoJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    exports: Option<serde_json::Value>,
    /// Workspace member globs (`["./apps/*", "./packages/*"]`).
    #[serde(default)]
    workspace: Option<DenoWorkspace>,
    /// Import map entries (`"@std/assert" → "jsr:@std/assert@1"`).
    #[serde(default)]
    imports: Option<FxHashMap<String, String>>,
}

/// Deno `workspace` accepts a bare member array or `{ "members": [...] }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum DenoWorkspace {
    Members(Vec<String>),
    Detailed {
        #[serde(default)]
        members: Vec<String>,
    },
}

impl DenoJson {
    /// Load `deno.json` or `deno.jsonc` from an explicit path.
    ///
    /// # Errors
    ///
    /// Returns an error string when the file cannot be read or parsed.
    fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let content = content.trim_start_matches('\u{FEFF}');
        crate::jsonc::parse_to_value(content)
            .map_err(|e| format!("Failed to parse {}: {e}", path.display()))
    }

    /// Load the first existing Deno config in `dir` (`deno.json`, then `deno.jsonc`).
    fn load_from_dir(dir: &Path) -> Result<Option<(PathBuf, Self)>, (PathBuf, String)> {
        for name in DENO_JSON_NAMES {
            let path = dir.join(name);
            if path.is_file() {
                let deno = Self::load(&path).map_err(|error| (path.clone(), error))?;
                return Ok(Some((path, deno)));
            }
        }
        Ok(None)
    }

    /// Workspace glob patterns from the root Deno config.
    #[must_use]
    fn workspace_patterns(&self) -> Vec<String> {
        match &self.workspace {
            Some(DenoWorkspace::Members(members)) | Some(DenoWorkspace::Detailed { members }) => {
                members.clone()
            }
            None => Vec::new(),
        }
    }

    /// Import map as sorted `(specifier, target)` pairs for stable hashing.
    #[must_use]
    fn import_map_entries(&self) -> Vec<(String, String)> {
        let Some(imports) = &self.imports else {
            return Vec::new();
        };
        let mut entries: Vec<(String, String)> = imports
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    /// Project a Deno package config into the `PackageJson` shape used by the
    /// resolver (`name` + `exports` only).
    #[must_use]
    fn to_package_json(&self) -> PackageJson {
        PackageJson {
            name: self.name.clone(),
            exports: self.exports.clone(),
            ..PackageJson::default()
        }
    }
}

/// Whether `dir` has a Deno package/workspace config file.
#[must_use]
pub fn dir_has_deno_json(dir: &Path) -> bool {
    DENO_JSON_NAMES.iter().any(|name| dir.join(name).is_file())
}

/// Whether `dir` has either an npm or Deno package manifest.
#[must_use]
pub fn dir_has_package_manifest(dir: &Path) -> bool {
    dir.join("package.json").is_file() || dir_has_deno_json(dir)
}

/// Load package identity for a workspace member directory.
///
/// Prefers `package.json` when present (npm / bridge layouts), while still
/// validating a colocated Deno config. Falls back to `deno.json` / `deno.jsonc`
/// so pure Deno packages resolve without bridges.
///
/// Returns `(name, package_json_view, dependency_names)`.
///
/// # Errors
///
/// Returns an error when a present manifest fails to parse.
pub fn load_member_package_manifest(
    dir: &Path,
) -> Result<Option<(String, PackageJson, Vec<String>)>, String> {
    manifest_from_probe(dir, DenoJson::load_from_dir(dir))
}

/// Project a pre-loaded Deno probe plus `package.json` into the
/// [`load_member_package_manifest`] result shape. Shared so combined probes
/// pay the `deno.json` / `deno.jsonc` filesystem probe only once.
fn manifest_from_probe(
    dir: &Path,
    deno: Result<Option<(PathBuf, DenoJson)>, (PathBuf, String)>,
) -> Result<Option<(String, PackageJson, Vec<String>)>, String> {
    let pkg_path = dir.join("package.json");
    if pkg_path.is_file() {
        let pkg = PackageJson::load(&pkg_path)?;
        deno.map_err(|(_path, error)| error)?;
        let deps = pkg.all_dependency_names();
        let name = pkg.name.clone().unwrap_or_else(|| dir_name_fallback(dir));
        return Ok(Some((name, pkg, deps)));
    }

    match deno.map_err(|(_path, error)| error)? {
        Some((_path, deno)) => {
            let pkg = deno.to_package_json();
            let name = pkg.name.clone().unwrap_or_else(|| dir_name_fallback(dir));
            Ok(Some((name, pkg, Vec::new())))
        }
        None => Ok(None),
    }
}

/// A directory's package-manifest outcome plus its Deno import map, produced
/// from a single `deno.json` / `deno.jsonc` filesystem probe.
///
/// The two fields keep the independent error semantics of the separate
/// loaders: `manifest` matches [`load_member_package_manifest`] exactly, and
/// `deno_import_map` matches the success case of [`load_deno_import_map`]
/// (absent when no Deno config exists or it fails to parse). A malformed
/// `package.json` therefore does not discard a valid colocated import map.
#[derive(Debug)]
pub struct DirManifestProbe {
    /// `(name, package_json_view, dependency_names)` or the manifest parse error.
    pub manifest: Result<Option<(String, PackageJson, Vec<String>)>, String>,
    /// Declaring config path and sorted `(specifier, target)` import-map entries.
    pub deno_import_map: Option<(PathBuf, Vec<(String, String)>)>,
}

/// Load a directory's package manifest and Deno import map together, probing
/// and parsing `deno.json` / `deno.jsonc` once instead of once per consumer.
#[must_use]
pub fn probe_dir_manifest(dir: &Path) -> DirManifestProbe {
    let deno = DenoJson::load_from_dir(dir);
    let deno_import_map = match &deno {
        Ok(Some((path, config))) => Some((path.clone(), config.import_map_entries())),
        _ => None,
    };
    let manifest = manifest_from_probe(dir, deno);
    DirManifestProbe {
        manifest,
        deno_import_map,
    }
}

/// Load a directory's package manifest as [`PackageJson`], preferring
/// `package.json` and falling back to `deno.json` / `deno.jsonc`.
#[must_use]
pub fn load_dir_package_json(dir: &Path) -> Option<PackageJson> {
    load_member_package_manifest(dir)
        .ok()
        .flatten()
        .map(|(_name, pkg, _deps)| pkg)
}

/// Load sorted Deno import-map entries and their declaring config path.
///
/// # Errors
///
/// Returns the chosen config path and parse error when a present config is
/// malformed.
#[expect(
    clippy::type_complexity,
    reason = "tuple projection keeps the full Deno config model private"
)]
pub fn load_deno_import_map(
    dir: &Path,
) -> Result<Option<(PathBuf, Vec<(String, String)>)>, (PathBuf, String)> {
    Ok(DenoJson::load_from_dir(dir)?.map(|(path, deno)| (path, deno.import_map_entries())))
}

/// Load root Deno workspace patterns and their declaring config path.
///
/// # Errors
///
/// Returns the chosen config path and parse error when a present config is
/// malformed.
#[expect(
    clippy::type_complexity,
    reason = "tuple projection keeps the full Deno config model private"
)]
pub fn load_root_deno_workspace_patterns(
    root: &Path,
) -> Result<Option<(PathBuf, Vec<String>)>, (PathBuf, String)> {
    Ok(DenoJson::load_from_dir(root)?.map(|(path, deno)| (path, deno.workspace_patterns())))
}

/// Whether the project has a root Deno config and should not warn about a
/// missing `node_modules` directory.
#[must_use]
pub fn is_deno_without_node_modules(root: &Path) -> bool {
    dir_has_deno_json(root) && !root.join("package.json").is_file()
}

fn dir_name_fallback(dir: &Path) -> String {
    dir.file_name().map_or_else(
        || dir.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_workspace_and_imports() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{
              "workspace": ["./apps/*", "./packages/*"],
              "imports": {
                "@std/assert": "jsr:@std/assert@1",
                "@std/": "jsr:@std/"
              }
            }"#,
        )
        .unwrap();

        let (path, deno) = DenoJson::load_from_dir(dir.path()).unwrap().unwrap();
        assert!(path.ends_with("deno.json"));
        assert_eq!(
            deno.workspace_patterns(),
            vec!["./apps/*".to_string(), "./packages/*".to_string()]
        );
        let map = deno.import_map_entries();
        assert!(
            map.iter()
                .any(|(k, v)| k == "@std/assert" && v == "jsr:@std/assert@1")
        );
    }

    #[test]
    fn parses_workspace_object_members_form() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{"workspace":{"members":["./packages/*"]}}"#,
        )
        .unwrap();

        let deno = DenoJson::load_from_dir(dir.path()).unwrap().unwrap().1;
        assert_eq!(deno.workspace_patterns(), vec!["./packages/*".to_string()]);
    }

    #[test]
    fn rejects_invalid_workspace_value() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), r#"{"workspace":5}"#).unwrap();

        assert!(DenoJson::load_from_dir(dir.path()).is_err());
    }

    #[test]
    fn parses_member_name_and_exports() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{
              "name": "@fallow/core",
              "exports": {
                ".": "./mod.ts",
                "./result": "./result.ts"
              }
            }"#,
        )
        .unwrap();

        let (_name, pkg, deps) = load_member_package_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(pkg.name.as_deref(), Some("@fallow/core"));
        assert!(pkg.exports.is_some());
        assert!(deps.is_empty());
    }

    #[test]
    fn prefers_package_json_over_deno_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"from-npm"}"#).unwrap();
        std::fs::write(dir.path().join("deno.json"), r#"{"name":"from-deno"}"#).unwrap();

        let (name, _, _) = load_member_package_manifest(dir.path()).unwrap().unwrap();
        assert_eq!(name, "from-npm");
    }

    #[test]
    fn import_map_loader_preserves_path_and_sort_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{"imports":{"z":"./z.ts","a":"./a.ts"}}"#,
        )
        .unwrap();

        let (path, entries) = load_deno_import_map(dir.path()).unwrap().unwrap();
        assert!(path.ends_with("deno.json"));
        assert_eq!(
            entries,
            vec![
                ("a".to_string(), "./a.ts".to_string()),
                ("z".to_string(), "./z.ts".to_string())
            ]
        );
    }

    #[test]
    fn deno_without_node_modules_requires_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        assert!(is_deno_without_node_modules(dir.path()));

        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(!is_deno_without_node_modules(dir.path()));
    }

    #[test]
    fn probe_keeps_import_map_when_package_json_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r"{,}").unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{"imports":{"@std/assert":"jsr:@std/assert@1"}}"#,
        )
        .unwrap();

        let probe = probe_dir_manifest(dir.path());
        assert!(
            probe.manifest.is_err(),
            "malformed package.json must surface as a manifest error"
        );
        let (path, entries) = probe
            .deno_import_map
            .expect("valid colocated import map survives a broken package.json");
        assert!(path.ends_with("deno.json"));
        assert_eq!(
            entries,
            vec![("@std/assert".to_string(), "jsr:@std/assert@1".to_string())]
        );
    }

    #[test]
    fn probe_drops_import_map_when_deno_config_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("deno.jsonc"), "{ imports: [ }").unwrap();

        let probe = probe_dir_manifest(dir.path());
        let error = probe
            .manifest
            .expect_err("malformed deno.jsonc must surface as a manifest error");
        assert!(
            error.contains("deno.jsonc"),
            "error should name the failing config: {error}"
        );
        assert!(probe.deno_import_map.is_none());
    }

    #[test]
    fn probe_returns_manifest_and_import_map_when_both_parse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name":"from-npm"}"#).unwrap();
        std::fs::write(
            dir.path().join("deno.json"),
            r#"{"imports":{"a":"./a.ts"}}"#,
        )
        .unwrap();

        let probe = probe_dir_manifest(dir.path());
        let (name, _pkg, _deps) = probe
            .manifest
            .expect("both configs parse")
            .expect("manifest present");
        assert_eq!(
            name, "from-npm",
            "package.json identity wins over deno.json"
        );
        let (_path, entries) = probe.deno_import_map.expect("import map present");
        assert_eq!(entries, vec![("a".to_string(), "./a.ts".to_string())]);
    }

    #[test]
    fn accepts_jsonc_comments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("deno.jsonc"),
            r#"{
              // root workspace
              "workspace": ["./packages/*"],
            }"#,
        )
        .unwrap();
        let deno = DenoJson::load_from_dir(dir.path()).unwrap().unwrap().1;
        assert_eq!(deno.workspace_patterns(), vec!["./packages/*".to_string()]);
    }
}
