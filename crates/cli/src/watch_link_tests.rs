//! Tests of the paths that the project watcher registers when the project
//! contains directory links.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::Watcher;

use super::watcher_config;

/// Return every path that a recursive watch of `root` registers.
///
/// The poll watcher walks the tree with the same link setting as the
/// project watcher, and its initial scan reports each path it registers
/// before `watch` returns. So the test does not depend on file system events.
fn registered_paths(root: &Path) -> Vec<PathBuf> {
    let (tx, rx) = mpsc::channel();
    let config = watcher_config().with_manual_polling();
    let mut watcher = notify::PollWatcher::with_initial_scan(|_| {}, config, tx)
        .expect("create the poll watcher");
    watcher
        .watch(root, notify::RecursiveMode::Recursive)
        .expect("watch the project root");
    drop(watcher);
    let mut paths: Vec<PathBuf> = rx.try_iter().filter_map(Result::ok).collect();
    paths.sort_unstable();
    paths
}

/// Return the registered paths that go through a directory link below `root`.
fn paths_through_links(root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| {
            path.ancestors()
                .skip(1)
                .take_while(|ancestor| *ancestor != root)
                .any(|ancestor| {
                    std::fs::symlink_metadata(ancestor)
                        .is_ok_and(|meta| meta.file_type().is_symlink())
                })
        })
        .cloned()
        .collect()
}

fn project_with_outside_dir() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let base = tempfile::tempdir().expect("create temp dir");
    let base_path = base.path().canonicalize().expect("canonical temp dir");
    let root = base_path.join("project");
    let outside = base_path.join("outside");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").expect("write index");
    std::fs::create_dir_all(outside.join("private/notes")).expect("create outside tree");
    std::fs::write(outside.join("private/notes/existing.ts"), "export {};\n")
        .expect("write outside file");
    (base, root, outside)
}

#[test]
fn watcher_does_not_register_paths_through_a_link_out_of_the_root() {
    let (_base, root, outside) = project_with_outside_dir();
    std::os::unix::fs::symlink(&outside, root.join("vendor")).expect("create link");

    let paths = registered_paths(&root);

    assert!(paths.contains(&root.join("src/index.ts")), "{paths:#?}");
    assert_eq!(paths_through_links(&root, &paths), Vec::<PathBuf>::new());
    assert!(
        !paths.iter().any(|path| path.ends_with("private/notes")),
        "{paths:#?}"
    );
}

#[test]
fn watcher_does_not_register_paths_through_linked_directories_that_loop() {
    let (_base, root, _outside) = project_with_outside_dir();
    std::fs::create_dir_all(root.join("a")).expect("create a");
    std::fs::create_dir_all(root.join("b")).expect("create b");
    std::fs::write(root.join("a/one.ts"), "export {};\n").expect("write a");
    std::fs::write(root.join("b/two.ts"), "export {};\n").expect("write b");
    std::os::unix::fs::symlink("../b", root.join("a/to_b")).expect("create a link");
    std::os::unix::fs::symlink("../a", root.join("b/to_a")).expect("create b link");
    std::os::unix::fs::symlink(".", root.join("self")).expect("create self link");

    let paths = registered_paths(&root);

    assert!(paths.contains(&root.join("a/one.ts")), "{paths:#?}");
    assert!(paths.contains(&root.join("b/two.ts")), "{paths:#?}");
    assert_eq!(paths_through_links(&root, &paths), Vec::<PathBuf>::new());
}

#[test]
fn watcher_keeps_the_target_of_a_link_inside_the_root() {
    let (_base, root, _outside) = project_with_outside_dir();
    std::os::unix::fs::symlink("src", root.join("alias")).expect("create link");

    let paths = registered_paths(&root);

    assert!(paths.contains(&root.join("src/index.ts")), "{paths:#?}");
    assert!(paths.contains(&root.join("alias")), "{paths:#?}");
    assert_eq!(paths_through_links(&root, &paths), Vec::<PathBuf>::new());
}
