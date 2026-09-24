#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fallow_bin, parse_json, run_fallow, run_fallow_combined, run_fallow_in_root, run_fallow_raw,
};

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

/// Named for the flag that actually drives the exit code. This case previously
/// also passed `--fail-on-issues` and was named for it, but `dupes` never reads
/// that flag (`dispatch_dupes` discards it and `DupesOptions` has no such
/// field), so the assertion was carried entirely by `--threshold`. Wiring
/// `--fail-on-issues` into `dupes` is a separate behaviour change; until then
/// this test pins the threshold gate only, under a name that matches.
#[test]
fn dupes_threshold_exits_1_with_clones() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--threshold", "0.1", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 1,
        "dupes over its threshold must exit 1. stderr: {}",
        output.stderr
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
fn combined_json_explain_includes_sectioned_meta() {
    let output = run_fallow_combined(
        "basic-project",
        &["--format", "json", "--quiet", "--explain"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined mode should not crash, got exit code {}",
        output.code
    );
    let json = parse_json(&output);
    assert!(
        json.pointer("/_meta/check/rules/unused-export/description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|text| text.contains("Named exports")),
        "combined _meta should include dead-code rule descriptions"
    );
    assert!(
        json.pointer("/_meta/dupes/metrics/duplication_percentage/description")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "combined _meta should include duplication metric descriptions"
    );
    assert!(
        json.pointer("/_meta/health/metrics/cyclomatic/description")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "combined _meta should include health metric descriptions"
    );
}

#[test]
fn human_explain_adds_inline_descriptions_for_analysis_commands() {
    let check = run_fallow("check", "basic-project", &["--quiet", "--explain"]);
    assert!(
        check
            .stdout
            .contains("Description: Named exports that are never imported"),
        "check --explain should describe dead-code sections, stdout:\n{}",
        check.stdout
    );

    let dupes = run_fallow("dupes", "duplicate-code", &["--quiet", "--explain"]);
    assert!(
        dupes.stdout.contains("Description: A block of code"),
        "dupes --explain should describe duplicate sections, stdout:\n{}",
        dupes.stdout
    );

    let health = run_fallow("health", "complexity-project", &["--quiet", "--explain"]);
    assert!(
        health
            .stdout
            .contains("Description: Function exceeds both cyclomatic and cognitive"),
        "health --explain should describe health sections, stdout:\n{}",
        health.stdout
    );
}

#[test]
fn health_json_reports_framework_abstains() {
    let output = run_fallow(
        "health",
        "sveltekit-load-data-global-abstain",
        &["--format", "json", "--quiet", "--score"],
    );
    assert_eq!(output.code, 0, "health should pass: {}", output.stderr);
    let json = parse_json(&output);
    assert!(
        json.pointer("/framework_health/detected_frameworks")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|frameworks| frameworks.iter().any(|framework| framework == "sveltekit")),
        "SvelteKit should be detected: {}",
        output.stdout
    );
    let detectors = json
        .pointer("/framework_health/detectors")
        .and_then(serde_json::Value::as_array)
        .expect("framework detectors should be present");
    let abstain = detectors
        .iter()
        .find(|detector| {
            detector.get("id").and_then(serde_json::Value::as_str) == Some("unused-load-data-key")
                && detector
                    .get("framework")
                    .and_then(serde_json::Value::as_str)
                    == Some("sveltekit")
        })
        .expect("unused-load-data-key detector should be reported");
    assert_eq!(
        abstain.get("status").and_then(serde_json::Value::as_str),
        Some("abstained")
    );
    assert_eq!(
        abstain.get("reason").and_then(serde_json::Value::as_str),
        Some("unused_load_data_keys_global_abstain")
    );
}

#[test]
fn health_json_reports_disabled_framework_detectors() {
    let output = run_fallow(
        "health",
        "unused-react-prop",
        &["--format", "json", "--quiet", "--score"],
    );
    assert_eq!(output.code, 0, "health should pass: {}", output.stderr);
    let json = parse_json(&output);
    let detectors = json
        .pointer("/framework_health/detectors")
        .and_then(serde_json::Value::as_array)
        .expect("framework detectors should be present");
    for id in ["prop-drilling", "thin-wrapper", "duplicate-prop-shape"] {
        let detector = detectors
            .iter()
            .find(|detector| {
                detector.get("id").and_then(serde_json::Value::as_str) == Some(id)
                    && detector
                        .get("framework")
                        .and_then(serde_json::Value::as_str)
                        == Some("react")
            })
            .unwrap_or_else(|| panic!("{id} should be reported"));
        assert_eq!(
            detector.get("status").and_then(serde_json::Value::as_str),
            Some("disabled_by_config")
        );
        assert_eq!(
            detector.get("reason").and_then(serde_json::Value::as_str),
            Some("disabled_by_config")
        );
    }
}

#[test]
fn health_json_reports_not_checked_framework_detectors() {
    let output = run_fallow(
        "health",
        "nuxt-auto-import-components",
        &["--format", "json", "--quiet", "--score"],
    );
    assert_eq!(output.code, 0, "health should pass: {}", output.stderr);
    let json = parse_json(&output);
    let detectors = json
        .pointer("/framework_health/detectors")
        .and_then(serde_json::Value::as_array)
        .expect("framework detectors should be present");
    assert!(
        !detectors.iter().any(|detector| {
            detector
                .get("framework")
                .and_then(serde_json::Value::as_str)
                == Some("vue")
        }),
        "Nuxt-only projects should not synthesize a Vue detector row"
    );
    let detector = detectors
        .iter()
        .find(|detector| {
            detector.get("id").and_then(serde_json::Value::as_str) == Some("unprovided-inject")
                && detector
                    .get("framework")
                    .and_then(serde_json::Value::as_str)
                    == Some("nuxt")
        })
        .expect("Nuxt unprovided-inject detector should be reported");
    assert_eq!(
        detector.get("status").and_then(serde_json::Value::as_str),
        Some("not_checked")
    );
    assert_eq!(
        detector.get("reason").and_then(serde_json::Value::as_str),
        Some("requires_vue_runtime_dependency")
    );
}

#[test]
fn combined_human_explain_renders_inline_descriptions() {
    let combined = run_fallow_combined("basic-project", &["--quiet", "--explain"]);
    assert!(
        combined.code == 0 || combined.code == 1,
        "combined --explain should not crash, got exit code {}",
        combined.code
    );
    assert!(
        combined
            .stdout
            .contains("Description: Named exports that are never imported"),
        "combined --explain should render dead-code descriptions inline, stdout:\n{}",
        combined.stdout
    );
}

#[test]
fn check_grouped_human_explain_renders_inline_descriptions() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--quiet", "--explain", "--group-by", "directory"],
    );
    assert!(
        output
            .stdout
            .contains("Description: Named exports that are never imported"),
        "check --group-by --explain should render dead-code descriptions inline, stdout:\n{}",
        output.stdout
    );
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

