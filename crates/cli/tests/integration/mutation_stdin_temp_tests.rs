use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

use crate::common::{commit_all, git, git_capture};

fn fixture(package: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    let root = repo.join(package);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(repo.join("tmp")).unwrap();
    fs::write(repo.join("tmp/.gitkeep"), "").unwrap();
    fs::write(repo.join(".gitignore"), ".fallow\n").unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"p","main":"src/math.js"}"#,
    )
    .unwrap();
    fs::write(
        root.join("src/math.js"),
        "export function add(a, b) { return a + b; }\n",
    )
    .unwrap();
    git(&repo, &["init", "-q"]);
    commit_all(&repo, "fixture");
    directory
}

fn entries(path: &Path) -> BTreeSet<OsString> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}

fn verify(directory: &Path, package: &str, temp: &Path) {
    let repo = directory.join("repo");
    let root = repo.join(package);
    let patch = tempfile::NamedTempFile::new_in(directory).unwrap();
    let file = if package.is_empty() {
        "src/math.js".to_owned()
    } else {
        format!("{package}/src/math.js")
    };
    let text = format!("--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n-a\n+b\n");
    fs::write(patch.path(), &text).unwrap();
    let before = entries(directory);
    let temp_before = entries(&repo.join("tmp"));
    assert_eq!(git_capture(&repo, &["status", "--porcelain"]), "");
    let (code, expected, stderr) = super::invoke_env(
        &root,
        &["--diff-file", patch.path().to_str().unwrap()],
        "",
        None,
        Some(temp),
    );
    assert_eq!(code, 0, "{expected} {stderr}");
    assert!(
        expected["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mutant| mutant["file"] == "src/math.js")
    );
    for flags in [vec!["--diff-stdin"], vec!["--diff-file", "-"]] {
        let (code, report, stderr) = super::invoke_env(&root, &flags, &text, None, Some(temp));
        assert_eq!(code, 0, "{report} {stderr}");
        assert_eq!(report["mutants"], expected["mutants"]);
        assert_eq!(report["worktree"]["checked"], true);
        assert_eq!(entries(directory), before);
        assert_eq!(entries(&repo.join("tmp")), temp_before);
        assert_eq!(git_capture(&repo, &["status", "--porcelain"]), "");
    }
    let (code, report, stderr) = super::invoke_env(
        &root,
        &["--plan", "--resume", "--diff-stdin"],
        &text,
        None,
        Some(temp),
    );
    assert_eq!(code, 2, "{report} {stderr}");
    assert_eq!(report["code"], "FALLOW_INVALID_MUTATION_OPTIONS");
    assert_eq!(entries(directory), before);
    assert_eq!(entries(&repo.join("tmp")), temp_before);
    assert_eq!(git_capture(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn stdin_temp_storage_stays_outside_root_and_nested_worktrees() {
    for package in ["", "packages/app"] {
        let directory = fixture(package);
        verify(
            directory.path(),
            package,
            &directory.path().join("repo/tmp"),
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_temp_storage_cannot_place_the_bridge_inside_the_worktree() {
    let package = "packages/app";
    let directory = fixture(package);
    let alias = directory.path().join("temp-alias");
    std::os::unix::fs::symlink(directory.path().join("repo/tmp"), &alias).unwrap();
    verify(directory.path(), package, &alias);
}
