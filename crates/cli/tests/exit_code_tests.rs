#[path = "common/mod.rs"]
mod common;

use common::{
    fallow_bin, parse_json, run_fallow, run_fallow_combined, run_fallow_in_root, run_fallow_raw,
};

// ---------------------------------------------------------------------------
// --fail-on-issues across commands
// ---------------------------------------------------------------------------

#[test]
fn fail_on_issues_check_exits_1_with_issues() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--fail-on-issues", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 1,
        "check --fail-on-issues should exit 1 with issues"
    );
}

#[test]
fn fail_on_issues_dupes_exits_1_with_clones() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &[
            "--threshold",
            "0.1",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "dupes with --fail-on-issues should not crash, got {}",
        output.code
    );
}

#[test]
fn combined_mode_runs_successfully() {
    let output = run_fallow_combined("basic-project", &["--format", "json", "--quiet"]);
    assert!(
        output.code == 0 || output.code == 1,
        "combined mode should not crash, got exit code {}",
        output.code
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout)
        .unwrap_or_else(|e| panic!("combined output should be JSON: {e}"));
    assert!(json.is_object(), "combined output should be a JSON object");
}

#[test]
fn combined_mode_config_enabled_coverage_gaps_stays_out_of_health_section() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    std::fs::write(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "warn"
  }
}
"#,
    )
    .expect("write config file");

    let output = run_fallow_raw(&[
        "--root",
        common::fixture_path("production-mode")
            .to_str()
            .expect("fixture path should be utf-8"),
        "--config",
        config_path.to_str().expect("config path should be utf-8"),
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "combined mode should not crash with config-enabled coverage gaps"
    );

    let json = parse_json(&output);
    assert!(
        json["health"].get("coverage_gaps").is_none(),
        "combined mode should not leak coverage_gaps into the embedded health report"
    );
}

#[test]
fn combined_mode_hidden_coverage_gap_gate_does_not_fail() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    std::fs::write(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "error",
    "unused-files": "off",
    "unused-dependencies": "off",
    "unused-exports": "off",
    "test-only-dependencies": "off"
  }
}
"#,
    )
    .expect("write config file");

    let output = run_fallow_raw(&[
        "--root",
        common::fixture_path("coverage-gaps")
            .to_str()
            .expect("fixture path should be utf-8"),
        "--config",
        config_path.to_str().expect("config path should be utf-8"),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "combined mode should not fail on hidden coverage-gap gates"
    );

    let json = parse_json(&output);
    assert!(
        json["health"].get("coverage_gaps").is_none(),
        "combined mode should keep hidden coverage gaps out of the embedded health report"
    );
}

#[test]
fn combined_human_output_labels_metrics_line() {
    let output = run_fallow_combined("basic-project", &[]);
    assert!(
        output.code == 0 || output.code == 1,
        "combined human output should not crash, got exit code {}",
        output.code
    );
    let metrics_line = output
        .stderr
        .lines()
        .find(|line| line.contains("dead files"))
        .expect("combined human output should include the orientation metrics line");
    assert!(
        metrics_line.trim_start().starts_with("■ Metrics:"),
        "combined human output should label the orientation metrics line. line: {metrics_line}\nstderr: {}",
        output.stderr,
    );
}

// ---------------------------------------------------------------------------
// --only / --skip in combined mode
// ---------------------------------------------------------------------------