#[test]
fn save_baseline_creates_file() {
    let project = common::copy_fixture("basic-project");
    let baseline_path = project.path().join("fallow-baselines/dead-code.json");

    let output = run_fallow_in_root(
        "check",
        project.path(),
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
        "save-baseline should not crash: {}",
        output.stderr
    );
    assert!(
        baseline_path.exists(),
        "--save-baseline should create the baseline file"
    );
    let content = std::fs::read_to_string(&baseline_path).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&content).expect("baseline file should be valid JSON");
}

#[test]
fn baseline_filters_known_issues() {
    let project = common::copy_fixture("basic-project");
    let baseline_path = project.path().join("baseline.json");

    let saved = run_fallow_in_root(
        "check",
        project.path(),
        &[
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        baseline_path.is_file(),
        "the baseline must be saved before it can filter: {}",
        saved.stdout
    );

    let output = run_fallow_in_root(
        "check",
        project.path(),
        &[
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let total = json["total_issues"]
        .as_u64()
        .unwrap_or_else(|| panic!("a report with total_issues: {json}"));
    assert_eq!(
        total, 0,
        "baseline should filter all known issues, got {total}"
    );
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

/// #2091 made workspace discovery fatal for analysis commands: a root
/// `package.json` that cannot be parsed must exit 2 with the malformed-JSON
/// message instead of analyzing a fictional workspace layout. The JSON error
/// envelope on stdout is a published contract.
#[test]
fn malformed_root_package_json_exits_2_across_commands() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mk src");
    std::fs::write(root.join("package.json"), "{ not json").expect("write package.json");
    std::fs::write(root.join("src").join("index.ts"), "export const a = 1;\n")
        .expect("write source");

    for command in ["check", "dupes", "list"] {
        let human = run_fallow_in_root(command, root, &["--quiet"]);
        assert_eq!(
            human.code, 2,
            "{command} should exit 2 on malformed root package.json, stderr: {}",
            human.stderr
        );
        assert!(
            human.stderr.contains("is not valid JSON"),
            "{command} stderr should explain the parse failure, got: {}",
            human.stderr
        );

        let json = run_fallow_in_root(command, root, &["--format", "json", "--quiet"]);
        assert_eq!(
            json.code, 2,
            "{command} --format json should exit 2, stderr: {}",
            json.stderr
        );
        let parsed: serde_json::Value = serde_json::from_str(&json.stdout).unwrap_or_else(|e| {
            panic!(
                "{command} stdout should be a JSON error envelope: {e}\nstdout: {}",
                json.stdout
            )
        });
        assert_eq!(parsed["error"], serde_json::Value::Bool(true));
        assert_eq!(parsed["exit_code"], serde_json::Value::from(2));
        assert!(
            parsed["message"]
                .as_str()
                .is_some_and(|msg| msg.contains("is not valid JSON")),
            "{command} envelope message should explain the parse failure, got: {}",
            json.stdout
        );
    }
}

/// Sibling of [`malformed_root_package_json_exits_2_across_commands`] for the
/// `MalformedRootDenoConfig` arm: a pure Deno root with an unparsable
/// `deno.json` must also exit 2 instead of proceeding without workspaces.
#[test]
fn malformed_root_deno_config_exits_2_with_structured_envelope() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mk src");
    std::fs::write(root.join("deno.json"), r#"{"name":"broken""#).expect("write deno.json");
    std::fs::write(root.join("src").join("index.ts"), "export const a = 1;\n")
        .expect("write source");

    let human = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        human.code, 2,
        "check should exit 2 on malformed root deno.json, stderr: {}",
        human.stderr
    );
    assert!(
        human.stderr.contains("is not valid JSONC"),
        "stderr should explain the parse failure, got: {}",
        human.stderr
    );

    let json = run_fallow_in_root("check", root, &["--format", "json", "--quiet"]);
    assert_eq!(json.code, 2, "stderr: {}", json.stderr);
    let parsed: serde_json::Value = serde_json::from_str(&json.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout should be a JSON error envelope: {e}\nstdout: {}",
            json.stdout
        )
    });
    assert_eq!(parsed["error"], serde_json::Value::Bool(true));
    assert_eq!(parsed["exit_code"], serde_json::Value::from(2));
    assert!(
        parsed["message"]
            .as_str()
            .is_some_and(|msg| msg.contains("is not valid JSONC")),
        "envelope message should explain the parse failure, got: {}",
        json.stdout
    );
}

