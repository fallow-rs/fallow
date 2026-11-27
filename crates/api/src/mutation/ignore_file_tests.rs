//! `ignore_cache_dir` writes a new `.gitignore` only, and never through a
//! symlink.

use std::os::unix::fs::symlink;

use super::ignore_cache_dir;

#[test]
fn a_missing_ignore_file_is_written() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join(".fallow");
    ignore_cache_dir(&cache).expect("ignore");
    assert_eq!(
        std::fs::read_to_string(cache.join(".gitignore")).expect("read"),
        "*\n"
    );
}

#[test]
fn an_existing_ignore_file_keeps_its_content() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join(".fallow");
    std::fs::create_dir_all(&cache).expect("cache");
    std::fs::write(cache.join(".gitignore"), "own\n").expect("write");
    ignore_cache_dir(&cache).expect("ignore");
    assert_eq!(
        std::fs::read_to_string(cache.join(".gitignore")).expect("read"),
        "own\n"
    );
}

#[test]
fn a_dangling_ignore_link_is_not_followed() {
    let dir = tempfile::tempdir().expect("dir");
    let cache = dir.path().join(".fallow");
    std::fs::create_dir_all(&cache).expect("cache");
    let target = dir.path().join("outside/created.txt");
    std::fs::create_dir_all(target.parent().expect("parent")).expect("outside");
    symlink(&target, cache.join(".gitignore")).expect("link");
    ignore_cache_dir(&cache).expect("ignore");
    assert!(!target.exists(), "the link target is not created");
}
