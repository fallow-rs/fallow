//! `override-files-unmatched`: the `overrides[].files` globs that match no
//! project file (issue #5227).

use std::path::Path;

use serde_json::{Value, json};

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

const KIND: &str = "override-files-unmatched";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write file");
}

fn set_overrides(root: &Path, overrides: &Value) {
    write(
        root,
        ".fallowrc.json",
        &json!({ "overrides": overrides }).to_string(),
    );
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let root = root.to_string_lossy();
    let mut all = vec![args[0], "--root", root.as_ref(), "--no-cache"];
    all.extend_from_slice(&args[1..]);
    run_fallow_raw(&all)
}

fn json_run(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["dead-code", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    parse_json(&run(root, &args))
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

/// `(pattern, reason)` of each entry.
fn patterns(envelope: &Value) -> Vec<(String, String)> {
    entries(envelope)
        .iter()
        .map(|entry| {
            (
                entry["pattern"].as_str().expect("pattern").to_owned(),
                entry["reason"].as_str().expect("reason").to_owned(),
            )
        })
        .collect()
}

fn pair(pattern: &str, reason: &str) -> (String, String) {
    (pattern.to_owned(), reason.to_owned())
}

/// The project of issue #5227: one file imports a stylesheet that does not
/// exist.
fn issue_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "import './missing.css';\n");
    write(root, "README.md", "# repro\n");
    dir
}

const ISSUE_GLOB: &str = "./vuetify/lib/styles/layers.css";

fn issue_override() -> Value {
    json!([{ "files": [ISSUE_GLOB], "rules": { "unresolved-import": "off" } }])
}

#[test]
fn an_import_target_glob_is_reported_with_the_key_that_works() {
    let project = issue_project();
    let root = project.path();
    set_overrides(root, &issue_override());

    let envelope = json_run(root, &[]);
    assert_eq!(
        envelope["unresolved_imports"].as_array().map(Vec::len),
        Some(1),
        "the override does not hide the finding: {envelope}"
    );
    let found = entries(&envelope);
    assert_eq!(found.len(), 1, "{envelope}");
    let entry = &found[0];
    assert_eq!(entry["pattern"], ISSUE_GLOB);
    assert_eq!(entry["files"], json!([ISSUE_GLOB]));
    assert_eq!(entry["rules"], json!(["unresolved-imports"]));
    assert_eq!(entry["reason"], "path-not-found");
    assert_eq!(entry["path"], ".");
    let message = entry["message"].as_str().expect("message");
    assert!(message.contains("matched no project file"), "{message}");
    assert!(message.contains("the importing file"), "{message}");
    assert!(message.contains("`ignoreUnresolvedImports`"), "{message}");

    write(
        root,
        ".fallowrc.json",
        &json!({ "ignoreUnresolvedImports": ["./missing.css"] }).to_string(),
    );
    let remedy = json_run(root, &[]);
    assert_eq!(
        remedy["unresolved_imports"].as_array().map_or(0, Vec::len),
        0,
        "the key that the message names hides the finding: {remedy}"
    );
    assert!(entries(&remedy).is_empty(), "{remedy}");
}

#[test]
fn a_glob_that_matches_a_project_file_gives_no_entry() {
    let project = issue_project();
    let root = project.path();
    set_overrides(
        root,
        &json!([
            { "files": ["src/**/*.ts"], "rules": { "unresolved-import": "off" } },
            { "files": ["README.md"], "rules": { "unused-files": "off" } },
        ]),
    );
    let envelope = json_run(root, &[]);
    assert!(entries(&envelope).is_empty(), "{envelope}");
}

#[test]
fn each_reason_names_its_cause() {
    let project = issue_project();
    let root = project.path();
    set_overrides(
        root,
        &json!([
            { "files": ["src/**/*.ts", "lib/**/*.ts"], "rules": { "unused-exports": "off" } },
            { "files": ["src"], "rules": { "unused-files": "off" } },
            { "files": ["typedoc", "@golar/vue"], "rules": { "unused-files": "off" } },
        ]),
    );
    let envelope = json_run(root, &[]);
    assert_eq!(
        patterns(&envelope),
        vec![
            pair("lib/**/*.ts", "glob-unmatched"),
            pair("src", "directory"),
            pair("typedoc", "not-a-path"),
            pair("@golar/vue", "not-a-path"),
        ],
        "{envelope}"
    );
    let found = entries(&envelope);
    assert_eq!(
        found[0]["files"],
        json!(["src/**/*.ts", "lib/**/*.ts"]),
        "the entry names every glob of the override"
    );
    let directory = found[1]["message"].as_str().expect("message");
    assert!(directory.contains("Use 'src/**'"), "{directory}");
    let name = found[2]["message"].as_str().expect("message");
    assert!(name.contains("`ignoreDependencies`"), "{name}");
    assert!(name.contains("`ignoreBinaries`"), "{name}");
}