#[test]
fn config_with_traversal_glob_exits_2() {
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

#[test]
fn combined_json_outside_git_repo_emits_single_document() {
    use std::process::Command;

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

    serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|e| {
        panic!(
            "combined mode outside a git repo must emit exactly one JSON document on stdout: {e}\nstdout was:\n{stdout}\nstderr was:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });

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

#[test]
fn config_with_unknown_boundary_zone_reference_exits_2() {
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
fn config_with_invalid_rule_pack_exits_2() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");
    std::fs::create_dir_all(root.join("packs")).expect("create packs dir");
    std::fs::write(
        root.join("packs/bad-kind.json"),
        r#"{
            "version": 1,
            "name": "bad-kind",
            "rules": [
                { "id": "no-foo", "kind": "banned-callee", "callees": ["foo.*"] }
            ]
        }"#,
    )
    .expect("write pack");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{ "rulePacks": ["packs/bad-kind.json", "packs/nonexistent.json"] }"#,
    )
    .expect("write config");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(
        output.code, 2,
        "an invalid or missing rule pack must fail the run instead of silently \
         skipping policy, stderr: {}",
        output.stderr
    );

    let stderr = &output.stderr;
    assert!(stderr.contains("invalid rule pack"), "stderr: {stderr}");
    assert!(
        stderr.contains("bad-kind.json"),
        "stderr should name the unparsable pack: {stderr}"
    );
    assert!(
        stderr.contains("nonexistent.json"),
        "stderr should collect the missing pack too: {stderr}"
    );
}

#[test]
fn fallow_config_subcommand_reports_unknown_boundary_zone_as_json() {
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
        "fallow config must reject invalid boundary config, stdout: {}",
        output.stdout
    );
    assert!(output.stderr.is_empty());
    let error: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("config errors should be structured JSON");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("typo-zone")),
        "JSON error should name the typo'd zone, got: {}",
        output.stdout
    );
}

