//! The versions of the hoisted Bun packages that import resolution read.
//!
//! The graph cache key reads the lockfiles and install markers that a
//! package manager writes on each install. Bun writes no install marker, and
//! the key lists only the package names in `node_modules` of a Bun project.
//! A `bun install` of a pulled lockfile change can replace a hoisted package
//! with another version under the same name, and the new version can have
//! other `exports` (#4197).
//!
//! A cold run records each `node_modules/<name>/package.json` that the
//! resolver read (see [`crate::missed_probes`]). The graph cache payload
//! stores the `version` field of each of these manifests that a Bun lockfile
//! governs, and a warm run refuses the payload when one of the versions
//! changed. The imports of the project bound this set, so the size of the
//! install does not change the cost of a warm run.

use std::path::{Component, Path};

use rustc_hash::FxHashMap;

/// Lockfiles of Bun.
const BUN_LOCKFILES: &[&str] = &["bun.lock", "bun.lockb"];

/// The `version` field of each read package manifest that a Bun lockfile
/// governs.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledVersions {
    /// Sorted by path.
    entries: Vec<InstalledVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct InstalledVersion {
    /// The manifest path relative to the root, with forward slashes and
    /// with `../` steps for a manifest above the root.
    manifest: String,
    /// `None` when the manifest has no string `version` field or is not
    /// valid JSON.
    version: Option<String>,
}

impl InstalledVersions {
    /// Read the version of each manifest in `manifests` (root-relative, see
    /// [`crate::resolve::ResolvedProject::package_manifests`]) whose
    /// `node_modules` directory a Bun lockfile governs. A manifest that is
    /// gone now is left out: the package names in the graph cache key cover
    /// an uninstall.
    #[must_use]
    pub(crate) fn read(root: &Path, manifests: &[String]) -> Self {
        let mut governed: FxHashMap<&str, bool> = FxHashMap::default();
        let mut entries: Vec<InstalledVersion> = manifests
            .iter()
            .filter(|manifest| {
                let owner = owner_dir(manifest);
                *governed
                    .entry(owner)
                    .or_insert_with(|| has_bun_lockfile(&root.join(owner)))
            })
            .filter_map(|manifest| {
                let content = std::fs::read(root.join(manifest)).ok()?;
                Some(InstalledVersion {
                    manifest: manifest.clone(),
                    version: version_of(&content),
                })
            })
            .collect();
        entries.sort_unstable_by(|a, b| a.manifest.cmp(&b.manifest));
        Self { entries }
    }

    /// The number of stored manifests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no manifest is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when a stored manifest has another version now, or is gone.
    #[must_use]
    pub(crate) fn any_changed(&self, root: &Path) -> bool {
        let check = |entry: &InstalledVersion| {
            !std::fs::read(root.join(&entry.manifest))
                .is_ok_and(|content| version_of(&content) == entry.version)
        };
        if self.entries.len() < crate::missed_probes::PARALLEL_MIN {
            return self.entries.iter().any(check);
        }
        use rayon::prelude::*;
        self.entries.par_iter().any(check)
    }
}

/// The directory before the first `node_modules` step of `manifest`, which
/// is the directory where the package manager installed the package.
fn owner_dir(manifest: &str) -> &str {
    manifest
        .split_once("node_modules/")
        .map_or("", |(owner, _)| owner.trim_end_matches('/'))
}

/// True when `dir` or one of its ancestors holds a Bun lockfile. A Bun
/// workspace keeps one lockfile at the repository root and can install a
/// package into the `node_modules` of a workspace package.
fn has_bun_lockfile(dir: &Path) -> bool {
    dir.ancestors()
        .any(|dir| BUN_LOCKFILES.iter().any(|name| dir.join(name).is_file()))
}

fn version_of(content: &[u8]) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        version: Option<serde_json::Value>,
    }
    let manifest: Manifest = serde_json::from_slice(content).ok()?;
    match manifest.version? {
        serde_json::Value::String(version) => Some(version),
        _ => None,
    }
}

