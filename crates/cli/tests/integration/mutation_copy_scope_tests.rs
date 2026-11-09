use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::{MATH, commit_all, git, json_mutation, untested_project};
use crate::common::git_capture;

fn lines(report: &serde_json::Value, file: &str) -> BTreeSet<u64> {
    report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["file"] == file)
        .map(|mutant| mutant["line"].as_u64().expect("line"))
        .collect()
}

fn plan(root: &Path, args: &[&str]) -> serde_json::Value {
    let mut flags = vec!["--plan", "--no-cache"];
    flags.extend_from_slice(args);
    let (output, report) = json_mutation(root, &flags);
    assert_eq!(output.code, 0, "{}", output.stderr);
    report
}

#[test]
fn generated_copy_scope_keeps_edits_and_additions_but_supplied_hunks_stay_literal() {
    let project = untested_project();
    let root = project.path();
    git(root, &["config", "diff.renames", "false"]);
    fs::write(root.join("src/exact.ts"), MATH).unwrap();
    fs::write(root.join("src/edited.ts"), MATH.replace("a + b", "a - b")).unwrap();
    fs::write(
        root.join("src/math.ts"),
        "export { add, isPositive } from './exact';\n",
    )
    .unwrap();
    fs::write(
        root.join("src/added.ts"),
        "export function twice(n: number) {\n  return n * 2;\n}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    fs::write(
        root.join("src/untracked.ts"),
        "export function half(n: number) {\n  return n / 2;\n}\n",
    )
    .unwrap();

    let generated = plan(root, &["--changed-since", "HEAD"]);
    assert_eq!(lines(&generated, "src/added.ts"), BTreeSet::from([2]));
    assert_eq!(lines(&generated, "src/untracked.ts"), BTreeSet::from([2]));
    assert_eq!(lines(&generated, "src/edited.ts"), BTreeSet::from([2]));
    assert!(lines(&generated, "src/exact.ts").is_empty(), "{generated}");

    let patches = tempfile::tempdir().unwrap();
    for (name, flags, expected) in [
        ("plain", vec!["--no-renames"], BTreeSet::from([2, 6])),
        ("copy", vec!["-M", "-C"], BTreeSet::from([2])),
    ] {
        let mut args = vec!["diff", "--unified=0"];
        args.extend(flags);
        args.push("HEAD");
        let patch = patches.path().join(format!("{name}.diff"));
        fs::write(&patch, git_capture(root, &args)).unwrap();
        let supplied = plan(root, &["--diff-file", patch.to_str().unwrap()]);
        assert_eq!(lines(&supplied, "src/edited.ts"), expected);
        assert_eq!(lines(&supplied, "src/added.ts"), BTreeSet::from([2]));
        assert_eq!(
            lines(&supplied, "src/exact.ts"),
            if name == "plain" {
                BTreeSet::from([2, 6])
            } else {
                BTreeSet::new()
            }
        );
    }

    // The working tree, including edits made after staging, defines the scope.
    fs::write(root.join("src/edited.ts"), MATH.replace("n > 0", "n >= 0")).unwrap();
    let unstaged = plan(root, &["--changed-since", "HEAD"]);
    assert_eq!(lines(&unstaged, "src/edited.ts"), BTreeSet::from([6]));
    assert_eq!(lines(&unstaged, "src/added.ts"), BTreeSet::from([2]));
}

#[test]
fn generated_rename_scope_ignores_disabled_rename_config() {
    let project = untested_project();
    let root = project.path();
    git(root, &["config", "diff.renames", "false"]);
    git(root, &["mv", "src/math.ts", "src/moved.ts"]);
    fs::write(root.join("src/moved.ts"), MATH.replace("a + b", "a - b")).unwrap();
    let report = plan(root, &["--changed-since", "HEAD"]);
    assert_eq!(lines(&report, "src/moved.ts"), BTreeSet::from([2]));
}

#[test]
fn generated_copy_scope_maps_nested_roots_and_excludes_outside_destinations() {
    const FOREIGN: &str = "export function foreignScale(value: number): number {\n  return value * 123;\n}\n\nexport function foreignLimit(value: number): boolean {\n  return value < 456;\n}\n";
    let project = untested_project();
    let root = project.path();
    let package = root.join("packages/lib");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"name":"lib","main":"src/source.ts"}"#,
    )
    .unwrap();
    fs::write(package.join("src/source.ts"), MATH).unwrap();
    fs::write(root.join("src/foreign.ts"), FOREIGN).unwrap();
    commit_all(root, "package source");
    fs::write(package.join("src/copy.ts"), MATH.replace("a + b", "a - b")).unwrap();
    fs::write(
        package.join("src/source.ts"),
        "export { add, isPositive } from './copy';\n",
    )
    .unwrap();
    fs::write(root.join("outside.ts"), MATH).unwrap();
    git(root, &["add", "."]);
    let report = plan(&package, &["--changed-since", "HEAD"]);
    assert_eq!(lines(&report, "src/copy.ts"), BTreeSet::from([2]));
    assert!(
        report["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["file"] == "src/copy.ts"),
        "{report}"
    );

    fs::write(package.join("src/incoming.ts"), FOREIGN).unwrap();
    fs::write(
        root.join("src/foreign.ts"),
        "export { foreignScale, foreignLimit } from '../packages/lib/src/incoming';\n",
    )
    .unwrap();
    commit_all(root, "copy across package boundary");
    let nested = plan(&package, &["--changed-since", "HEAD~1"]);
    assert_eq!(lines(&nested, "src/copy.ts"), BTreeSet::from([2]));
    // Git's relative diff cannot establish ancestry from outside this root.
    assert_eq!(lines(&nested, "src/incoming.ts"), BTreeSet::from([2, 6]));
    let repository = plan(root, &["--changed-since", "HEAD~1"]);
    assert_eq!(
        lines(&repository, "packages/lib/src/copy.ts"),
        BTreeSet::from([2])
    );
    assert!(
        lines(&repository, "packages/lib/src/incoming.ts").is_empty(),
        "{repository}"
    );
}