#[test]
fn fallow_config_subcommand_json_format_emits_structured_error_envelope() {
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

#[test]
fn regression_baseline_schema_mismatch_json_format_emits_structured_error_envelope() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"test"}"#).expect("write package.json");

    let baseline_path = root.join("stale-baseline.json");
    std::fs::write(
        &baseline_path,
        r#"{
  "schema_version": 99,
  "fallow_version": "9.9.9",
  "timestamp": "2030-01-01T00:00:00Z",
  "check": {"total_issues": 0, "unused_files": 0}
}"#,
    )
    .expect("write baseline");

    let output = run_fallow_in_root(
        "check",
        root,
        &[
            "--regression-baseline",
            baseline_path.to_str().expect("utf-8 baseline path"),
            "--fail-on-regression",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 2,
        "schema mismatch should exit 2, stderr: {}",
        output.stderr
    );

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
    assert!(msg.contains("schema_version 99"), "msg: {msg}");
    assert!(msg.contains("expects 2"), "msg: {msg}");
    assert!(msg.contains("fallow 9.9.9"), "msg: {msg}");
    assert!(
        msg.contains("fallow dead-code --save-regression-baseline"),
        "msg should include regenerate command, msg: {msg}"
    );
}

/// Subcommands without a baseline reject the global `--baseline` and
/// `--save-baseline` flags with exit 2, before they do any work. The flag goes
/// before the subcommand here, which is the form a shared CI wrapper uses.
#[test]
fn subcommands_without_a_baseline_reject_global_baseline_flags() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name": "no-baseline"}"#).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
    let root_arg = root.to_str().unwrap();
    let target = root.join("out.json");
    let target_arg = target.to_str().unwrap();
    let commands: &[&[&str]] = &[
        &["list"],
        &["workspaces"],
        &["flags"],
        &["suppressions"],
        &["fix", "--dry-run"],
        &["inspect", "--file", "src/index.ts"],
        &["guard", "src/index.ts"],
        &["explain", "unused-exports"],
        &["config"],
        &["config-schema"],
        &["schema"],
        &["report", "--from", "saved.json"],
        &["decision-surface"],
        &["telemetry", "status"],
    ];
    for command in commands {
        for flag in ["--baseline", "--save-baseline"] {
            let mut args = vec![flag, target_arg, "--root", root_arg, "--format", "json"];
            args.extend_from_slice(command);
            let output = run_fallow_raw(&args);
            assert_eq!(
                output.code, 2,
                "{command:?} {flag} should exit 2. stdout: {} stderr: {}",
                output.stdout, output.stderr
            );
            let doc = parse_json(&output);
            let message = doc["message"].as_str().unwrap_or_default();
            assert!(
                message.contains(&format!("`fallow {}`", command[0])) && message.contains(flag),
                "{command:?} {flag}: {message}"
            );
            assert!(!target.exists(), "{command:?} {flag}");
        }
    }
}

