#[path = "common/mod.rs"]
mod common;

use common::{fixture_path, parse_json, redact_all, run_fallow, run_fallow_raw};

// ---------------------------------------------------------------------------
// Exit code semantics
// ---------------------------------------------------------------------------

// check always exits 1 when error-severity issues exist (default severity = error).
// --fail-on-issues additionally promotes warn-severity rules to error.

#[test]
fn check_with_issues_exits_1() {
    // basic-project has issues with default error severity → exit 1
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 1,
        "check should exit 1 when error-severity issues found"
    );
}

#[test]
fn check_warn_severity_exits_0_without_fail_flag() {
    // config-file-project has rules.unused-files = "warn" in .fallowrc.json
    // With only warn-severity rules, should exit 0
    let output = run_fallow(
        "check",
        "config-file-project",
        &["--unused-files", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "check with only warn-severity issues should exit 0 without --fail-on-issues"
    );
    // Verify issues were actually found (not just "no issues at all")
    let json = parse_json(&output);
    assert!(
        json["total_issues"].as_u64().unwrap_or(0) > 0,
        "config-file-project should have warn-severity unused files"
    );
}

#[test]
fn check_warn_severity_exits_1_with_fail_on_issues() {
    // --fail-on-issues promotes warns to errors
    let output = run_fallow(
        "check",
        "config-file-project",
        &[
            "--unused-files",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "--fail-on-issues should promote warns to errors and exit 1"
    );
}

#[test]
fn check_ci_flag_implies_fail_on_issues() {
    let output = run_fallow("check", "basic-project", &["--ci", "--format", "json"]);
    assert_eq!(output.code, 1, "--ci should imply --fail-on-issues");
}

// ---------------------------------------------------------------------------
// Format switching
// ---------------------------------------------------------------------------

#[test]
fn check_json_format_produces_valid_json() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("schema_version").is_some(),
        "JSON output should have schema_version"
    );
    assert!(json.is_object(), "JSON output should be an object");
}

#[test]
fn check_compact_format_has_no_ansi() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "compact", "--quiet"],
    );
    assert!(
        !output.stdout.contains("\x1b["),
        "compact output should have no ANSI escape sequences"
    );
    assert!(
        !output.stdout.trim().is_empty(),
        "compact output should not be empty for project with issues"
    );
}

#[test]
fn check_sarif_format_has_schema() {
    let output = run_fallow("check", "basic-project", &["--format", "sarif", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some(),
        "SARIF output should have $schema key"
    );
}

#[test]
fn check_markdown_format_has_heading() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "markdown", "--quiet"],
    );
    assert!(
        output.stdout.contains('#'),
        "markdown output should contain heading markers"
    );
}

#[test]
fn check_codeclimate_format_is_array() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "codeclimate", "--quiet"],
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse codeclimate JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(json.is_array(), "codeclimate output should be a JSON array");
}

#[test]
fn check_gitlab_codequality_alias_is_array() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "gitlab-codequality", "--quiet"],
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse gitlab-codequality JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(
        json.is_array(),
        "gitlab-codequality output should be a JSON array"
    );
}

// ---------------------------------------------------------------------------
// Issue type filtering
// ---------------------------------------------------------------------------

#[test]
fn check_unused_files_filter_limits_output() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--unused-files", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_files").is_some(),
        "should have unused_files when filtered"
    );
    let unused_exports = json["unused_exports"].as_array();
    assert!(
        unused_exports.is_none() || unused_exports.unwrap().is_empty(),
        "unused_exports should be empty when only --unused-files"
    );
}

#[test]
fn check_multiple_filters_combined() {
    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--unused-files",
            "--unused-exports",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_files").is_some(),
        "should have unused_files"
    );
    assert!(
        json.get("unused_exports").is_some(),
        "should have unused_exports"
    );
}

#[test]
fn check_unused_deps_filter() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--unused-deps", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_dependencies").is_some(),
        "should have unused_dependencies"
    );
}

// ---------------------------------------------------------------------------
// JSON structure validation
// ---------------------------------------------------------------------------

#[test]
fn check_json_has_total_issues() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("total_issues").is_some(),
        "JSON should have total_issues"
    );
    assert!(
        json["total_issues"].as_u64().unwrap() > 0,
        "basic-project should have issues"
    );
}

#[test]
fn check_json_has_version_and_elapsed() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(json.get("version").is_some(), "JSON should have version");
    assert!(
        json.get("elapsed_ms").is_some(),
        "JSON should have elapsed_ms"
    );
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
fn check_invalid_root_exits_2() {
    let output = run_fallow_raw(&["check", "--root", "/nonexistent/path/xyz", "--quiet"]);
    assert_eq!(output.code, 2, "invalid root should exit with code 2");
}

#[test]
fn check_json_error_format() {
    let output = run_fallow_raw(&[
        "check",
        "--root",
        "/nonexistent/path/xyz",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 2);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "error output should be valid JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(
        json.get("error").is_some(),
        "error JSON should have 'error' field"
    );
}

// ---------------------------------------------------------------------------
// Human output snapshots (Phase 6)
// ---------------------------------------------------------------------------

// NOTE: full human output snapshot (all issue types combined) is not snapshotted
// because FxHashMap iteration order makes dependency listing non-deterministic.
// Individual issue-type snapshots below are stable.

#[test]
fn check_human_output_unused_files_only() {
    let output = run_fallow("check", "basic-project", &["--unused-files", "--quiet"]);
    let root = fixture_path("basic-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("check_human_unused_files_only", redacted);
}

#[test]
fn check_human_output_unused_exports_only() {
    let output = run_fallow("check", "basic-project", &["--unused-exports", "--quiet"]);
    let root = fixture_path("basic-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("check_human_unused_exports_only", redacted);
}

// NOTE: unused-deps human snapshot skipped — dependency iteration order is non-deterministic
#[test]
fn check_human_output_unused_deps_has_content() {
    let output = run_fallow("check", "basic-project", &["--unused-deps", "--quiet"]);
    assert!(
        output.stdout.contains("Unused dependencies"),
        "unused-deps output should contain section header"
    );
    assert!(
        output.stdout.contains("unused-dep"),
        "should list unused-dep"
    );
}
