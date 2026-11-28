//! Canonical path index of the discovered files.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use fallow_types::discover::{DiscoveredFile, FileId};
use rustc_hash::{FxHashMap, FxHashSet};

/// Map the canonical path of each discovered file to its id.
///
/// `realpath` looks up each component of a path, so a `realpath` of each
/// file costs one lookup per component per file. On a large monorepo that is
/// hundreds of thousands of system calls. Files in one directory share the
/// canonical path of that directory: when the directory listing shows the
/// exact file name and the entry is not a symlink, the canonical path of the
/// file is the canonical directory joined with the name. A symlink, or a name
/// that is not on disk in exactly that form (another case on a
/// case-insensitive volume), still goes through `realpath`.
///
/// A listing costs a `realpath` of the directory plus the read of its
/// entries, so only a directory with at least [`MIN_FILES_TO_LIST_DIR`]
/// indexed files is listed. Test fixture trees often hold one or two files
/// per directory, and there a `realpath` of each file is cheaper. Each
/// directory is listed at most once per index build; a failed listing is
/// kept too, and its files go through `realpath`.
pub(super) fn canonical_file_index(files: &[DiscoveredFile]) -> FxHashMap<PathBuf, FileId> {
    let mut files_per_dir: FxHashMap<&Path, usize> = FxHashMap::default();
    for file in files {
        if let Some((dir, _)) = split_normal(&file.path) {
            *files_per_dir.entry(dir).or_default() += 1;
        }
    }
    let mut dirs: FxHashMap<&Path, Option<ListedDir>> = FxHashMap::default();
    files
        .iter()
        .filter_map(|file| {
            canonical_file_path(&file.path, &files_per_dir, &mut dirs).map(|c| (c, file.id))
        })
        .collect()
}

/// The number of indexed files in one directory from which a listing of the
/// directory is cheaper than a `realpath` of each file.
const MIN_FILES_TO_LIST_DIR: usize = 3;

/// The canonical path of a directory and the names of its entries that are
/// not symlinks.
struct ListedDir {
    canonical: PathBuf,
    plain_entries: FxHashSet<OsString>,
}

fn canonical_file_path<'a>(
    path: &'a Path,
    files_per_dir: &FxHashMap<&Path, usize>,
    dirs: &mut FxHashMap<&'a Path, Option<ListedDir>>,
) -> Option<PathBuf> {
    if let Some((dir, name)) = split_normal(path)
        && files_per_dir.get(dir).copied().unwrap_or(0) >= MIN_FILES_TO_LIST_DIR
        && let Some(listed) = dirs.entry(dir).or_insert_with(|| list_dir(dir))
        && listed.plain_entries.contains(name)
    {
        crate::read_trace::note_existing_file(path);
        return Some(listed.canonical.join(name));
    }
    crate::read_trace::canonicalize(path).ok()
}

/// Split an absolute path in normal form into its directory and file name.
///
/// Only Unix takes this route: Windows canonical paths depend on more than
/// the parent directory (reparse points, 8.3 names), so they keep the full
/// `canonicalize`.
fn split_normal(path: &Path) -> Option<(&Path, &OsStr)> {
    if !cfg!(unix) || !path.is_absolute() {
        return None;
    }
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next_back() else {
        return None;
    };
    let dir = components.as_path();
    if components.any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        || dir.join(name).as_os_str() != path.as_os_str()
    {
        return None;
    }
    Some((dir, name))
}

fn list_dir(dir: &Path) -> Option<ListedDir> {
    let canonical = crate::read_trace::canonicalize_dir_untraced(dir).ok()?;
    let plain_entries = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_symlink()))
        .map(|entry| entry.file_name())
        .collect();
    Some(ListedDir {
        canonical,
        plain_entries,
    })
}

#[cfg(all(test, unix, not(miri)))]
mod tests {
    use super::*;
    use crate::read_trace::realpath_calls;
    use crate::resolve::types::CanonicalizeCache;
    use std::os::unix::fs::symlink;

