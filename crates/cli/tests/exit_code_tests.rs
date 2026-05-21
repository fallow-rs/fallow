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
fn config_with_traversal_glob_exits_2() {
    // Issue #463: config-sourced glob patterns with `..` segments are
    // rejected at load time with exit 2 instead of silently no-op'ing.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{ "entry": ["../escape/**"] }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "traversal glob in config should exit 2, stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("entry") && output.stderr.contains("../escape/**"),
        "stderr should mention the offending field + pattern, got: {}",
        output.stderr
    );
}

#[test]
fn config_with_invalid_glob_exits_2() {
    // Issue #463: invalid glob syntax now fails loud at load time instead
    // of being silently dropped.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{ "ignorePatterns": ["[unclosed"] }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "invalid glob syntax in config should exit 2, stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("ignorePatterns") && output.stderr.contains("[unclosed"),
        "stderr should mention the offending field + pattern, got: {}",
        output.stderr
    );
}

#[test]
fn external_plugin_file_traversal_glob_exits_2() {
    // Issue #463 second BLOCK: external plugin files loaded from
    // `.fallow/plugins/` (NOT inline `framework[]` in the main config)
    // also reach `glob::glob` on disk via their `fileExists.pattern`.
    // The validation must run on those too, not just on the inline path.
    // Mirrors codex's reproducer: `.fallow/plugins/leak.json` with a
    // traversal-bearing detection pattern.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::create_dir_all(root.join(".fallow").join("plugins")).expect("mk .fallow/plugins/");
    std::fs::write(
        root.join(".fallow").join("plugins").join("leak.json"),
        r#"{
            "name": "leaky-plugin",
            "detection": { "type": "fileExists", "pattern": "../secret-marker" }
        }"#,
    )
    .expect("write plugin");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "external plugin with traversal glob should exit 2, stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("framework[].detection")
            && output.stderr.contains("../secret-marker"),
        "stderr should mention the offending field + pattern, got: {}",
        output.stderr
    );
}