#[test]
fn combined_only_dead_code() {
    let output = run_fallow_combined(
        "basic-project",
        &["--only", "dead-code", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined --only dead-code should not crash"
    );
}

#[test]
fn combined_skip_dead_code() {
    let output = run_fallow_combined(
        "basic-project",
        &["--skip", "dead-code", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined --skip dead-code should not crash"
    );
}

#[test]
fn combined_only_and_skip_are_mutually_exclusive() {
    let output = run_fallow_combined(
        "basic-project",
        &[
            "--only",
            "dead-code",
            "--skip",
            "health",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 2,
        "--only and --skip together should exit 2 (invalid args)"
    );
}

// ---------------------------------------------------------------------------
// Baseline round-trip
// ---------------------------------------------------------------------------

#[test]
fn save_baseline_creates_file() {
    let dir = std::env::temp_dir().join(format!("fallow-baseline-test-{}", std::process::id()));
    // Pre-clean to avoid false positives from previous runs
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let baseline_path = dir.join("fallow-baselines/dead-code.json");

    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "save-baseline should not crash"
    );
    assert!(
        baseline_path.exists(),
        "--save-baseline should create the baseline file"
    );

    let content = std::fs::read_to_string(&baseline_path).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&content).expect("baseline file should be valid JSON");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn baseline_filters_known_issues() {
    let dir = std::env::temp_dir().join(format!(
        "fallow-baseline-filter-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let baseline_path = dir.join("baseline.json");

    run_fallow(
        "check",
        "basic-project",
        &[
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );

    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let total = json["total_issues"].as_u64().unwrap_or(0);
    assert_eq!(
        total, 0,
        "baseline should filter all known issues, got {total}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_baseline_distinguishes_same_unused_dep_across_workspaces() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "baseline-workspace-deps",
  "private": true,
  "workspaces": ["packages/*"]
}
"#,
    )
    .expect("write root package.json");
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "strict": true
  }
}
"#,
    )
    .expect("write tsconfig");

    for package in ["app-a", "app-b"] {
        let package_dir = dir.path().join("packages").join(package);
        let src_dir = package_dir.join("src");
        std::fs::create_dir_all(&src_dir).expect("create package src");
        std::fs::write(
            package_dir.join("package.json"),
            format!(
                r#"{{
  "name": "{package}",
  "version": "1.0.0",
  "main": "src/index.ts",
  "dependencies": {{ "lodash-es": "4.17.21" }}
}}
"#
            ),
        )
        .expect("write workspace package.json");
        std::fs::write(
            src_dir.join("index.ts"),
            format!("export const {package}_value = 1;\n").replace('-', "_"),
        )
        .expect("write source file");
    }

    let baseline_path = dir.path().join("baseline.json");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--save-baseline",
            baseline_path
                .to_str()
                .expect("baseline path should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "save-baseline should not crash, got {}: {}",
        output.code,
        output.stderr
    );

    let baseline: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&baseline_path).expect("read baseline"))
            .expect("baseline should be valid JSON");
    let deps: Vec<&str> = baseline["unused_dependencies"]
        .as_array()
        .expect("unused_dependencies should be an array")
        .iter()
        .map(|value| value.as_str().expect("dependency key should be a string"))
        .collect();

    assert_eq!(
        deps,
        vec![
            "packages/app-a/package.json:lodash-es",
            "packages/app-b/package.json:lodash-es"
        ]
    );
}

// ---------------------------------------------------------------------------
// --changed-since
// ---------------------------------------------------------------------------

#[test]
fn changed_since_accepts_head() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--changed-since", "HEAD", "--format", "json", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "check --changed-since HEAD should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json.get("total_issues").is_some(),
        "should still have total_issues key even with --changed-since"
    );
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[test]
fn nonexistent_root_exits_2() {
    let output = run_fallow_raw(&[
        "check",
        "--root",
        "/nonexistent/path/for/testing",
        "--quiet",
    ]);
    assert_eq!(output.code, 2, "nonexistent root should exit 2");
}

#[test]
fn no_package_json_returns_empty_results() {
    let output = run_fallow(
        "check",
        "error-no-package-json",
        &["--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "missing package.json should exit 0 with no issues, stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["total_issues"].as_u64().unwrap_or(0),
        0,
        "should have 0 issues without package.json"
    );
}

// ---------------------------------------------------------------------------
// Combined-mode JSON contract: stdout is exactly one JSON document even when
// the project is outside a Git repository (regression for #294).
// ---------------------------------------------------------------------------

#[test]
fn combined_json_outside_git_repo_emits_single_document() {
    use std::process::Command;

    // Build a minimal TS project in a tempdir whose parent chain has no `.git`,
    // so the hotspot pipeline's `is_git_repo` check returns false. We isolate
    // from any inherited `GIT_DIR` / `GIT_WORK_TREE` set by parent test hooks
    // and from any global git config that could redirect rev-parse upward.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"no-git-combined","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2020","module":"ES2020","strict":true},"include":["src"]}"#,
    )
    .expect("write tsconfig.json");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("src/index.ts"),
        "export function add(a: number, b: number): number { return a + b; }\n",
    )
    .expect("write index.ts");

    let mut cmd = Command::new(fallow_bin());
    cmd.arg("--root")
        .arg(root)
        .arg("--format")
        .arg("json")
        .arg("--quiet")
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    let output = cmd.output().expect("failed to run fallow binary");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The bug in #294 was that stdout contained an inline `{"error": true,
    // "message": "hotspot analysis requires a git repository", ...}` followed
    // by the combined report — two top-level JSON values. Parsing as a single
    // value catches that exactly: serde_json rejects trailing input.
    serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|e| {
        panic!(
            "combined mode outside a git repo must emit exactly one JSON document on stdout: {e}\nstdout was:\n{stdout}\nstderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });

    // And the parsed envelope should be the combined report — schema_version is
    // the canonical marker.
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("already parsed");
    assert!(
        json.get("schema_version").is_some(),
        "stdout should be the combined report envelope, got: {json}"
    );
    assert!(
        json.get("error").is_none(),
        "combined report must not surface a top-level `error` key from a nested hotspot bail-out"
    );
}
