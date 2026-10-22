//! Find the graph module of a package entry path.
//!
//! The public API entry sets of `fallow-core` and `fallow-engine` both use
//! this lookup, so both crates match a package entry to the same module.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_types::discover::FileId;
use rustc_hash::FxHashMap;

use super::ModuleGraph;

impl ModuleGraph {
    /// Return a resolver for the package entry paths of `package_root`.
    #[must_use]
    pub fn package_entry_resolver<'a>(
        &'a self,
        package_root: &'a Path,
    ) -> PackageEntryResolver<'a> {
        PackageEntryResolver {
            graph: self,
            package_root,
            canonical_modules: OnceLock::new(),
        }
    }
}

/// Find the graph module of a package entry path for one package root.
///
/// The resolver tries the path as given, then its canonical form. Then it
/// compares the canonical form with the canonical path of each module under
/// the package root. That last step finds a module that the walk reached
/// through a symlink. The resolver builds the canonical module index on the
/// first miss and keeps it, so a package with many missed entry candidates
/// resolves each module path one time only.
pub struct PackageEntryResolver<'a> {
    graph: &'a ModuleGraph,
    package_root: &'a Path,
    canonical_modules: OnceLock<FxHashMap<PathBuf, FileId>>,
}

impl PackageEntryResolver<'_> {
    /// Return the module of the package entry `entry_path`.
    ///
    /// `lookup` maps a module path to its `FileId`.
    #[must_use]
    pub fn file_id(
        &self,
        entry_path: &Path,
        lookup: impl Fn(&Path) -> Option<FileId>,
    ) -> Option<FileId> {
        lookup(entry_path).or_else(|| {
            let canonical = dunce::canonicalize(entry_path).ok()?;
            lookup(&canonical).or_else(|| {
                self.canonical_modules
                    .get_or_init(|| {
                        canonical_module_index(
                            self.graph
                                .modules
                                .iter()
                                .map(|module| (module.path.as_path(), module.file_id)),
                            self.package_root,
                        )
                    })
                    .get(&canonical)
                    .copied()
            })
        })
    }
}

/// Map the canonical path of each candidate under `package_root` to its
/// `FileId`. The first candidate wins when two paths share a canonical form.
fn canonical_module_index<'a>(
    candidates: impl Iterator<Item = (&'a Path, FileId)>,
    package_root: &Path,
) -> FxHashMap<PathBuf, FileId> {
    let mut canonical_dirs = FxHashMap::default();
    let mut index = FxHashMap::default();
    for (path, file_id) in candidates.filter(|(path, _)| path.starts_with(package_root)) {
        if let Some(canonical) = canonical_module_path(path, &mut canonical_dirs) {
            index.entry(canonical).or_insert(file_id);
        }
    }
    index
}

/// Return the same path as `dunce::canonicalize(path)`, with one directory
/// resolution for each distinct parent directory.
///
/// When the file itself is not a symlink, its canonical path is the canonical
/// parent directory joined with the file name. A symlinked file gets a full
/// resolution.
fn canonical_module_path(
    path: &Path,
    canonical_dirs: &mut FxHashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return dunce::canonicalize(path).ok();
    };
    if metadata.file_type().is_symlink() || parent.as_os_str().is_empty() {
        return dunce::canonicalize(path).ok();
    }
    canonical_dirs
        .entry(parent.to_path_buf())
        .or_insert_with(|| dunce::canonicalize(parent).ok())
        .as_ref()
        .map(|dir| dir.join(name))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // A module whose discovered (raw) path goes through a symlinked directory
    // has a raw path that differs from the canonical entry path. The raw-map
    // lookup cannot reach it, so the package-scoped canonical index must.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn scoped_canonical_index_matches_module_reached_through_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let real_file = real_dir.join("mod.ts");
        std::fs::write(&real_file, "export const x = 1;\n").unwrap();
        let link_dir = dir.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();

        let module_raw_path = link_dir.join("mod.ts");
        let canonical_entry = dunce::canonicalize(&real_file).unwrap();
        let package_root = dir.path();

        let candidates = [(module_raw_path.as_path(), FileId(7))];
        let index = canonical_module_index(candidates.iter().copied(), package_root);
        assert_eq!(index.get(&canonical_entry), Some(&FileId(7)));

        let outside_root = dir.path().join("other-package");
        let index = canonical_module_index(candidates.iter().copied(), &outside_root);
        assert!(index.is_empty());

        let unrelated = dunce::canonicalize(dir.path()).unwrap().join("nope.ts");
        let index = canonical_module_index(candidates.iter().copied(), package_root);
        assert_eq!(index.get(&unrelated), None);
    }

    // A module file that is itself a symlink resolves to its target, the same
    // as a full canonicalization of the module path.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn canonical_index_resolves_symlinked_module_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.ts");
        std::fs::write(&target, "export const x = 1;\n").unwrap();
        let link = dir.path().join("link.ts");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let missing = dir.path().join("missing.ts");

        let candidates = [
            (link.as_path(), FileId(1)),
            (target.as_path(), FileId(2)),
            (missing.as_path(), FileId(3)),
        ];
        let index = canonical_module_index(candidates.iter().copied(), dir.path());

        // Both paths share one canonical form, and the first candidate wins.
        assert_eq!(index.len(), 1);
        assert_eq!(
            index.get(&dunce::canonicalize(&target).unwrap()),
            Some(&FileId(1))
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn canonical_module_path_matches_full_canonicalization() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("mod.ts");
        std::fs::write(&file, "").unwrap();
        let link_dir = dir.path().join("via");
        std::os::unix::fs::symlink(&nested, &link_dir).unwrap();

        let mut canonical_dirs = FxHashMap::default();
        for path in [
            file,
            link_dir.join("mod.ts"),
            dir.path().join("a/b/../b/mod.ts"),
        ] {
            assert_eq!(
                canonical_module_path(&path, &mut canonical_dirs),
                dunce::canonicalize(&path).ok(),
                "{}",
                path.display()
            );
        }
        assert_eq!(
            canonical_module_path(&dir.path().join("a/none.ts"), &mut canonical_dirs),
            None
        );
    }
}