/// The subcommands that use the global baseline flags keep them.
#[test]
fn baseline_subcommands_keep_the_global_save_baseline_flag() {
    for command in ["dead-code", "dupes", "health"] {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"name": "keeps-baseline"}"#).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
        let target = root.join("out.json");
        let output = run_fallow_in_root(
            command,
            root,
            &[
                "--save-baseline",
                target.to_str().unwrap(),
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_ne!(output.code, 2, "{command}: {}", output.stderr);
        assert!(target.exists(), "{command} writes the baseline");
    }
}

/// A project under `<tmp>/project` with one source file, and nothing else in
/// `<tmp>`, so a path in `<tmp>` is outside the project root.
fn write_confinement_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name": "confine"}"#).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
    (dir, root)
}

/// The system temp directory the child process sees: `<case>/system-tmp`.
/// Every other path in `<case>` is then outside both the project root and the
/// temp directory, which the save check allows.
fn system_tmp(case: &std::path::Path) -> std::path::PathBuf {
    let tmp = case.join("system-tmp");
    std::fs::create_dir_all(&tmp).expect("create system tmp");
    tmp
}

/// Run fallow from `cwd`, so a relative path resolves the way a user types it.
/// The child sees `<case>/system-tmp` as its temp directory and no
/// `RUNNER_TEMP`, unless `env` sets it.
fn run_fallow_from(
    case: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
) -> common::CommandOutput {
    run_fallow_from_env(case, cwd, args, &[])
}

