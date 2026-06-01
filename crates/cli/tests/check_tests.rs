#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, parse_json, redact_all, run_fallow, run_fallow_combined, run_fallow_in_root,
    run_fallow_raw,
};

#[test]
fn check_with_issues_exits_1() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 1,
        "check should exit 1 when error-severity issues found"
    );
}

#[test]
fn check_warn_severity_exits_0_without_fail_flag() {
    let output = run_fallow(
        "check",
        "config-file-project",
        &["--unused-files", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "check with only warn-severity issues should exit 0 without --fail-on-issues"
    );
    let json = parse_json(&output);
    assert!(
        json["total_issues"].as_u64().unwrap_or(0) > 0,
        "config-file-project should have warn-severity unused files"
    );
}

#[test]
fn check_warn_severity_exits_1_with_fail_on_issues() {
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
fn duplicate_export_add_to_config_is_auto_fixable_with_explicit_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/one")).unwrap();
    std::fs::create_dir_all(root.join("src/two")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"explicit-config","main":"src/index.ts"}"#,
    )
    .unwrap();
    let config_path = root.join("custom.fallow.json");
    std::fs::write(&config_path, "{}\n").unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export { Button } from './one';\nexport { Button as Button2 } from './two';\nconsole.log(Button2);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/one/index.ts"), "export const Button = 1;\n").unwrap();
    std::fs::write(root.join("src/two/index.ts"), "export const Button = 2;\n").unwrap();

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--config",
            config_path.to_str().unwrap(),
            "--duplicate-exports",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "duplicate export should be reported: stdout={}, stderr={}",
        output.stdout, output.stderr
    );

    let json = parse_json(&output);
    let actions = json["duplicate_exports"][0]["actions"].as_array().unwrap();
    assert_eq!(actions[0]["type"], "add-to-config");
    assert_eq!(actions[0]["auto_fixable"], true);
}

#[test]
fn combined_performance_includes_duplication_stage() {
    let output = run_fallow_combined(
        "duplicate-code",
        &["--only", "dead-code,dupes", "--performance", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined performance run should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    assert!(
        output.stderr.contains("Pipeline Performance"),
        "combined --performance should print pipeline table: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("duplication:"),
        "pipeline table should include duplication stage: {}",
        output.stderr
    );
}

/// Combined mode runs check and dupes via `rayon::join`. Verify the parallel
/// scheduling does not leak nondeterminism into the rendered JSON: repeated
/// runs against the same fixture must produce byte-identical output once the
/// inherently nondeterministic wall-clock fields are stripped.
#[test]
fn combined_parallel_output_is_deterministic() {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("elapsed_ms");
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    normalize(v);
                }
            }
            _ => {}
        }
    }

    let mut canonicalized: Vec<String> = std::iter::repeat_with(|| {
        let output = run_fallow_combined(
            "duplicate-code",
            &["--only", "dead-code,dupes", "--format", "json", "--quiet"],
        );
        assert!(
            output.code == 0 || output.code == 1,
            "combined run should not crash: stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
        let mut value = parse_json(&output);
        normalize(&mut value);
        serde_json::to_string(&value).expect("re-serialize canonical json")
    })
    .take(3)
    .collect();

    let first = canonicalized.remove(0);
    for (idx, run) in canonicalized.iter().enumerate() {
        assert_eq!(
            &first,
            run,
            "combined parallel run #{} differed from run #0",
            idx + 1
        );
    }
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

fn combined_check_unused_export_names(json: &serde_json::Value) -> Vec<String> {
    json["check"]["unused_exports"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["export_name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn include_entry_exports_works_in_combined_mode() {
    let output = run_fallow_combined(
        "entry-export-validation",
        &["--include-entry-exports", "--format", "json", "--quiet"],
    );
    assert!(
        !output.stderr.contains("unexpected argument")
            && !output.stderr.contains("error: unrecognized argument"),
        "combined mode must accept --include-entry-exports; stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let names = combined_check_unused_export_names(&json);
    assert!(
        names.iter().any(|n| n == "meatdata"),
        "meatdata typo should be flagged in combined mode with --include-entry-exports, got: {names:?}"
    );
}

#[test]
fn include_entry_exports_via_config_file_in_combined_mode() {
    let output = run_fallow_combined(
        "entry-export-validation-config",
        &["--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let names = combined_check_unused_export_names(&json);
    assert!(
        names.iter().any(|n| n == "meatdata"),
        "meatdata should be flagged via includeEntryExports in config, got: {names:?}"
    );
}

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
