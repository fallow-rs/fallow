#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep JSON assertions concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{CommandOutput, run_fallow};

fn assert_json_style(compact: &CommandOutput, pretty: &CommandOutput, command: &str) {
    assert_eq!(
        compact.code, pretty.code,
        "presentation must not change exit code"
    );
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "{command} JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent {command} JSON"
    );
    serde_json::from_str::<serde_json::Value>(&compact.stdout)
        .expect("compact output should be valid JSON");
    serde_json::from_str::<serde_json::Value>(&pretty.stdout)
        .expect("pretty output should be valid JSON");
}

#[test]
fn fix_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let pretty = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--pretty", "--quiet"],
    );

    assert_json_style(&compact, &pretty, "fix");

    let compact: serde_json::Value =
        serde_json::from_str(&compact.stdout).expect("compact fix JSON should parse");
    let pretty: serde_json::Value =
        serde_json::from_str(&pretty.stdout).expect("pretty fix JSON should parse");
    assert_eq!(compact, pretty, "presentation must not change fix data");
}

#[test]
fn list_json_uses_the_selected_presentation_style() {
    let compact = run_fallow("list", "basic-project", &["--format", "json", "--quiet"]);
    let pretty = run_fallow(
        "list",
        "basic-project",
        &["--format", "json", "--pretty", "--quiet"],
    );

    assert_json_style(&compact, &pretty, "list");

    let compact: serde_json::Value =
        serde_json::from_str(&compact.stdout).expect("compact list JSON should parse");
    let pretty: serde_json::Value =
        serde_json::from_str(&pretty.stdout).expect("pretty list JSON should parse");
    assert_eq!(compact, pretty, "presentation must not change list data");
}

#[test]
fn flags_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let pretty = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--format", "json", "--pretty", "--quiet", "--no-cache"],
    );

    assert_json_style(&compact, &pretty, "flags");
}

#[test]
fn suppressions_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "suppressions",
        "suppression-reasons",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let pretty = run_fallow(
        "suppressions",
        "suppression-reasons",
        &["--format", "json", "--pretty", "--quiet", "--no-cache"],
    );

    assert_json_style(&compact, &pretty, "suppressions");
}