fn run_fallow_from_env(
    case: &std::path::Path,
    cwd: &std::path::Path,
    args: &[&str],
    env: &[(&str, &std::path::Path)],
) -> common::CommandOutput {
    let tmp = system_tmp(case);
    let mut cmd = std::process::Command::new(fallow_bin());
    cmd.current_dir(cwd)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("TMPDIR", &tmp)
        .env("TMP", &tmp)
        .env("TEMP", &tmp)
        .env_remove("RUNNER_TEMP");
    common::scrub_coverage_env(&mut cmd);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd.args(args).output().expect("run fallow");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// The three save flags, each with the subcommand that writes it.
const SAVE_FLAGS: [(&str, &str); 3] = [
    ("dead-code", "--save-baseline"),
    ("dead-code", "--save-regression-baseline"),
    ("health", "--save-snapshot"),
];

/// A save path outside the project root fails with exit 2 before any work,
/// for a relative `../` path and for an absolute path.
#[test]
fn save_paths_outside_the_project_root_are_rejected() {
    let (dir, root) = write_confinement_project();
    let absolute = dir.path().join("absolute.json");
    for (command, flag) in SAVE_FLAGS {
        for target in ["../outside.json", absolute.to_str().unwrap()] {
            let output = run_fallow_from(
                dir.path(),
                &root,
                &[command, flag, target, "--format", "json", "--quiet"],
            );
            assert_eq!(
                output.code, 2,
                "{command} {flag} {target} should exit 2. stdout: {} stderr: {}",
                output.stdout, output.stderr
            );
            let doc = parse_json(&output);
            let message = doc["message"].as_str().unwrap_or_default();
            assert!(
                message.contains(flag) && message.contains("outside the project root"),
                "{command} {flag} {target}: {message}"
            );
        }
        assert!(
            !dir.path().join("outside.json").exists(),
            "{command} {flag}"
        );
        assert!(!absolute.exists(), "{command} {flag}");
    }
}

/// A save path inside the project root, in a nested directory, keeps working.
#[test]
fn save_paths_inside_the_project_root_keep_working() {
    let (dir, root) = write_confinement_project();
    for (command, flag) in SAVE_FLAGS {
        let target = format!("out/{}/file.json", flag.trim_start_matches("--"));
        let output = run_fallow_from(
            dir.path(),
            &root,
            &[command, flag, &target, "--format", "json", "--quiet"],
        );
        assert_ne!(output.code, 2, "{command} {flag}: {}", output.stderr);
        assert!(
            root.join(&target).is_file(),
            "{command} {flag} writes {target}"
        );
    }
}

/// A symlink inside the root that points outside it does not open a way out.
#[cfg(unix)]
#[test]
fn save_paths_through_a_symlink_that_leaves_the_root_are_rejected() {
    let (dir, root) = write_confinement_project();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("link")).unwrap();
    for (command, flag) in SAVE_FLAGS {
        let output = run_fallow_from(
            dir.path(),
            &root,
            &[
                command,
                flag,
                "link/file.json",
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_eq!(output.code, 2, "{command} {flag}: {}", output.stderr);
        assert!(!elsewhere.join("file.json").exists(), "{command} {flag}");
    }
}

/// A project root inside a Git work tree may save anywhere in that work tree,
/// so a monorepo job that runs from the repository root with
/// `--root packages/app` keeps its repository-relative baseline path.
#[test]
fn save_paths_in_the_git_work_tree_of_the_root_keep_working() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let repo = dir.path().join("repo");
    let root = repo.join("packages/app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name": "app"}"#).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
    common::git(&repo, &["init", "-q"]);
    let output = run_fallow_from(
        dir.path(),
        &repo,
        &[
            "dead-code",
            "--root",
            "packages/app",
            "--save-baseline",
            "baselines/app.json",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_ne!(output.code, 2, "{}", output.stderr);
    assert!(repo.join("baselines/app.json").is_file());

    let output = run_fallow_from(
        dir.path(),
        &repo,
        &[
            "dead-code",
            "--root",
            "packages/app",
            "--save-baseline",
            "../outside.json",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(!dir.path().join("outside.json").exists());
}

/// A save into the system temp directory is allowed: CI jobs keep baselines
/// there between steps.
#[test]
fn save_paths_in_the_system_temp_dir_keep_working() {
    let (dir, root) = write_confinement_project();
    let tmp = system_tmp(dir.path());
    for (command, flag) in SAVE_FLAGS {
        let target = tmp.join(format!("{}.json", flag.trim_start_matches("--")));
        let output = run_fallow_from(
            dir.path(),
            &root,
            &[
                command,
                flag,
                target.to_str().unwrap(),
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_ne!(output.code, 2, "{command} {flag}: {}", output.stdout);
        assert!(
            target.is_file(),
            "{command} {flag} writes into the temp dir"
        );
    }
}

/// A save into `RUNNER_TEMP` is allowed, also when it is not the system temp
/// directory, as on a self-hosted GitHub runner.
#[test]
fn save_paths_in_runner_temp_keep_working() {
    let (dir, root) = write_confinement_project();
    let runner_temp = dir.path().join("runner-temp");
    std::fs::create_dir_all(&runner_temp).unwrap();
    for (command, flag) in SAVE_FLAGS {
        let target = runner_temp.join(format!("{}.json", flag.trim_start_matches("--")));
        let output = run_fallow_from_env(
            dir.path(),
            &root,
            &[
                command,
                flag,
                target.to_str().unwrap(),
                "--format",
                "json",
                "--quiet",
            ],
            &[("RUNNER_TEMP", &runner_temp)],
        );
        assert_ne!(output.code, 2, "{command} {flag}: {}", output.stdout);
        assert!(target.is_file(), "{command} {flag} writes into RUNNER_TEMP");
    }
}

/// A save into the home directory is outside the project, the Git work tree
/// and the temp directories, so it is rejected.
#[cfg(unix)]
#[test]
fn save_paths_in_the_home_dir_are_rejected() {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return;
    };
    let (dir, root) = write_confinement_project();
    for (command, flag) in SAVE_FLAGS {
        let target = home.join(format!(
            ".fallow-confine-probe-{}-{}.json",
            std::process::id(),
            flag.trim_start_matches("--")
        ));
        let output = run_fallow_from(
            dir.path(),
            &root,
            &[
                command,
                flag,
                target.to_str().unwrap(),
                "--format",
                "json",
                "--quiet",
            ],
        );
        let written = target.exists();
        let _ = std::fs::remove_file(&target);
        assert_eq!(output.code, 2, "{command} {flag}: {}", output.stdout);
        assert!(
            !written,
            "{command} {flag} must not write into the home dir"
        );
    }
}

/// A committed `.fallow` symlink that points outside the project must not
/// carry the default snapshot write out of it.
#[cfg(unix)]
#[test]
fn a_default_snapshot_through_a_fallow_symlink_is_rejected() {
    let (dir, root) = write_confinement_project();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join(".fallow")).unwrap();
    for args in [
        &["health", "--save-snapshot", "--format", "json", "--quiet"][..],
        &["--save-snapshot", "--format", "json", "--quiet"][..],
    ] {
        let output = run_fallow_from(dir.path(), &root, args);
        assert_eq!(output.code, 2, "{args:?}: {}", output.stdout);
        let message = parse_json(&output)["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(message.contains("--save-snapshot"), "{args:?}: {message}");
        assert!(
            !elsewhere.join("snapshots").exists(),
            "{args:?} must not write a snapshot through the link"
        );
    }
}

/// A flag-only `--save-regression-baseline` rewrites the config file. A
/// committed config symlink that points outside the project, dangling or not,
/// must not carry that write out of it.
#[cfg(unix)]
#[test]
fn a_config_rewrite_through_a_config_symlink_is_rejected() {
    for dangling in [true, false] {
        let (dir, root) = write_confinement_project();
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let target = elsewhere.join("config.json");
        if !dangling {
            std::fs::write(&target, "{}\n").unwrap();
        }
        std::os::unix::fs::symlink(&target, root.join(".fallowrc.json")).unwrap();
        let output = run_fallow_from(
            dir.path(),
            &root,
            &[
                "dead-code",
                "--save-regression-baseline",
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_eq!(output.code, 2, "dangling={dangling}: {}", output.stdout);
        let message = parse_json(&output)["message"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(
            message.contains("--save-regression-baseline"),
            "dangling={dangling}: {message}"
        );
        if dangling {
            assert!(
                !target.exists(),
                "the config write must not create the target"
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "{}\n",
                "the config write must not rewrite the target"
            );
        }
    }
}

/// The default destinations inside the project keep working.
#[test]
fn default_save_destinations_inside_the_project_keep_working() {
    let (dir, root) = write_confinement_project();
    let output = run_fallow_from(
        dir.path(),
        &root,
        &["health", "--save-snapshot", "--format", "json", "--quiet"],
    );
    assert_ne!(output.code, 2, "{}", output.stdout);
    assert!(root.join(".fallow/snapshots").is_dir());
    let output = run_fallow_from(
        dir.path(),
        &root,
        &[
            "dead-code",
            "--save-regression-baseline",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_ne!(output.code, 2, "{}", output.stdout);
    assert!(root.join(".fallowrc.json").is_file());
}

/// The Git work tree counts only when the working directory is inside it. A
/// run from outside the repository must not save into the repository just
/// because the root sits in it, which matters when `$HOME` itself is a Git
/// repository.
#[test]
fn the_git_work_tree_counts_only_for_a_working_directory_inside_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let repo = dir.path().join("repo");
    let root = repo.join("packages/app");
    let outside_cwd = dir.path().join("elsewhere");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(&outside_cwd).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name": "app"}"#).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
    common::git(&repo, &["init", "-q"]);
    let target = repo.join("baselines/app.json");
    let output = run_fallow_from(
        dir.path(),
        &outside_cwd,
        &[
            "dead-code",
            "--root",
            root.to_str().unwrap(),
            "--save-baseline",
            target.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(!target.exists());
}
