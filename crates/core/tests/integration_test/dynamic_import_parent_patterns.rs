//! Dynamic import patterns that start with `../` match files relative to the
//! importing file, the same way Vite and webpack resolve them.

use super::common::{create_config, fixture_path};

fn unused_files() -> Vec<String> {
    unused_files_at(fixture_path("dynamic-import-parent-patterns"))
}

fn unused_files_at(root: std::path::PathBuf) -> Vec<String> {
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused.sort();
    unused
}

#[test]
fn parent_directory_patterns_credit_their_targets() {
    let unused = unused_files();
    for used in [
        "src/components/button.js",
        "src/components/card.js",
        "src/locales/en.js",
        "src/locales/nl.js",
        "src/views/home.js",
        "src/widgets/clock.js",
    ] {
        assert!(
            !unused.contains(&used.to_string()),
            "a `../` pattern in src/pages/index.js loads {used}, found {unused:?}"
        );
    }
}

#[test]
fn parent_directory_excludes_remove_their_targets() {
    let unused = unused_files();
    for excluded in [
        "src/components/draft.js",
        "src/components/skip.js",
        "src/orphan.js",
    ] {
        assert!(
            unused.contains(&excluded.to_string()),
            "no pattern loads {excluded}, found {unused:?}"
        );
    }
}

/// A root behind a symlink is not canonical, so the resolver matches the
/// canonical file paths and checks them against the canonical root.
#[cfg(unix)]
#[test]
fn parent_directory_patterns_resolve_under_a_symlinked_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let link = dir.path().join("project");
    std::os::unix::fs::symlink(fixture_path("dynamic-import-parent-patterns"), &link)
        .expect("symlink fixture root");
    let unused = unused_files_at(link);
    assert_eq!(
        unused,
        vec![
            "src/components/draft.js".to_string(),
            "src/components/skip.js".to_string(),
            "src/orphan.js".to_string(),
        ]
    );
}