#[test]
fn an_override_of_project_level_rules_only_gets_the_rule_note() {
    let project = issue_project();
    let root = project.path();
    set_overrides(
        root,
        &json!([{ "files": ["dot.exe"], "rules": { "unlisted-binary": "off" } }]),
    );
    let envelope = json_run(root, &[]);
    assert!(entries(&envelope).is_empty(), "{envelope}");
    let rule_notes = envelope["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
        .count();
    assert_eq!(rule_notes, 1, "{envelope}");
}

/// A monorepo with two packages. The first override matches only a file of
/// package `b`, the second matches nothing.
fn monorepo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"mono","private":true,"workspaces":["packages/*"]}"#,
    );
    write(
        root,
        "packages/a/package.json",
        r#"{"name":"a","main":"src/index.ts"}"#,
    );
    write(root, "packages/a/src/index.ts", "export const a = 1;\n");
    write(
        root,
        "packages/b/package.json",
        r#"{"name":"b","main":"src/index.ts"}"#,
    );
    write(root, "packages/b/src/index.ts", "export const b = 1;\n");
    write(root, "packages/b/src/b.test.ts", "export const t = 1;\n");
    set_overrides(
        root,
        &json!([
            { "files": ["packages/b/src/**/*.test.ts"], "rules": { "unused-files": "off" } },
            { "files": ["packages/c/**"], "rules": { "unused-files": "off" } },
        ]),
    );
    git(root, &["init", "-q"]);
    commit_all(root, "initial");
    write(root, "packages/a/src/index.ts", "export const a = 2;\n");
    dir
}

/// The check reads every project file, so a scope flag cannot make a glob
/// look stale, and the stale glob stays reported in every scope.
#[test]
fn the_answer_does_not_depend_on_the_scope_of_the_run() {
    let project = monorepo();
    let root = project.path();
    let expected = vec![pair("packages/c/**", "glob-unmatched")];
    for extra in [
        &[][..],
        &["--changed-since", "HEAD"][..],
        &["--workspace", "a"][..],
        &["--production"][..],
    ] {
        let envelope = json_run(root, extra);
        assert_eq!(patterns(&envelope), expected, "{extra:?}: {envelope}");
    }
}

#[test]
fn human_output_has_one_note_line_per_glob() {
    let project = issue_project();
    let root = project.path();
    set_overrides(root, &issue_override());
    let output = run(root, &["dead-code"]);
    assert!(
        output.stderr.contains(
            "Note: overrides[].files glob matched no file this run: \
             ./vuetify/lib/styles/layers.css (the override applies to the importing file; add \
             the import specifier to ignoreUnresolvedImports)."
        ),
        "{}",
        output.stderr
    );

    let quiet = run(root, &["dead-code", "--quiet"]);
    assert!(
        !quiet.stderr.contains("overrides[].files"),
        "--quiet hides the note: {}",
        quiet.stderr
    );
}

#[test]
fn sarif_and_markdown_carry_the_entry() {
    let project = issue_project();
    let root = project.path();
    set_overrides(root, &issue_override());

    let sarif = parse_json(&run(root, &["dead-code", "--format", "sarif", "--quiet"]));
    let notifications = sarif["runs"][0]["invocations"][0]["toolConfigurationNotifications"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let notification = notifications
        .iter()
        .find(|notification| notification["descriptor"]["id"] == KIND)
        .unwrap_or_else(|| panic!("no SARIF notification: {sarif}"));
    assert_eq!(notification["properties"]["setting"], "overrides");
    assert_eq!(notification["properties"]["pattern"], ISSUE_GLOB);
    assert_eq!(notification["properties"]["reason"], "path-not-found");

    let markdown = run(root, &["dead-code", "--format", "markdown", "--quiet"]);
    assert!(
        markdown
            .stdout
            .contains("## Override globs that match no file"),
        "{}",
        markdown.stdout
    );
    assert!(
        markdown
            .stdout
            .contains("- `./vuetify/lib/styles/layers.css`: the override"),
        "{}",
        markdown.stdout
    );
    assert!(
        !markdown.stderr.contains("overrides[].files"),
        "the document carries the entry, so stderr has no note: {}",
        markdown.stderr
    );
}

/// A stale glob is a typo, not an import target, so it gets the typo hint
/// also when the override sets `unresolved-imports`. The note puts the globs
/// with the same hint on one line.
#[test]
fn a_stale_glob_gets_the_typo_hint_and_one_grouped_line() {
    let project = issue_project();
    let root = project.path();
    set_overrides(
        root,
        &json!([
            { "files": ["nope/**", "gone/*.ts"], "rules": { "unresolved-import": "off" } },
            { "files": ["vendor/x.css"], "rules": { "unresolved-import": "off" } },
        ]),
    );
    let envelope = json_run(root, &[]);
    let found = entries(&envelope);
    assert_eq!(found.len(), 3, "{envelope}");
    let glob_message = found[0]["message"].as_str().expect("message");
    assert!(glob_message.contains("Fix the glob"), "{glob_message}");
    assert!(
        !glob_message.contains("ignoreUnresolvedImports"),
        "a glob with glob characters is not an import target: {glob_message}"
    );

    let output = run(root, &["dead-code"]);
    assert!(
        output.stderr.contains(
            "Note: overrides[].files globs matched no file this run: nope/**, gone/*.ts \
             (globs are project-root-relative; check for typos, or remove the glob)."
        ),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains(
            "Note: overrides[].files glob matched no file this run: vendor/x.css (the \
             override applies to the importing file; add the import specifier to \
             ignoreUnresolvedImports)."
        ),
        "{}",
        output.stderr
    );
}
