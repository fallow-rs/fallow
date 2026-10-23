#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! `health.cyclomaticPolicy` decides how the cyclomatic gate and CRAP count
//! literal membership chains (issue #3781).

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};
use serde_json::Value;

/// Standard cyclomatic 8 (one `if` plus six `||`). Under
/// `literal-membership` the chain is one predicate, so the value is 2.
const MEMBERSHIP: &str = r#"export function byComparisons(kind: string): void {
  if (
    kind === "a" ||
    kind === "b" ||
    kind === "c" ||
    kind === "d" ||
    kind === "e" ||
    kind === "f" ||
    kind === "g"
  ) {
    throw new Error("Unsupported kind");
  }
}
"#;

/// The same chain plus unrelated decisions: standard 13, policy value 7.
const MEMBERSHIP_AND_BRANCHES: &str = r#"export function withBranches(kind: string, n: number): number {
  if (kind === "a" || kind === "b" || kind === "c" || kind === "d" || kind === "e" || kind === "f" || kind === "g") {
    return 0;
  }
  if (n > 1) { return 1; }
  if (n > 2) { return 2; }
  if (n > 3) { return 3; }
  if (n > 4) { return 4; }
  if (n > 5) { return 5; }
  return 6;
}
"#;

/// Different operands: not a membership chain under any policy (standard 7).
const MIXED: &str = r#"export function mixed(kind: string, other: string): number {
  if (kind === "a" || kind === "b" || kind === "c" || kind === "d" || kind === "e" || other === "f") {
    return 1;
  }
  return 0;
}
"#;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn project(policy: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"cyclomatic-policy","version":"1.0.0","private":true}"#,
    );
    let policy = policy.map_or_else(String::new, |value| {
        format!(r#", "cyclomaticPolicy": "{value}""#)
    });
    write(
        root,
        ".fallowrc.json",
        &format!(
            r#"{{
  "entry": ["src/**/*.ts"],
  "health": {{ "maxCyclomatic": 6, "maxCognitive": 50, "maxCrap": 40{policy} }}
}}
"#
        ),
    );
    write(root, "src/membership.ts", MEMBERSHIP);
    write(root, "src/branches.ts", MEMBERSHIP_AND_BRANCHES);
    write(root, "src/mixed.ts", MIXED);
    dir
}

fn health(root: &Path) -> Value {
    let output = run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--complexity-breakdown",
            "--no-cache",
            "--format",
            "json",
            "--quiet",
        ],
    );
    parse_json(&output)
}

fn finding<'a>(report: &'a Value, name: &str) -> Option<&'a Value> {
    report["findings"]
        .as_array()?
        .iter()
        .find(|finding| finding["name"] == name)
}

#[test]
fn standard_policy_keeps_the_mccabe_count_and_output_shape() {
    let dir = project(None);
    let report = health(dir.path());

    assert!(report["summary"].get("cyclomatic_policy").is_none());
    let membership = finding(&report, "byComparisons").expect("standard gate reports the chain");
    assert_eq!(membership["cyclomatic"], 8);
    assert!(membership.get("standard_cyclomatic").is_none());
    let flagged = membership["contributions"]
        .as_array()
        .expect("breakdown")
        .iter()
        .any(|contribution| contribution.get("literal_membership").is_some());
    assert!(!flagged, "the default breakdown must not change");
}

#[test]
fn literal_membership_policy_counts_a_chain_as_one_predicate() {
    let dir = project(Some("literal-membership"));
    let report = health(dir.path());

    assert_eq!(report["summary"]["cyclomatic_policy"], "literal-membership");
    assert!(
        finding(&report, "byComparisons").is_none(),
        "the chain alone must pass the cyclomatic and CRAP gates: {report:#}"
    );

    let branches = finding(&report, "withBranches").expect("unrelated branches still fail");
    assert_eq!(branches["cyclomatic"], 7);
    assert_eq!(branches["standard_cyclomatic"], 13);
    let contributions = branches["contributions"].as_array().expect("breakdown");
    let flagged = contributions
        .iter()
        .filter(|contribution| contribution["literal_membership"] == true)
        .count();
    assert_eq!(flagged, 6);
    let cyclomatic_sum: u64 = contributions
        .iter()
        .filter(|contribution| {
            contribution["metric"] == "cyclomatic" && contribution["literal_membership"] != true
        })
        .map(|contribution| contribution["weight"].as_u64().expect("weight"))
        .sum();
    assert_eq!(
        cyclomatic_sum + 1,
        7,
        "the breakdown rebuilds the gated value"
    );

    let mixed = finding(&report, "mixed").expect("different operands keep the standard count");
    assert_eq!(mixed["cyclomatic"], 7);
    assert!(mixed.get("standard_cyclomatic").is_none());
}

#[test]
fn literal_membership_policy_lowers_crap_with_the_cyclomatic_value() {
    let dir = project(Some("literal-membership"));
    let report = parse_json(&run_fallow_in_root(
        "health",
        dir.path(),
        &["--file-scores", "--no-cache", "--format", "json", "--quiet"],
    ));
    let file_score = |suffix: &str| -> f64 {
        report["file_scores"]
            .as_array()
            .expect("file scores")
            .iter()
            .find(|score| {
                score["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(suffix))
            })
            .and_then(|score| score["crap_max"].as_f64())
            .expect("crap_max")
    };
    // Untested: CRAP = cc^2 + cc. cc 2 under the policy, 8 under standard.
    assert!((file_score("membership.ts") - 6.0).abs() < f64::EPSILON);
}

#[test]
fn unknown_policy_value_is_a_config_error() {
    let dir = project(Some("loose"));
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &["--complexity", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("cyclomaticPolicy") || output.stdout.contains("unknown variant")
    );
}
