//! The mutation cache directory follows the symlink rule of the analysis
//! caches: a link on the way to the run directories that resolves outside the
//! project stops the run before fallow writes or removes anything there.

use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::mutation::guard_cache_dir;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("dir");
    let root = dir.path().join("project");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(root.join("src")).expect("root");
    std::fs::create_dir_all(&outside).expect("outside");
    Fixture {
        _dir: dir,
        root,
        outside,
    }
}

fn assert_refused(f: &Fixture, cache: &std::path::Path) {
    let error = guard_cache_dir(&f.root, cache).expect_err("refused");
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_UNSAFE_CACHE_DIR")
    );
    assert_eq!(error.exit_code, 2);
}

#[test]
fn a_cache_dir_link_that_leaves_the_root_is_refused() {
    let f = fixture();
    symlink(&f.outside, f.root.join(".fallow")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}

#[test]
fn a_runs_link_that_leaves_the_root_is_refused() {
    let f = fixture();
    let mutation = f.root.join(".fallow/mutation");
    std::fs::create_dir_all(&mutation).expect("mutation");
    symlink(&f.outside, mutation.join("runs")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}

#[test]
fn a_mutation_link_into_git_metadata_is_refused() {
    let f = fixture();
    std::fs::create_dir_all(f.root.join(".git/objects")).expect("git");
    std::fs::create_dir_all(f.root.join(".fallow")).expect("cache");
    symlink(f.root.join(".git/objects"), f.root.join(".fallow/mutation")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}

#[test]
fn a_plain_cache_dir_and_a_cache_link_inside_the_root_pass() {
    let f = fixture();
    assert!(guard_cache_dir(&f.root, &f.root.join(".fallow")).is_ok());
    std::fs::create_dir_all(f.root.join("cache-store")).expect("store");
    symlink(f.root.join("cache-store"), f.root.join(".fallow")).expect("link");
    assert!(guard_cache_dir(&f.root, &f.root.join(".fallow")).is_ok());
}

#[test]
fn a_mutation_link_inside_the_root_is_refused() {
    let f = fixture();
    std::fs::create_dir_all(f.root.join("inner")).expect("inner");
    std::fs::create_dir_all(f.root.join(".fallow")).expect("cache");
    symlink("../inner", f.root.join(".fallow/mutation")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}

#[test]
fn a_companion_link_inside_the_root_is_refused() {
    let f = fixture();
    std::fs::create_dir_all(f.root.join("inner")).expect("inner");
    let mutation = f.root.join(".fallow/mutation");
    std::fs::create_dir_all(&mutation).expect("mutation");
    symlink("../../inner", mutation.join("companion")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}

#[test]
fn a_runs_link_inside_the_root_is_refused() {
    let f = fixture();
    std::fs::create_dir_all(f.root.join("inner")).expect("inner");
    let mutation = f.root.join(".fallow/mutation");
    std::fs::create_dir_all(&mutation).expect("mutation");
    symlink("../../inner", mutation.join("runs")).expect("link");
    assert_refused(&f, &f.root.join(".fallow"));
}
