//! Dead-code CodeClimate fingerprints and review markers come from the
//! `finding_id`, so a line shift above a finding keeps GitLab's issue and the
//! PR or MR review thread.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::common::{copy_fixture, parse_json, run_fallow_raw};

const BASIC: &str = "finding-ids-basic";
const WORKSPACES: &str = "finding-ids-workspaces";
const SHIFTED_FILES: &[&str] = &[
    "src/utils.ts",
    "src/lib.ts",
    "src/flags.ts",
    "src/orphan.ts",
];

fn render(root: &Path, format: &str) -> Value {
    let root = root.to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        format,
        "--quiet",
        "--no-cache",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code --format {format} failed with {}: {}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

fn shift_lines(root: &Path) {
    for file in SHIFTED_FILES {
        let path = root.join(file);
        let source = std::fs::read_to_string(&path).expect("read fixture file");
        std::fs::write(&path, format!("\n\n\n// shifted\n\n{source}")).expect("write fixture");
    }
}

fn fingerprints(issues: &Value) -> BTreeSet<String> {
    issues
        .as_array()
        .expect("CodeClimate output is an array")
        .iter()
        .map(|issue| {
            issue["fingerprint"]
                .as_str()
                .expect("fingerprint")
                .to_owned()
        })
        .collect()
}

fn begin_lines(issues: &Value) -> Vec<u64> {
    issues
        .as_array()
        .expect("CodeClimate output is an array")
        .iter()
        .map(|issue| issue["location"]["lines"]["begin"].as_u64().unwrap_or(0))
        .collect()
}

/// The comment fingerprints of a review envelope, each with the marker value
/// its body ends with.
fn review_markers(envelope: &Value) -> BTreeSet<(String, String)> {
    envelope["comments"]
        .as_array()
        .expect("comments array")
        .iter()
        .map(|comment| {
            let body = comment["body"].as_str().expect("body");
            let marker = body
                .split("<!-- fallow-fingerprint:v3: ")
                .nth(1)
                .and_then(|rest| rest.split(" -->").next())
                .unwrap_or_else(|| panic!("comment has no v3 marker: {body}"));
            (
                comment["fingerprint"]
                    .as_str()
                    .expect("fingerprint")
                    .to_owned(),
                marker.to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_line_shift_keeps_every_dead_code_codeclimate_fingerprint() {
    let dir = copy_fixture(BASIC);
    let before = render(dir.path(), "codeclimate");
    shift_lines(dir.path());
    let after = render(dir.path(), "codeclimate");

    assert!(
        before.as_array().is_some_and(|issues| issues.len() >= 5),
        "fixture reports too few issues: {before}"
    );
    assert_ne!(
        begin_lines(&before),
        begin_lines(&after),
        "the guard is only meaningful while the lines actually moved"
    );
    assert_eq!(fingerprints(&before), fingerprints(&after));
    assert_eq!(
        fingerprints(&before).len(),
        before.as_array().map_or(0, Vec::len),
        "every issue has its own fingerprint"
    );
}

#[test]
fn a_line_shift_keeps_every_review_marker() {
    let dir = copy_fixture(BASIC);
    let before = render(dir.path(), "review-github");
    shift_lines(dir.path());
    let after = render(dir.path(), "review-github");

    let markers = review_markers(&before);
    assert!(!markers.is_empty(), "fixture renders no comments: {before}");
    assert!(
        markers
            .iter()
            .all(|(fingerprint, marker)| fingerprint == marker),
        "the marker holds the comment fingerprint: {markers:?}"
    );
    assert_eq!(markers, review_markers(&after));
    let has_legacy = before["comments"]
        .as_array()
        .expect("comments array")
        .iter()
        .any(|comment| comment.get("legacy_fingerprint").is_some());
    assert!(
        has_legacy,
        "a finding whose old fingerprint held the line carries it for v2 matching: {before}"
    );
}

#[test]
fn the_same_dependency_in_two_workspaces_gets_two_fingerprints() {
    let dir = copy_fixture(WORKSPACES);
    let issues = render(dir.path(), "codeclimate");

    let dependency_fingerprints: Vec<&str> = issues
        .as_array()
        .expect("CodeClimate output is an array")
        .iter()
        .filter(|issue| issue["check_name"] == "fallow/unused-dependency")
        .map(|issue| issue["fingerprint"].as_str().expect("fingerprint"))
        .collect();
    assert_eq!(dependency_fingerprints.len(), 2, "{issues}");
    assert_ne!(dependency_fingerprints[0], dependency_fingerprints[1]);
}
