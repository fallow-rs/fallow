//! `public-package-candidate`: a workspace package that looks published but
//! that `publicPackages` does not list (issue #5335). Every output carries
//! the hint in one place, and a saved envelope renders the same hint.

use std::path::Path;

use serde_json::Value;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

const KIND: &str = "public-package-candidate";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write file");
}

/// A monorepo with a package `@x/sdk` that looks published. Its entry
/// re-exports an enum with one unused member.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"mono","private":true,"workspaces":["packages/*","apps/*"]}"#,
    );
    write(
        root,
        "packages/sdk/package.json",
        r#"{"name":"@x/sdk","exports":{".":"./src/index.ts"}}"#,
    );
    write(
        root,
        "packages/sdk/src/index.ts",
        "export * from './enums';\n",
    );
    write(
        root,
        "packages/sdk/src/enums.ts",
        "export enum Mode { On = 'on', Off = 'off' }\n",
    );
    write(
        root,
        "apps/web/package.json",
        r#"{"name":"web","private":true,"main":"./src/main.ts","dependencies":{"@x/sdk":"*"}}"#,
    );
    write(
        root,
        "apps/web/src/main.ts",
        "import { Mode } from '@x/sdk';\nconsole.log(Mode.On);\n",
    );
    dir
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let root = root.to_string_lossy();
    let mut all = vec![args[0], "--root", root.as_ref(), "--no-cache"];
    all.extend_from_slice(&args[1..]);
    run_fallow_raw(&all)
}

fn entries(envelope: &Value) -> Vec<Value> {
    envelope["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["kind"] == KIND)
        .cloned()
        .collect()
}

fn notifications(sarif: &Value) -> Vec<Value> {
    sarif["runs"][0]["invocations"][0]["toolConfigurationNotifications"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|notification| notification["descriptor"]["id"] == KIND)
        .cloned()
        .collect()
}

#[test]
fn every_output_carries_the_hint_and_no_finding_changes() {
    let dir = project();
    let root = dir.path();

    let output = run(root, &["dead-code", "--format", "json", "--quiet"]);
    let envelope = parse_json(&output);
    let found = entries(&envelope);
    assert_eq!(found.len(), 1, "{envelope}");
    assert_eq!(found[0]["package"], "@x/sdk");
    assert_eq!(found[0]["findings"], 1);
    assert_eq!(found[0]["path"], "packages/sdk");
    assert_eq!(
        envelope["unused_enum_members"].as_array().map(Vec::len),
        Some(1)
    );

    let sarif = parse_json(&run(root, &["dead-code", "--format", "sarif", "--quiet"]));
    let notes = notifications(&sarif);
    assert_eq!(notes.len(), 1, "{sarif}");
    assert_eq!(notes[0]["level"], "note");
    assert_eq!(notes[0]["properties"]["setting"], "publicPackages");
    assert_eq!(notes[0]["properties"]["package"], "@x/sdk");

    let human = run(root, &["dead-code"]);
    assert!(
        human.stderr.contains(
            "Note: these workspace packages look published, but publicPackages does not list \
             them: @x/sdk (1 finding on the exports or members of its entry)."
        ),
        "{}",
        human.stderr
    );

    let saved = root.join("saved.json");
    std::fs::write(&saved, &output.stdout).expect("save envelope");
    let saved = saved.to_string_lossy();
    let rendered = run(
        root,
        &[
            "report",
            "--from",
            saved.as_ref(),
            "--format",
            "sarif",
            "--quiet",
        ],
    );
    assert_eq!(notifications(&parse_json(&rendered)), notes);
    let markdown = run(
        root,
        &[
            "report",
            "--from",
            saved.as_ref(),
            "--format",
            "markdown",
            "--quiet",
        ],
    );
    assert!(
        markdown
            .stdout
            .contains("- `@x/sdk`: 1 finding on the exports or members of its entry"),
        "{}",
        markdown.stdout
    );
}

#[test]
fn a_listed_package_gets_no_hint() {
    let dir = project();
    let root = dir.path();
    write(root, ".fallowrc.json", r#"{"publicPackages":["@x/sdk"]}"#);

    let envelope = parse_json(&run(root, &["dead-code", "--format", "json", "--quiet"]));

    assert!(entries(&envelope).is_empty(), "{envelope}");
    assert_eq!(
        envelope["unused_enum_members"].as_array().map(Vec::len),
        Some(0)
    );
}

fn packages(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["dead-code", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    entries(&parse_json(&run(root, &args)))
        .iter()
        .map(|entry| entry["package"].as_str().expect("package").to_owned())
        .collect()
}

#[test]
fn a_workspace_scope_keeps_only_the_hints_inside_it() {
    let dir = project();
    let root = dir.path();

    assert_eq!(packages(root, &["--workspace", "@x/sdk"]), vec!["@x/sdk"]);
    assert!(packages(root, &["--workspace", "web"]).is_empty());
}

#[test]
fn a_change_scope_drops_the_hint() {
    let dir = project();
    let root = dir.path();
    git(root, &["init", "-q"]);
    commit_all(root, "initial");
    write(
        root,
        "apps/web/src/main.ts",
        "import { Mode } from '@x/sdk';\nconsole.log(Mode.On, 1);\n",
    );

    assert!(packages(root, &["--changed-since", "HEAD"]).is_empty());
    assert_eq!(packages(root, &[]), vec!["@x/sdk"]);
}