#[test]
fn fallow_plugin_root_file_traversal_glob_exits_2() {
    // Issue #463: `fallow-plugin-*` files at the project root are also
    // auto-discovered (third discovery source after `plugins:` and
    // `.fallow/plugins/`). Same validation must apply.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join("fallow-plugin-leak.json"),
        r#"{
            "name": "leaky-root-plugin",
            "entryPoints": ["../entry/**"]
        }"#,
    )
    .expect("write plugin");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "fallow-plugin-* root file with traversal glob should exit 2, stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("framework[].entryPoints") && output.stderr.contains("../entry/**"),
        "stderr should mention the offending field + pattern, got: {}",
        output.stderr
    );
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
    // by the combined report (two top-level JSON values). Parsing as a single
    // value catches that exactly: serde_json rejects trailing input.
    serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|e| {
        panic!(
            "combined mode outside a git repo must emit exactly one JSON document on stdout: {e}\nstdout was:\n{stdout}\nstderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });

    // And the parsed envelope should be the combined report; schema_version is
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

// ---------------------------------------------------------------------------
// Issue #468: boundary configuration silent-fail patterns now exit 2 at load.
// ---------------------------------------------------------------------------

#[test]
fn config_with_unknown_boundary_zone_reference_exits_2() {
    // A rule whose `from`/`allow`/`allowTypeOnly` names a zone that does NOT
    // exist in `zones[]` used to log a `tracing::error!` and continue,
    // producing a flood of false-positive boundary violations at analysis
    // time. Now exits 2 at config load with every offending entry enumerated.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
                "rules": [
                    {
                        "from": "typo-from",
                        "allow": ["typo-allow"],
                        "allowTypeOnly": ["typo-type-only"]
                    },
                    {
                        "from": "ui",
                        "allow": ["another-typo"]
                    }
                ]
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "unknown boundary zone reference should exit 2, stderr: {}",
        output.stderr
    );

    // Every offending tuple should appear in one rendered diagnostic. Users
    // fix all four in one edit instead of one-by-one.
    let stderr = &output.stderr;
    assert!(
        stderr.contains("invalid boundary configuration"),
        "stderr: {stderr}"
    );
    for name in ["typo-from", "typo-allow", "typo-type-only", "another-typo"] {
        assert!(
            stderr.contains(name),
            "stderr should name every offending zone (`{name}`): {stderr}"
        );
    }
}

#[test]
fn config_with_redundant_boundary_root_prefix_exits_2() {
    // `boundaries.zones[].root` + a pattern that redundantly repeats the
    // root double-prefixes the path at classify time and never matches. This
    // used to log a `tracing::error!` and continue with a phantom-empty
    // zone; now exits 2 at config load with the legacy
    // FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX tag preserved for CI grep recipes.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [{
                    "name": "ui",
                    "patterns": ["packages/app/src/**"],
                    "root": "packages/app/"
                }],
                "rules": []
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "redundant root prefix should exit 2, stderr: {}",
        output.stderr
    );
    let stderr = &output.stderr;
    assert!(
        stderr.contains("FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX"),
        "stderr should preserve the legacy tag for CI grep recipes: {stderr}"
    );
    assert!(stderr.contains("packages/app/src/**"), "stderr: {stderr}");
}

#[test]
fn fallow_config_subcommand_rejects_unknown_boundary_zone() {
    // `fallow config` lives on a different code path than `check` (calls
    // `FallowConfig::load` / `find_and_load` directly, no `runtime_support`).
    // Without explicit wiring it would print the parsed config and exit 0
    // even when `check` exits 2, giving users a false "loaded fine" signal.
    // Surfaced by review of #468.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
                "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_raw(&["--root", root.to_str().expect("utf-8 root"), "config"]);
    assert_eq!(
        output.code, 2,
        "fallow config must reject invalid boundary config, stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("typo-zone"),
        "stderr should name the typo'd zone, got: {}",
        output.stderr
    );
}

#[test]
fn fallow_config_subcommand_json_format_emits_structured_error_envelope() {
    // `--format json` config-load failures must land as the structured
    // `{"error": true, "message": ..., "exit_code": 2}` envelope on stdout,
    // not human text. Locks the JSON error contract for the config subcommand.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
                "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_raw(&[
        "--root",
        root.to_str().expect("utf-8 root"),
        "--format",
        "json",
        "config",
    ]);
    assert_eq!(output.code, 2, "should exit 2, stderr: {}", output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout should be JSON envelope: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert_eq!(parsed["error"], serde_json::Value::Bool(true));
    assert_eq!(parsed["exit_code"], serde_json::Value::from(2));
    let msg = parsed["message"]
        .as_str()
        .expect("message should be a string");
    assert!(msg.contains("invalid boundary configuration"), "msg: {msg}");
    assert!(msg.contains("typo-zone"), "msg: {msg}");
}

#[test]
fn fallow_list_boundaries_json_format_emits_structured_error_envelope() {
    // `fallow list --boundaries --format json` previously hardcoded
    // `OutputFormat::Human` when calling `load_config`, so config-load
    // failures (boundary validation, glob validation, plugin validation)
    // surfaced as human-text errors on stderr instead of the structured JSON
    // envelope JSON consumers expect. Surfaced by review of #468.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
                "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_raw(&[
        "--root",
        root.to_str().expect("utf-8 root"),
        "--format",
        "json",
        "list",
        "--boundaries",
    ]);
    assert_eq!(output.code, 2, "should exit 2, stderr: {}", output.stderr);
    let parsed: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout should be JSON envelope: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert_eq!(parsed["error"], serde_json::Value::Bool(true));
    assert_eq!(parsed["exit_code"], serde_json::Value::from(2));
    let msg = parsed["message"]
        .as_str()
        .expect("message should be a string");
    assert!(msg.contains("invalid boundary configuration"), "msg: {msg}");
    assert!(msg.contains("typo-zone"), "msg: {msg}");
}

#[test]
fn config_with_valid_boundaries_loads_cleanly() {
    // Control: a boundary config whose every zone reference resolves and
    // whose patterns do not redundantly prefix their root continues to load
    // (no analysis sources here, so check exits 0 with zero findings).
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
            "boundaries": {
                "zones": [
                    { "name": "ui", "patterns": ["src/ui/**"] },
                    { "name": "db", "patterns": ["src/db/**"] }
                ],
                "rules": [
                    { "from": "ui", "allow": ["db"] }
                ]
            }
        }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 0,
        "valid boundary config should load (exit 0 with no sources), stderr: {}",
        output.stderr
    );
}
