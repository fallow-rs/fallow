#[path = "common/mod.rs"]
mod common;

use common::{fixture_path, parse_json, redact_all, run_fallow, run_fallow_in_root};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// JSON output structure
// ---------------------------------------------------------------------------

/// `fallow dupes --performance` was previously a no-op: the global flag was
/// parsed but never wired through to `DupesOptions`, so users got nothing.
/// This pins the behaviour: human format renders a stderr "Duplication
/// Performance" panel; structured formats (JSON / SARIF / CodeClimate) stay
/// silent so the machine envelope is uncorrupted.
#[test]
fn dupes_performance_panel_renders_for_human_format() {
    let output = run_fallow("dupes", "duplicate-code", &["--performance"]);
    assert!(
        output.stderr.contains("Duplication Performance"),
        "human dupes --performance should print panel header. stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("clone groups:"),
        "panel should include clone group count. stderr: {}",
        output.stderr
    );
}

#[test]
fn dupes_performance_panel_suppressed_for_json_format() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--performance", "--format", "json", "--quiet"],
    );
    assert!(
        !output.stderr.contains("Duplication Performance"),
        "json dupes --performance must not corrupt machine output with the panel. stderr: {}",
        output.stderr
    );
}

#[test]
fn dupes_json_output_has_clone_groups() {
    let output = run_fallow("dupes", "duplicate-code", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("clone_groups").is_some(),
        "dupes JSON should have clone_groups key"
    );
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        !groups.is_empty(),
        "duplicate-code fixture should have clone groups"
    );
}

#[test]
fn dupes_json_has_stats() {
    let output = run_fallow("dupes", "duplicate-code", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("stats").is_some(),
        "dupes JSON should have stats key"
    );
}

// ---------------------------------------------------------------------------
// Mode flags
// ---------------------------------------------------------------------------

#[test]
fn dupes_strict_mode_accepted() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--mode", "strict", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "dupes --mode strict should not crash, got exit code {}",
        output.code
    );
}

#[test]
fn dupes_mild_mode_accepted() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--mode", "mild", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "dupes --mode mild should not crash"
    );
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

#[test]
fn dupes_min_tokens_filter() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--min-tokens", "1000", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        groups.is_empty(),
        "high min-tokens should filter out all clones"
    );
}

#[test]
fn dupes_top_flag() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        groups.len() <= 1,
        "--top 1 should return at most 1 clone group"
    );
}

#[test]
fn dupes_group_by_package_validates_non_monorepo() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"single","version":"1.0.0","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n").unwrap();

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--group-by", "package", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 2, "dupes should reject package grouping");
    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be a single JSON error object");
    assert_eq!(parsed["error"], serde_json::json!(true));
    let msg = parsed["message"]
        .as_str()
        .expect("error message should be a string");
    assert!(
        msg.contains("monorepo"),
        "error message should mention 'monorepo': {msg}"
    );
}

#[test]
fn dupes_save_baseline_creates_parent_directory() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-save","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let clone = "export function shared(value) {\n  if (value > 1) {\n    return value * 2;\n  }\n  return value + 1;\n}\n";
    std::fs::write(dir.path().join("src/one.ts"), clone).unwrap();
    std::fs::write(dir.path().join("src/two.ts"), clone).unwrap();

    let baseline_path = dir.path().join("fallow-baselines/dupes.json");
    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    let rendered = redact_all(&format!("{}\n{}", output.stdout, output.stderr), dir.path());
    assert!(
        output.code == 0 || output.code == 1,
        "dupes save baseline should not crash: {rendered}"
    );
    assert!(
        baseline_path.exists(),
        "dupes save baseline should create nested file: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Path relativization (regression: #85)
// ---------------------------------------------------------------------------

#[test]
fn dupes_json_paths_are_relative() {
    let output = run_fallow("dupes", "duplicate-code", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(!groups.is_empty(), "fixture should have clone groups");

    // All instance paths must be relative (no leading /)
    for group in groups {
        for instance in group["instances"].as_array().unwrap() {
            let path = instance["file"].as_str().unwrap();
            assert!(
                !path.starts_with('/'),
                "clone group instance path should be relative, got: {path}"
            );
        }
    }

    // Clone families should also have relative paths
    if let Some(families) = json.get("clone_families").and_then(|f| f.as_array()) {
        for family in families {
            if let Some(files) = family.get("files").and_then(|f| f.as_array()) {
                for file in files {
                    let path = file.as_str().unwrap();
                    assert!(
                        !path.starts_with('/'),
                        "clone family file path should be relative, got: {path}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Human output snapshot
// ---------------------------------------------------------------------------

#[test]
fn dupes_human_output_snapshot() {
    let output = run_fallow("dupes", "duplicate-code", &["--quiet"]);
    let root = fixture_path("duplicate-code");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("dupes_human_output", redacted);
}

// ---------------------------------------------------------------------------
// Plugin-scoped hidden directory traversal
// ---------------------------------------------------------------------------

/// Standalone `fallow dupes` must include React Router's `.client` / `.server`
/// folders in its file walk. The threshold is dropped to the minimum so the
/// small fixture files survive dupes' token / line filters and surface in
/// `stats.total_files`.
#[test]
fn dupes_includes_plugin_scoped_hidden_dirs_for_react_router() {
    let output = run_fallow(
        "dupes",
        "react-router-conventions",
        &[
            "--format",
            "json",
            "--quiet",
            "--min-tokens",
            "1",
            "--min-lines",
            "1",
        ],
    );
    assert_eq!(output.code, 0, "stderr was: {}", output.stderr);

    let json = parse_json(&output);
    let total_files = json["stats"]["total_files"]
        .as_u64()
        .expect("stats.total_files is a number");
    assert!(
        total_files >= 5,
        "expected stats.total_files >= 5 (root + routes + .client + .server), got {total_files}"
    );
}
