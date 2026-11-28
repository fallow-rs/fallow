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
