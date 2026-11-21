//! Clone-region attribution in `fallow audit`: one paste is one finding, an
//! edit in every copy of a base clone is `inherited-edited`, and an
//! `ignoredClones` entry follows an edited clone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const GATE_CONFIG: &str = r#"{"rules":{"introduced-clone-group":"error"}}"#;

const HAS_EXACT_FIELDS: &str = "export function hasExactFields(value: unknown, fields: ReadonlyArray<string>): boolean {\n\treturn isObject(value) && Reflect.ownKeys(value).length === fields.length && fields.every((field) => Object.hasOwn(value, field))\n}\n\nfunction isObject(value: unknown): value is Record<string, unknown> {\n\treturn value !== null && typeof value === \"object\" && !Array.isArray(value)\n}";

const SHA256_LABEL: &str = "export function sha256Label(bytes: Uint8Array): string {\n\tconst digest = Array.from(bytes, (byte) => byte.toString(16).padStart(2, \"0\")).join(\"\")\n\treturn `sha256:${digest}`\n}\n";

fn new_repo(name: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let repo = tmp.path();
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join("package.json"),
        format!(r#"{{"name":"{name}","private":true,"type":"module"}}"#),
    )
    .unwrap();
    fs::write(repo.join(".fallowrc.json"), GATE_CONFIG).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "core.autocrlf", "false"]);
    tmp
}

fn audit(repo: &Path, base: &str) -> (CommandOutput, serde_json::Value) {
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        repo.to_str().unwrap(),
        "--only",
        "dupes",
        "--base",
        base,
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&output);
    (output, json)
}

fn regions(json: &serde_json::Value) -> &Vec<serde_json::Value> {
    json["duplication"]["clone_regions"]
        .as_array()
        .unwrap_or_else(|| panic!("expected duplication.clone_regions: {json}"))
}

