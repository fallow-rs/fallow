//! The base snapshots of `fallow audit` and the lookups above a project.
//!
//! An audit analyzes its base snapshot (and the tree of `--staged`) in a
//! directory below the temp dir. Module resolution and project discovery
//! walk up from the analysis root to find `node_modules`, `package.json`,
//! `tsconfig.json`, lockfiles and workspace roots. For a checkout these
//! walks read the directories of the user above it. For a snapshot they
//! would read the temp dir, so they stop at the snapshot root.

use std::ffi::OsStr;
use std::path::{Component, Path};

/// The name prefix of the directory of the user that holds the snapshots.
/// The effective user id follows it.
pub const SNAPSHOT_DIR_PREFIX: &str = "fallow-audit-snapshots-";

/// The name prefix of each snapshot directory.
pub const SNAPSHOT_NAME_PREFIX: &str = "fallow-audit-base-";

/// The names that a lookup above a project probes in each directory.
const LOOKUP_NAMES: &[&str] = &[
    "node_modules",
    "package.json",
    "tsconfig.json",
    "jsconfig.json",
    ".pnp.cjs",
    ".pnp.js",
    ".pnp.data.json",
    ".pnp.loader.mjs",
    "deno.json",
    "deno.jsonc",
    "pnpm-workspace.yaml",
    "bun.lock",
    "bun.lockb",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
];

/// The name of the snapshot directory of the effective user.
#[cfg(unix)]
#[must_use]
pub fn snapshot_dir_name() -> String {
    format!(
        "{SNAPSHOT_DIR_PREFIX}{}",
        rustix::process::geteuid().as_raw()
    )
}

/// The root of the audit snapshot that holds `path`: the nearest ancestor
/// named `fallow-audit-base-*` in the snapshot directory of the user. `None`
/// for a path outside every snapshot.
#[must_use]
pub fn snapshot_root(path: &Path) -> Option<&Path> {
    path.ancestors().find(|dir| {
        dir.file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(SNAPSHOT_NAME_PREFIX))
            && dir.parent().is_some_and(is_snapshot_dir)
    })
}

#[cfg(unix)]
fn is_snapshot_dir(dir: &Path) -> bool {
    dir.file_name()
        .is_some_and(|name| *name == *snapshot_dir_name())
}

/// On Windows the snapshots are directly in the temp dir.
#[cfg(not(unix))]
fn is_snapshot_dir(dir: &Path) -> bool {
    let temp = std::env::temp_dir();
    dir == temp
        || std::fs::canonicalize(dir)
            .ok()
            .zip(std::fs::canonicalize(&temp).ok())
            .is_some_and(|(dir, temp)| dir == temp)
}

/// The ancestors of `start`, closest first, that a lookup above a project
/// visits: each ancestor, or for a path in a snapshot only the ancestors up
/// to and including the snapshot root.
pub fn lookup_ancestors(start: &Path) -> impl Iterator<Item = &Path> {
    let ceiling = snapshot_root(start);
    start
        .ancestors()
        .take_while(move |dir| ceiling.is_none_or(|top| dir.starts_with(top)))
}

/// [`lookup_ancestors`] without `start` itself.
pub fn lookup_parents(start: &Path) -> impl Iterator<Item = &Path> {
    lookup_ancestors(start).skip(1)
}

/// True when `path` is a lookup name in a directory above the snapshot root
/// `ceiling`, or a path below such a name: `<dir>/node_modules/...` or
/// `<dir>/package.json` for a strict ancestor `<dir>` of `ceiling`. A path
/// that only passes through an ancestor, such as the target of a linked
/// `node_modules` in the checkout, is not a lookup.
#[must_use]
pub fn is_lookup_above(ceiling: &Path, path: &Path) -> bool {
    if path.starts_with(ceiling) {
        return false;
    }
    ceiling.ancestors().skip(1).any(|dir| {
        path.strip_prefix(dir).is_ok_and(|rest| {
            matches!(
                rest.components().next(),
                Some(Component::Normal(name))
                    if name.to_str().is_some_and(|name| LOOKUP_NAMES.contains(&name))
            )
        })
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn snapshot() -> PathBuf {
        PathBuf::from("/tmp")
            .join(snapshot_dir_name())
            .join("fallow-audit-base-cache-0000000000000000-root-0000000000000000")
    }

    #[test]
    fn a_path_in_a_snapshot_has_its_snapshot_root() {
        let root = snapshot();
        assert_eq!(
            snapshot_root(&root.join("packages/app")),
            Some(root.as_path())
        );
        assert_eq!(snapshot_root(&root), Some(root.as_path()));
        assert_eq!(
            snapshot_root(Path::new("/tmp/fallow-audit-base-1-2-3/app")),
            None
        );
        assert_eq!(snapshot_root(Path::new("/home/user/project")), None);
    }

    #[test]
    fn lookups_stop_at_the_snapshot_root() {
        let root = snapshot();
        let start = root.join("packages/app");
        let visited: Vec<&Path> = lookup_ancestors(&start).collect();
        assert_eq!(visited.last(), Some(&root.as_path()));
        assert_eq!(visited.len(), 3);
        assert_eq!(lookup_ancestors(Path::new("/a/b")).count(), 3);
    }

    #[test]
    fn a_lookup_name_above_the_snapshot_is_refused() {
        let root = snapshot();
        let dir = root.parent().expect("snapshot directory");
        assert!(is_lookup_above(&root, &dir.join("package.json")));
        assert!(is_lookup_above(
            &root,
            Path::new("/tmp/node_modules/pkg/index.js")
        ));
        assert!(is_lookup_above(&root, Path::new("/tsconfig.json")));
        assert!(!is_lookup_above(&root, &root.join("node_modules/pkg")));
        assert!(!is_lookup_above(
            &root,
            Path::new("/home/user/repo/node_modules/pkg")
        ));
        assert!(!is_lookup_above(
            &root,
            Path::new("/tmp/ci/repo/package.json")
        ));
    }
}