    fn discovered(paths: &[PathBuf]) -> Vec<DiscoveredFile> {
        paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).unwrap()),
                path: path.clone(),
                size_bytes: 0,
            })
            .collect()
    }

    /// The index that a `realpath` of each file gives.
    fn oracle(files: &[DiscoveredFile]) -> FxHashMap<PathBuf, FileId> {
        files
            .iter()
            .filter_map(|file| dunce::canonicalize(&file.path).ok().map(|c| (c, file.id)))
            .collect()
    }

    /// A tree with a plain directory, a directory symlink, a file symlink, a
    /// dangling symlink and a file below a symlinked parent.
    fn mixed_tree(root: &Path) -> Vec<PathBuf> {
        let src = root.join("src");
        let real = root.join("real");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(real.join("nested")).unwrap();
        for name in ["a.ts", "b.ts", "c.tsx"] {
            std::fs::write(src.join(name), "").unwrap();
        }
        std::fs::write(real.join("target.ts"), "").unwrap();
        for name in ["deep.ts", "one.ts", "two.ts"] {
            std::fs::write(real.join("nested").join(name), "").unwrap();
        }
        symlink(&real, root.join("linked")).unwrap();
        symlink(real.join("target.ts"), src.join("alias.ts")).unwrap();
        symlink(root.join("missing.ts"), src.join("dangling.ts")).unwrap();
        vec![
            src.join("a.ts"),
            src.join("b.ts"),
            src.join("c.tsx"),
            src.join("alias.ts"),
            src.join("dangling.ts"),
            src.join("gone.ts"),
            root.join("linked/target.ts"),
            root.join("linked/nested/deep.ts"),
            root.join("linked/nested/one.ts"),
            root.join("linked/nested/two.ts"),
            root.join("removed/a.ts"),
            root.join("removed/b.ts"),
            root.join("removed/c.ts"),
            real.join("target.ts"),
        ]
    }

    #[test]
    fn index_matches_realpath_of_each_file() {
        let temp = tempfile::tempdir().unwrap();
        let files = discovered(&mixed_tree(temp.path()));
        assert_eq!(canonical_file_index(&files), oracle(&files));
    }

    #[test]
    fn index_matches_realpath_for_a_name_in_another_case() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("src");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Button.tsx"), "").unwrap();
        std::fs::write(dir.join("Card.tsx"), "").unwrap();
        let files = discovered(&[
            dir.join("button.tsx"),
            dir.join("Button.tsx"),
            dir.join("Card.tsx"),
        ]);
        assert_eq!(canonical_file_index(&files), oracle(&files));
    }

    #[test]
    fn index_matches_realpath_for_a_name_in_another_unicode_form() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("src");
        std::fs::create_dir_all(&dir).unwrap();
        let composed = "caf\u{e9}.ts";
        let decomposed = "cafe\u{301}.ts";
        for name in [composed, "a.ts", "b.ts"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let files = discovered(&[
            dir.join(decomposed),
            dir.join(composed),
            dir.join("a.ts"),
            dir.join("b.ts"),
        ]);
        assert_eq!(canonical_file_index(&files), oracle(&files));
    }

    #[cfg(feature = "read-trace")]
    #[test]
    fn index_records_the_same_reads_as_a_realpath_of_each_file() {
        let temp = tempfile::tempdir().unwrap();
        let files = discovered(&mixed_tree(temp.path()));
        let recorder = crate::read_trace::ReadRecorder::start(temp.path());
        let _ = canonical_file_index(&files);
        let reads = recorder.finish();
        let recorder = crate::read_trace::ReadRecorder::start(temp.path());
        for file in &files {
            let _ = crate::read_trace::canonicalize(&file.path);
        }
        assert_eq!(reads, recorder.finish());
    }

    #[test]
    fn index_keeps_realpath_for_a_directory_with_few_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("fixture");
        std::fs::create_dir_all(&dir).unwrap();
        let paths = [dir.join("input.js"), dir.join("output.js")];
        for path in &paths {
            std::fs::write(path, "").unwrap();
        }
        let files = discovered(&paths);
        let before = realpath_calls();
        let index = canonical_file_index(&files);
        assert_eq!(realpath_calls() - before, 2);
        assert_eq!(index, oracle(&files));
    }

    #[test]
    fn index_needs_one_realpath_per_directory_of_plain_files() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("src");
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<PathBuf> = (0..20).map(|i| dir.join(format!("f{i}.ts"))).collect();
        for path in &paths {
            std::fs::write(path, "").unwrap();
        }
        let files = discovered(&paths);
        let before = realpath_calls();
        let index = canonical_file_index(&files);
        assert_eq!(realpath_calls() - before, 1);
        assert_eq!(index, oracle(&files));
    }

    #[test]
    fn canonicalize_cache_matches_realpath_for_probes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let paths = mixed_tree(root);
        let cache = CanonicalizeCache::default();
        let mut probes = paths;
        probes.push(root.join("src/a.ts/below-a-file.ts"));
        probes.push(root.join("no-dir/x.ts"));
        probes.push(root.join("linked"));
        for probe in &probes {
            assert_eq!(
                cache.get(probe),
                dunce::canonicalize(probe).ok(),
                "{probe:?}"
            );
        }
    }

    #[test]
    fn canonicalize_cache_finds_a_missing_probe_without_realpath() {
        let temp = tempfile::tempdir().unwrap();
        let cache = CanonicalizeCache::default();
        let before = realpath_calls();
        assert!(cache.get(&temp.path().join("missing/index.ts")).is_none());
        assert!(cache.get(&temp.path().join("missing.ts")).is_none());
        assert_eq!(realpath_calls() - before, 0);
    }
}
