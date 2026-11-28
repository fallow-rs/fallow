#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fix` plans only the findings that `dead-code` reports with the same scope
//! flags: `--workspace`, `--changed-workspaces`, `--changed-since` and a diff.

use std::path::Path;

use crate::common::{commit_all, git, parse_json, run_fallow_in_root};

/// Two workspace packages, each with one unused export in `lib.js`.
fn write_two_package_project(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    for name in ["a", "b"] {
        let dir = root.join("packages").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"@x/{name}","private":true,"main":"index.js"}}"#),
        )
        .unwrap();
        std::fs::write(
            dir.join("index.js"),
            "import { used } from './lib.js';\nconsole.log(used);\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("lib.js"),
            format!("export const used = 1;\nexport const unused_{name} = 2;\n"),
        )
        .unwrap();
    }
}

/// The paths of the planned export fixes, sorted.
fn planned_export_paths(root: &Path, args: &[&str]) -> Vec<String> {
    let mut all_args = vec!["--dry-run", "--format", "json", "--quiet"];
    all_args.extend_from_slice(args);
    let output = run_fallow_in_root("fix", root, &all_args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let json = parse_json(&output);
    let mut paths: Vec<String> = json["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_export")
        .map(|fix| fix["path"].as_str().unwrap().to_string())
        .collect();
    paths.sort();
    paths
}

#[test]
fn fix_without_scope_flags_plans_every_package() {
    let dir = tempfile::tempdir().unwrap();
    write_two_package_project(dir.path());
    assert_eq!(
        planned_export_paths(dir.path(), &[]),
        ["packages/a/lib.js", "packages/b/lib.js"]
    );
}

#[test]
fn fix_with_workspace_plans_only_that_package() {
    let dir = tempfile::tempdir().unwrap();
    write_two_package_project(dir.path());
    assert_eq!(
        planned_export_paths(dir.path(), &["--workspace", "@x/a"]),
        ["packages/a/lib.js"]
    );
}

#[test]
fn fix_with_an_unknown_workspace_is_an_input_error() {
    let dir = tempfile::tempdir().unwrap();
    write_two_package_project(dir.path());
    let output = run_fallow_in_root(
        "fix",
        dir.path(),
        &[
            "--dry-run",
            "--format",
            "json",
            "--quiet",
            "--workspace",
            "@x/none",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
}

#[test]
fn fix_with_changed_since_plans_only_changed_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_two_package_project(root);
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");
    git(root, &["checkout", "-b", "feature"]);
    std::fs::write(
        root.join("packages/b/lib.js"),
        "export const used = 1;\nexport const unused_b = 3;\n",
    )
    .unwrap();
    commit_all(root, "change b");

    assert_eq!(
        planned_export_paths(root, &["--changed-since", "main"]),
        ["packages/b/lib.js"]
    );
    assert_eq!(
        planned_export_paths(root, &["--changed-workspaces", "main"]),
        ["packages/b/lib.js"]
    );
}

#[test]
fn fix_with_a_diff_file_plans_only_lines_in_the_diff() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_two_package_project(root);
    let diff = root.join("change.diff");
    std::fs::write(
        &diff,
        "diff --git a/packages/a/lib.js b/packages/a/lib.js\n\
         --- a/packages/a/lib.js\n\
         +++ b/packages/a/lib.js\n\
         @@ -1,2 +1,2 @@\n \
         export const used = 1;\n\
         -export const unused_a = 1;\n\
         +export const unused_a = 2;\n",
    )
    .unwrap();
    assert_eq!(
        planned_export_paths(root, &["--diff-file", diff.to_str().unwrap()]),
        ["packages/a/lib.js"]
    );
}
