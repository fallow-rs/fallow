//! Find the graph module of a package entry path.
//!
//! The public API entry sets of `fallow-core` and `fallow-engine` both use
//! this lookup, so both crates match a package entry to the same module.

use std::path::Path;

use fallow_types::discover::FileId;

use super::ModuleGraph;

impl ModuleGraph {
    /// Return the module of the package entry `entry_path`.
    ///
    /// `lookup` maps a module path to its `FileId`. The lookup tries the path
    /// as given, then its canonical form. Then it compares the canonical
    /// form with the canonical path of each module under `package_root`. That
    /// last step finds a module that the walk reached through a symlink, and
    /// the package scope keeps a miss bounded by the package file count.
    #[must_use]
    pub fn package_entry_file_id(
        &self,
        package_root: &Path,
        entry_path: &Path,
        lookup: impl Fn(&Path) -> Option<FileId>,
    ) -> Option<FileId> {
        lookup(entry_path).or_else(|| {
            let canonical = dunce::canonicalize(entry_path).ok()?;
            lookup(&canonical).or_else(|| {
                match_canonical_entry_under_package(
                    self.modules
                        .iter()
                        .map(|module| (module.path.as_path(), module.file_id)),
                    package_root,
                    &canonical,
                )
            })
        })
    }
}

/// Return the `FileId` of the first candidate under `package_root` whose
/// canonical form equals `canonical_entry`.
fn match_canonical_entry_under_package<'a>(
    candidates: impl Iterator<Item = (&'a Path, FileId)>,
    package_root: &Path,
    canonical_entry: &Path,
) -> Option<FileId> {
    candidates
        .filter(|(path, _)| path.starts_with(package_root))
        .find_map(|(path, file_id)| {
            (dunce::canonicalize(path).ok().as_deref() == Some(canonical_entry)).then_some(file_id)
        })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // A module whose discovered (raw) path goes through a symlinked directory
    // has a raw path that differs from the canonical entry path. The raw-map
    // lookup cannot reach it, so the package-scoped canonical match must.
    #[cfg_attr(miri, ignore)]
    #[test]
    fn scoped_canonical_matches_module_reached_through_symlink() {
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
        assert_eq!(
            match_canonical_entry_under_package(
                candidates.iter().copied(),
                package_root,
                &canonical_entry,
            ),
            Some(FileId(7)),
        );

        let outside_root = dir.path().join("other-package");
        assert_eq!(
            match_canonical_entry_under_package(
                candidates.iter().copied(),
                &outside_root,
                &canonical_entry,
            ),
            None,
        );

        let unrelated = dunce::canonicalize(dir.path()).unwrap().join("nope.ts");
        assert_eq!(
            match_canonical_entry_under_package(
                candidates.iter().copied(),
                package_root,
                &unrelated,
            ),
            None,
        );
    }
}
