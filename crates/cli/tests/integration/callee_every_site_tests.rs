//! `banned-call`, `banned-effect` and `boundaries.calls.forbidden` report each
//! call site, not only the first site of a callee path in a file.
//!
//! A suppression comment covers the next line only. If the rules report only
//! the first site, one comment above that site hides every later site, and a
//! gate passes that must fail (issue #4210).

use std::path::Path;

use serde_json::Value;

use crate::common::{CommandOutput, parse_json, run_fallow_in_root};

const CLOCK_RULE: &str = r#"{ "id": "no-clock", "kind": "banned-call", "callees": ["Date.now"] }"#;
const NETWORK_RULE: &str =
    r#"{ "id": "no-network", "kind": "banned-effect", "effects": ["network"] }"#;

const THREE_CLOCK_CALLS: &str =
    "export const a = Date.now();\nexport const b = Date.now();\nexport const c = Date.now();\n";
const SUPPRESSED_CLOCK_CALLS: &str = "// fallow-ignore-next-line policy-violation\nexport const a = Date.now();\nexport const b = Date.now();\nexport const c = Date.now();\n";

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// A project whose entry is `src/a.js`, with one rule pack.
fn policy_project(source: &str, rule: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module", "main": "src/a.js" }"#,
    );
    write(
        root,
        "pack.json",
        &format!(r#"{{ "version": 1, "name": "repro", "rules": [{rule}] }}"#),
    );
    write(
        root,
        ".fallowrc.json",
        r#"{ "rulePacks": ["pack.json"], "rules": { "policy-violation": "error" } }"#,
    );
    write(root, "src/a.js", source);
    dir
}

/// A project with one zone `d` that forbids the callee pattern `Date.now`.
fn boundary_project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module", "main": "src/a.js" }"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{
  "rules": { "boundary-violation": "error" },
  "boundaries": {
    "zones": [{ "name": "d", "patterns": ["src/**"] }],
    "rules": [],
    "calls": { "forbidden": [{ "from": "d", "callee": "Date.now" }] }
  }
}"#,
    );
    write(root, "src/a.js", source);
    dir
}

fn architecture(root: &Path, args: &[&str]) -> CommandOutput {
    let mut all = vec!["--quiet", "--format", "json"];
    all.extend_from_slice(args);
    run_fallow_in_root("architecture", root, &all)
}

/// The line of each entry of `key`, in report order.
fn lines(json: &Value, key: &str) -> Vec<u64> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("no `{key}`: {json}"))
        .iter()
        .map(|finding| finding["line"].as_u64().expect("line"))
        .collect()
}

/// Run the policy check and return the exit code and the finding lines.
fn policy_run(root: &Path) -> (i32, Vec<u64>) {
    let output = architecture(root, &["--no-cache", "--policy"]);
    (
        output.code,
        lines(&parse_json(&output), "policy_violations"),
    )
}

/// Run the boundary check and return the exit code and the finding lines.
fn boundary_run(root: &Path) -> (i32, Vec<u64>) {
    let output = architecture(root, &["--no-cache", "--boundaries"]);
    (
        output.code,
        lines(&parse_json(&output), "boundary_call_violations"),
    )
}

#[test]
fn banned_call_reports_each_site() {
    let dir = policy_project(THREE_CLOCK_CALLS, CLOCK_RULE);
    assert_eq!(policy_run(dir.path()), (1, vec![1, 2, 3]));
}

#[test]
fn banned_call_suppression_covers_only_the_next_site() {
    let dir = policy_project(SUPPRESSED_CLOCK_CALLS, CLOCK_RULE);
    assert_eq!(policy_run(dir.path()), (1, vec![3, 4]));
}

#[test]
fn banned_call_gives_each_site_a_distinct_finding_id() {
    let dir = policy_project(THREE_CLOCK_CALLS, CLOCK_RULE);
    let json = parse_json(&architecture(dir.path(), &["--no-cache", "--policy"]));
    let mut ids: Vec<&str> = json["policy_violations"]
        .as_array()
        .expect("policy_violations")
        .iter()
        .map(|finding| finding["finding_id"].as_str().expect("finding_id"))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 3, "ids: {ids:?}");
}

#[test]
fn banned_effect_reports_each_site() {
    let dir = policy_project(
        "export const a = fetch(\"/a\");\nexport const b = fetch(\"/b\");\n",
        NETWORK_RULE,
    );
    assert_eq!(policy_run(dir.path()), (1, vec![1, 2]));
}

#[test]
fn banned_effect_suppression_covers_only_the_next_site() {
    let dir = policy_project(
        "// fallow-ignore-next-line policy-violation\nexport const a = fetch(\"/a\");\nexport const b = fetch(\"/b\");\n",
        NETWORK_RULE,
    );
    assert_eq!(policy_run(dir.path()), (1, vec![3]));
}

#[test]
fn forbidden_call_reports_each_site() {
    let dir = boundary_project(THREE_CLOCK_CALLS);
    assert_eq!(boundary_run(dir.path()), (1, vec![1, 2, 3]));
}

#[test]
fn forbidden_call_suppression_covers_only_the_next_site() {
    let dir = boundary_project(
        "// fallow-ignore-next-line boundary-violation\nexport const a = Date.now();\nexport const b = Date.now();\nexport const c = Date.now();\n",
    );
    assert_eq!(boundary_run(dir.path()), (1, vec![3, 4]));
}

#[test]
fn warm_cache_reports_each_site() {
    let dir = policy_project(THREE_CLOCK_CALLS, CLOCK_RULE);
    let run = || {
        let output = architecture(dir.path(), &["--policy"]);
        lines(&parse_json(&output), "policy_violations")
    };
    assert_eq!(run(), vec![1, 2, 3], "cold cache");
    assert_eq!(run(), vec![1, 2, 3], "warm cache");
}