/// The manifest path of a package below a `node_modules` directory,
/// relative to one of `scopes`, or `None` for another file.
///
/// A path below a scope is spelled relative to it. A path below the
/// `node_modules` directory of an ancestor of a scope, where Node looks up a
/// package for a project in a subdirectory, gets one `../` step for each
/// level.
pub(crate) fn manifest_key(scopes: &[std::path::PathBuf], path: &Path) -> Option<String> {
    if path.file_name()? != "package.json" || !has_node_modules_step(path) {
        return None;
    }
    scopes.iter().find_map(|scope| {
        scope.ancestors().enumerate().find_map(|(depth, base)| {
            let rest = path.strip_prefix(base).ok()?;
            if depth > 0 && rest.components().next() != Some(node_modules()) {
                return None;
            }
            let mut key = "../".repeat(depth);
            key.push_str(&rest.to_string_lossy().replace('\\', "/"));
            Some(key)
        })
    })
}

fn has_node_modules_step(path: &Path) -> bool {
    path.components()
        .any(|component| component == node_modules())
}

fn node_modules() -> Component<'static> {
    Component::Normal("node_modules".as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, content).expect("file");
    }

    #[test]
    fn manifest_keys_cover_the_root_and_ancestor_node_modules_only() {
        let scopes = vec![std::path::PathBuf::from("/repo/app")];
        let key = |path: &str| manifest_key(&scopes, Path::new(path));
        assert_eq!(
            key("/repo/app/node_modules/a/package.json").as_deref(),
            Some("node_modules/a/package.json")
        );
        assert_eq!(
            key("/repo/node_modules/@s/b/package.json").as_deref(),
            Some("../node_modules/@s/b/package.json")
        );
        assert_eq!(key("/repo/app/package.json"), None);
        assert_eq!(key("/repo/app/node_modules/a/index.js"), None);
        assert_eq!(key("/repo/other/node_modules/a/package.json"), None);
    }

    #[test]
    fn owner_dir_is_the_directory_before_the_first_node_modules_step() {
        assert_eq!(owner_dir("node_modules/a/package.json"), "");
        assert_eq!(
            owner_dir("packages/ui/node_modules/a/node_modules/b/package.json"),
            "packages/ui"
        );
        assert_eq!(owner_dir("../node_modules/a/package.json"), "..");
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_hoisted_bun_version_change_is_seen() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("repo");
        write(&root.join("bun.lock"), "{}");
        let manifest = root.join("node_modules/a/package.json");
        write(&manifest, r#"{"name":"a","version":"1.0.0"}"#);
        let manifests = vec!["node_modules/a/package.json".to_string()];
        let versions = InstalledVersions::read(&root, &manifests);
        assert_eq!(versions.len(), 1);
        assert!(!versions.any_changed(&root));

        write(&manifest, r#"{"version":"1.0.0","name":"a","main":"x.js"}"#);
        assert!(
            !versions.any_changed(&root),
            "a rewrite with the same version must keep the payload"
        );

        write(&manifest, r#"{"name":"a","version":"2.0.0"}"#);
        assert!(versions.any_changed(&root));

        std::fs::remove_file(&manifest).expect("remove");
        assert!(versions.any_changed(&root));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn only_a_bun_install_is_stored() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("repo/app");
        let workspace = temp.path().join("repo/app/packages/ui");
        write(&root.join("node_modules/a/package.json"), "{}");
        write(&workspace.join("node_modules/b/package.json"), "{}");
        let manifests = vec![
            "node_modules/a/package.json".to_string(),
            "packages/ui/node_modules/b/package.json".to_string(),
        ];
        assert!(InstalledVersions::read(&root, &manifests).is_empty());

        write(&temp.path().join("repo/bun.lockb"), "");
        assert_eq!(
            InstalledVersions::read(&root, &manifests).len(),
            2,
            "a Bun lockfile above the owner directory governs it"
        );
    }
}