/// The `ignoredClones` key of the only clone group of a `fallow dupes` run.
fn only_ignore_key(repo: &Path) -> String {
    let output = run_fallow_raw(&[
        "dupes",
        "--root",
        repo.to_str().unwrap(),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().expect("clone groups");
    assert_eq!(groups.len(), 1, "expected one base clone group: {json}");
    format!(
        "{}:{}",
        groups[0]["fingerprint"].as_str().unwrap(),
        groups[0]["instances"].as_array().unwrap().len()
    )
}

/// The base holds the block in `a.ts` (after another function) and its core
/// in `c.ts`. The head copies `a.ts` into `d.ts`. The detector reports a long
/// group (`a.ts`, `d.ts`) and a short group inside it (`a.ts`, `c.ts`,
/// `d.ts`), but the change made one copy.
fn nested_paste_repo() -> TempDir {
    let tmp = new_repo("overlap");
    let repo = tmp.path();
    fs::write(
        repo.join("src/a.ts"),
        format!("{SHA256_LABEL}\n{HAS_EXACT_FIELDS}\n"),
    )
    .unwrap();
    fs::write(
        repo.join("src/c.ts"),
        format!("export const c = 1\n\n{HAS_EXACT_FIELDS}\n"),
    )
    .unwrap();
    commit_all(repo, "base");
    fs::copy(repo.join("src/a.ts"), repo.join("src/d.ts")).unwrap();
    commit_all(repo, "copy a.ts into d.ts");
    tmp
}

/// Issue #4967: the nested groups of one paste are one region and one gate
/// finding.
#[test]
fn nested_paste_is_one_introduced_region() {
    let tmp = nested_paste_repo();
    let (output, json) = audit(tmp.path(), "HEAD~1");
    assert_eq!(output.code, 1, "the paste must fail the gate: {json}");
    let groups = json["duplication"]["clone_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "the detector reports two groups: {json}");
    assert_eq!(json["attribution"]["duplication_introduced"], 1, "{json}");
    let regions = regions(&json);
    assert_eq!(regions.len(), 1, "{json}");
    assert_eq!(regions[0]["grade"], "introduced");
    assert_eq!(regions[0]["groups"].as_array().unwrap().len(), 2);
    assert!(
        groups
            .iter()
            .all(|group| group["region"] == 0 && group["introduced"] == true),
        "{json}"
    );
}

/// Issue #4967: one `ignoredClones` entry for the outer group accepts the
/// region, because the other member only adds a copy that the base already
/// had.
#[test]
fn ignored_outer_group_accepts_the_nested_paste() {
    let tmp = nested_paste_repo();
    let repo = tmp.path();
    let (_, json) = audit(repo, "HEAD~1");
    let outer = json["duplication"]["clone_groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["instances"].as_array().unwrap().len() == 2)
        .expect("the outer group");
    let key = format!("{}:2", outer["fingerprint"].as_str().unwrap());
    fs::write(
        repo.join(".fallowrc.json"),
        format!(
            r#"{{"rules":{{"introduced-clone-group":"error"}},"duplicates":{{"ignoredClones":["{key}"]}}}}"#
        ),
    )
    .unwrap();
    let (output, json) = audit(repo, "HEAD~1");
    assert_eq!(output.code, 0, "the accepted paste must pass: {json}");
    assert_eq!(json["attribution"]["duplication_introduced"], 0, "{json}");
    let regions = regions(&json);
    assert_eq!(regions.len(), 1, "{json}");
    assert_eq!(regions[0]["accepted_by"], serde_json::json!([key]));
}

fn describe_scenarios(extra: &str, message: &str) -> String {
    format!(
        "export function describeScenarios(results: ReadonlyArray<{{ name: string; outcome: string }}>): string {{\n\tconst expected = [\n\t\t{{ name: \"unmigrated\", outcome: \"pass\" }},\n\t\t{{ name: \"baseline\", outcome: \"pass\" }},\n\t\t{{ name: \"ownership\", outcome: \"pass\" }},\n\t\t{{ name: \"graceful\", outcome: \"pass\" }},\n\t\t{{ name: \"restart\", outcome: \"pass\" }},{extra}\n\t]\n\tif (JSON.stringify(results) !== JSON.stringify(expected)) throw new Error(\"{message}\")\n\treturn results.map((result) => `${{result.name}}: ${{result.outcome}}`).join(\"\\n\")\n}}\n"
    )
}

/// The base holds one clone in `a.ts` and `b.ts`. `edit` writes the head.
fn audit_after_edit(edit: impl FnOnce(&Path)) -> (CommandOutput, serde_json::Value) {
    let tmp = new_repo("edit");
    let repo = tmp.path();
    let body = describe_scenarios("", "Incomplete baseline");
    fs::write(repo.join("src/a.ts"), &body).unwrap();
    fs::write(repo.join("src/b.ts"), &body).unwrap();
    commit_all(repo, "base");
    edit(repo);
    commit_all(repo, "edit");
    audit(repo, "HEAD~1")
}

fn assert_one_region(json: &serde_json::Value, grade: &str) {
    let regions = regions(json);
    assert_eq!(regions.len(), 1, "{json}");
    assert_eq!(regions[0]["grade"], grade, "{json}");
    assert_eq!(json["attribution"]["duplication_introduced"], 0, "{json}");
}

/// Issue #4968, case 1: the same one-line edit in both copies.
#[test]
fn same_edit_in_every_copy_is_inherited_edited() {
    let (output, json) = audit_after_edit(|repo| {
        let body = describe_scenarios("", "Incomplete scenario baseline");
        fs::write(repo.join("src/a.ts"), &body).unwrap();
        fs::write(repo.join("src/b.ts"), &body).unwrap();
    });
    assert_eq!(output.code, 0, "an edit in every copy must pass: {json}");
    assert_one_region(&json, "inherited-edited");
}

/// Issue #4968, control: the edit in one copy only.
#[test]
fn edit_in_one_copy_is_not_introduced() {
    let (output, json) = audit_after_edit(|repo| {
        let body = describe_scenarios("", "Incomplete scenario baseline");
        fs::write(repo.join("src/a.ts"), body).unwrap();
    });
    assert_eq!(output.code, 0, "{json}");
    assert!(
        regions(&json)
            .iter()
            .all(|region| region["grade"] != "introduced"),
        "{json}"
    );
    assert_eq!(json["attribution"]["duplication_introduced"], 0, "{json}");
}

/// Issue #4968, case 2: one line added to both copies.
#[test]
fn line_added_to_every_copy_is_inherited_edited() {
    let (output, json) = audit_after_edit(|repo| {
        let body = describe_scenarios(
            "\n\t\t{ name: \"backup\", outcome: \"pass\" },",
            "Incomplete baseline",
        );
        fs::write(repo.join("src/a.ts"), &body).unwrap();
        fs::write(repo.join("src/b.ts"), &body).unwrap();
    });
    assert_eq!(output.code, 0, "{json}");
    assert_one_region(&json, "inherited-edited");
}

/// Issue #4968, case 3: the base accepts the clone in `ignoredClones`, and the
/// head edits both copies. The region stays accepted, and the output names
/// the new key for the config.
#[test]
fn accepted_clone_stays_accepted_after_an_edit_in_every_copy() {
    let tmp = new_repo("accepted");
    let repo = tmp.path();
    let body = describe_scenarios("", "Incomplete baseline");
    fs::write(repo.join("src/a.ts"), &body).unwrap();
    fs::write(repo.join("src/b.ts"), &body).unwrap();
    let key = only_ignore_key(repo);
    fs::write(
        repo.join(".fallowrc.json"),
        format!(
            r#"{{"rules":{{"introduced-clone-group":"error"}},"duplicates":{{"ignoredClones":["{key}"]}}}}"#
        ),
    )
    .unwrap();
    commit_all(repo, "accept clone");
    let edited = describe_scenarios("", "Incomplete scenario baseline");
    fs::write(repo.join("src/a.ts"), &edited).unwrap();
    fs::write(repo.join("src/b.ts"), &edited).unwrap();
    commit_all(repo, "edit both copies");

    let (output, json) = audit(repo, "HEAD~1");
    assert_eq!(output.code, 0, "the accepted clone must pass: {json}");
    assert_one_region(&json, "inherited-edited");
    let region = &regions(&json)[0];
    assert_eq!(region["accepted_by"], serde_json::json!([key]), "{json}");
    let group = &json["duplication"]["clone_groups"][0];
    let new_key = format!("{}:2", group["fingerprint"].as_str().unwrap());
    assert_ne!(new_key, key);
    assert_eq!(
        region["ignore_hint"],
        serde_json::json!([new_key]),
        "{json}"
    );

    let human = run_fallow_raw(&[
        "audit",
        "--root",
        repo.to_str().unwrap(),
        "--only",
        "dupes",
        "--base",
        "HEAD~1",
        "--no-cache",
    ]);
    assert_eq!(human.code, 0, "{}", human.stdout);
    assert!(
        human.stdout.contains(&new_key) || human.stderr.contains(&new_key),
        "the human output must print the new key {new_key}:\n{}\n{}",
        human.stdout,
        human.stderr
    );
}
