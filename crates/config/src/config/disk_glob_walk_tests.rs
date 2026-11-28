//! The walk of a disk glob with `**` segments: bounded work, and the same
//! matches as the `glob` crate.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;

/// The wall-clock bound of one bounded expansion in a debug build.
const EXPAND_BOUND: Duration = Duration::from_secs(3);

fn expanded(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = expand_disk_glob(root, pattern)
        .expect("valid pattern")
        .filter_map(Result::ok)
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn many_recursive_segments_over_nested_folders_stay_bounded() {
    const DEPTH: usize = 24;
    let dir = tempfile::tempdir().unwrap();
    let deepest = dir.path().join(vec!["a"; DEPTH].join("/"));
    std::fs::create_dir_all(&deepest).unwrap();
    std::fs::write(deepest.join("x.json"), "").unwrap();
    let pattern = format!("{}nope", "**/a/".repeat(DEPTH / 2));

    let start = Instant::now();
    let hits = expanded(dir.path(), &pattern);
    let elapsed = start.elapsed();

    assert!(hits.is_empty());
    assert!(elapsed < EXPAND_BOUND, "expansion took {elapsed:?}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn recursive_walk_matches_the_glob_crate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for folder in ["a/b/c", "a/.hidden/c", "x/a/b", "b"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    for file in [
        "a/b/c/m.json",
        "a/b/n.json",
        "a/.hidden/c/m.json",
        "x/a/b/m.json",
        "b/m.json",
        "m.json",
        ".dot.json",
    ] {
        std::fs::write(root.join(file), "").unwrap();
    }
    let canonical = dunce::canonicalize(root).unwrap();
    for pattern in [
        "**",
        "**/*.json",
        "**/m.json",
        "a/**",
        "a/**/m.json",
        "**/a/**/m.json",
        "**/b/*.json",
        "**/**/c/*",
        "a/**/c",
        "*/**/b",
        "./**/n.json",
        "**/.hidden/**/*.json",
    ] {
        #[expect(
            clippy::disallowed_methods,
            reason = "the reference result of the test"
        )]
        let mut reference: Vec<PathBuf> = glob::glob(&format!(
            "{}/{pattern}",
            glob::Pattern::escape(&canonical.to_string_lossy())
        ))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
        reference.sort();
        reference.dedup();
        assert_eq!(expanded(&canonical, pattern), reference, "{pattern}");
    }
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn recursive_walk_ends_on_a_folder_link_loop() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::os::unix::fs::symlink("..", dir.path().join("a/up")).unwrap();
    std::os::unix::fs::symlink(".", dir.path().join("a/same")).unwrap();
    std::fs::write(dir.path().join("a/m.json"), "").unwrap();

    let start = Instant::now();
    let hits = expanded(dir.path(), "**/m.json");

    assert!(hits.contains(&dir.path().join("a/m.json")));
    assert!(start.elapsed() < EXPAND_BOUND);
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn recursive_walk_follows_a_link_inside_the_checkout_above_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dunce::canonicalize(dir.path()).unwrap();
    std::fs::create_dir(repo.join(".git")).unwrap();
    let root = repo.join("packages/app");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(repo.join("shared/cfg/x")).unwrap();
    std::fs::write(repo.join("shared/cfg/x/marker.json"), "").unwrap();
    std::os::unix::fs::symlink("../../shared/cfg", root.join("cfg")).unwrap();

    let hits = expanded(&root, "cfg/**/marker.json");

    assert_eq!(hits, vec![root.join("cfg/x/marker.json")]);
}

/// Whether the file system of `dir` finds a file by a name in other letter
/// case.
fn case_insensitive_file_system(dir: &Path) -> bool {
    let probe = dir.join("CaseProbe");
    std::fs::write(&probe, "").unwrap();
    let insensitive = dir.join("caseprobe").exists();
    std::fs::remove_file(probe).unwrap();
    insensitive
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn a_literal_segment_after_a_recursive_segment_compares_letter_case() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    if !case_insensitive_file_system(&root) {
        return;
    }
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::write(root.join("a/README.md"), "").unwrap();
    std::fs::write(root.join("README.md"), "").unwrap();

    assert!(expanded(&root, "**/readme.md").is_empty());
    assert_eq!(
        expanded(&root, "**/README.md"),
        vec![root.join("README.md"), root.join("a/README.md")]
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn the_first_match_comes_before_the_walk_reads_the_full_tree() {
    const FOLDERS: usize = 4_000;
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::write(root.join("a/m.json"), "").unwrap();
    for index in 0..FOLDERS {
        std::fs::create_dir_all(root.join(format!("z/{}/{index}", index % 40))).unwrap();
    }

    let start = Instant::now();
    let first = expand_disk_glob(&root, "**/m.json")
        .unwrap()
        .find_map(Result::ok);
    let first_elapsed = start.elapsed();
    let start = Instant::now();
    let all = expand_disk_glob(&root, "**/m.json")
        .unwrap()
        .filter_map(Result::ok)
        .count();
    let all_elapsed = start.elapsed();

    assert_eq!(first, Some(root.join("a/m.json")));
    assert_eq!(all, 1);
    assert!(
        first_elapsed * 10 < all_elapsed,
        "first match {first_elapsed:?}, full walk {all_elapsed:?}"
    );
}
