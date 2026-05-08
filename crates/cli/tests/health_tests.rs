#[path = "common/mod.rs"]
mod common;

use common::{fixture_path, parse_json, redact_all, run_fallow, run_fallow_in_root};
use std::path::Path;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination directory");
    for entry in std::fs::read_dir(src).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type().expect("read source entry type");
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else if !file_type.is_dir() {
            std::fs::copy(&src_path, &dst_path).expect("copy file");
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        // Isolate from parent git context (pre-push hook sets GIT_DIR to the main repo,
        // which overrides current_dir and causes commits to leak into the real repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} should succeed");
}

// ---------------------------------------------------------------------------
// JSON output structure
// ---------------------------------------------------------------------------

#[test]
fn health_json_output_is_valid() {
    // Disable the default CRAP gate (30.0) so the fixture's branchy untested
    // function doesn't push the process to exit 1. This test only verifies
    // shape, not findings.
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--max-crap", "10000", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "health should succeed");
    let json = parse_json(&output);
    assert!(json.is_object(), "health JSON output should be an object");
}

#[test]
fn health_rejects_relative_coverage_root() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--coverage-root", "src", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 2,
        "relative --coverage-root should be rejected before health runs. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

#[test]
fn health_json_has_findings() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--complexity", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    // complexity-project has a function with cyclomatic > 10
    assert!(
        json.get("findings").is_some(),
        "health JSON should have findings key"
    );
}

#[test]
fn health_reports_angular_template_complexity() {
    let output = run_fallow(
        "health",
        "angular-template-complexity",
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("permissions.component.html"))
        })
        .unwrap_or_else(|| panic!("expected template complexity finding, got: {findings:#?}"));

    assert!(
        template["cyclomatic"].as_u64().unwrap_or_default() > 3,
        "template should exceed cyclomatic threshold: {template:#?}"
    );
    assert!(
        template["cognitive"].as_u64().unwrap_or_default() > 3,
        "template should exceed cognitive threshold: {template:#?}"
    );
    let actions = template["actions"].as_array().expect("actions array");
    let suppress = actions
        .iter()
        .find(|action| action["type"] == "suppress-file")
        .unwrap_or_else(|| panic!("expected HTML suppress action, got: {actions:#?}"));
    assert_eq!(
        suppress["comment"],
        "<!-- fallow-ignore-file complexity -->"
    );
}

#[test]
fn health_reports_angular_inline_template_complexity() {
    let output = run_fallow(
        "health",
        "angular-inline-template-complexity",
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("host-game.component.ts"))
        })
        .unwrap_or_else(|| {
            panic!("expected inline template complexity finding, got: {findings:#?}")
        });

    assert!(
        template["cyclomatic"].as_u64().unwrap_or_default() > 3,
        "inline template should exceed cyclomatic threshold: {template:#?}"
    );
    assert!(
        template["cognitive"].as_u64().unwrap_or_default() > 3,
        "inline template should exceed cognitive threshold: {template:#?}"
    );
    // Anchored at the `@Component` decorator (line 16 of host-game.component.ts).
    assert_eq!(
        template["line"].as_u64(),
        Some(16),
        "inline template finding should anchor at the @Component decorator: {template:#?}"
    );
    // The .ts host file uses TS-style suppression actions, not the HTML
    // suppress-file action that external `templateUrl` files emit.
    let actions = template["actions"].as_array().expect("actions array");
    assert!(
        actions
            .iter()
            .any(|action| action["type"] == "suppress-line"),
        "inline template finding should expose a suppress-line action: {actions:#?}"
    );
    let suppress_line = actions
        .iter()
        .find(|action| action["type"] == "suppress-line")
        .expect("suppress-line action");
    assert_eq!(
        suppress_line["placement"].as_str(),
        Some("above-angular-decorator"),
        "inline template suppress-line should point at the decorator: {actions:#?}"
    );
    assert!(
        actions
            .iter()
            .all(|action| action["type"] != "suppress-file"),
        "inline template finding should not emit the HTML suppress-file action: {actions:#?}"
    );
}

#[test]
fn health_inline_template_complexity_can_be_suppressed() {
    let dir = tempdir().unwrap();
    let fixture = fixture_path("angular-inline-template-complexity");
    copy_dir_recursive(&fixture, dir.path());

    let component_path = dir.path().join("src/host-game.component.ts");
    let original = std::fs::read_to_string(&component_path).expect("read component");
    let prefixed = original.replacen(
        "@Component({",
        "// fallow-ignore-next-line complexity\n@Component({",
        1,
    );
    assert_ne!(
        original, prefixed,
        "fixture should contain a @Component decorator"
    );
    std::fs::write(&component_path, prefixed).expect("write suppressed component");

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "suppressed inline template should not fail health"
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array();
    assert!(
        findings.is_none_or(|arr| arr.iter().all(|f| f["name"] != "<template>")),
        "suppressed inline template should not emit a <template> finding: {json:#?}"
    );
}

#[test]
fn health_html_template_complexity_can_be_suppressed() {
    let dir = tempdir().unwrap();
    let fixture = fixture_path("angular-template-complexity");
    copy_dir_recursive(&fixture, dir.path());

    let template_path = dir.path().join("src/permissions.component.html");
    let original = std::fs::read_to_string(&template_path).expect("read template");
    std::fs::write(
        &template_path,
        format!("<!-- fallow-ignore-file complexity -->\n{original}"),
    )
    .expect("write suppressed template");

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "suppressed template should not fail health");
    let json = parse_json(&output);
    assert!(
        json["findings"].as_array().is_none_or(Vec::is_empty),
        "suppressed template should not emit findings: {json:#?}"
    );
}

#[test]
fn health_save_baseline_creates_parent_directory() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"health-save","version":"1.0.0"}"#,
    );
    write_file(
        &dir.path().join("src/index.ts"),
        r"export function alpha(value: number): number {
  if (value > 10) return value * 2;
  return value + 1;
}
",
    );

    let baseline_path = dir.path().join("fallow-baselines/health.json");
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--targets",
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    let rendered = redact_all(&format!("{}\n{}", output.stdout, output.stderr), dir.path());
    assert_eq!(
        output.code, 0,
        "health save baseline should succeed: {rendered}"
    );
    assert!(
        baseline_path.exists(),
        "health save baseline should create nested file: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Exit code with threshold
// ---------------------------------------------------------------------------

#[test]
fn health_exits_0_below_threshold() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "50",
            // Raise the CRAP gate out of the way so this test isolates the
            // cyclomatic/cognitive behaviour under test.
            "--max-crap",
            "10000",
            "--complexity",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "health should exit 0 when complexity below threshold"
    );
}

#[test]
fn health_exits_1_when_threshold_exceeded() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "3",
            "--complexity",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should exit 1 when complexity exceeds threshold"
    );
}

// ---------------------------------------------------------------------------
// CRAP threshold (--max-crap)
// ---------------------------------------------------------------------------

/// With a high `--max-crap`, no function should trigger a CRAP finding and the
/// summary's `max_crap_threshold` must reflect the CLI override.
#[test]
fn health_exits_0_when_crap_below_threshold() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "99",
            "--max-crap",
            "10000",
            "--complexity",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "health should exit 0 when CRAP stays below a very high threshold"
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(
        json["summary"]["max_crap_threshold"].as_f64(),
        Some(10_000.0),
        "summary should echo the CLI-supplied threshold"
    );
}

/// With a very low `--max-crap`, every nontrivial function should become a
/// finding and the command must exit 1.
#[test]
fn health_exits_1_when_crap_threshold_exceeded() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "9999",
            "--max-cognitive",
            "9999",
            "--max-crap",
            "1",
            "--complexity",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should exit 1 when any function has CRAP >= 1"
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    let findings = json["findings"].as_array().expect("findings array");
    assert!(
        !findings.is_empty(),
        "crap-triggered run should emit at least one finding"
    );
    let any_crap = findings
        .iter()
        .any(|f| f.get("crap").and_then(|v| v.as_f64()).is_some());
    assert!(
        any_crap,
        "at least one finding should carry a populated `crap` score when --max-crap triggered"
    );
}

// ---------------------------------------------------------------------------
// Section flags
// ---------------------------------------------------------------------------

#[test]
fn health_score_flag_shows_score() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--score", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("score").is_some() || json.get("health_score").is_some(),
        "health --score should include score data"
    );
    let penalties = json["health_score"]["penalties"]
        .as_object()
        .expect("health --score should include penalty breakdown");
    assert!(
        !penalties.contains_key("hotspots"),
        "health --score should not run churn-backed hotspot analysis unless --hotspots is requested"
    );
    assert!(
        json.get("file_scores").is_none(),
        "health --score should not render file_scores"
    );
    assert!(
        json.get("coverage_gaps").is_none(),
        "health --score should not render coverage_gaps"
    );
    assert!(
        json.get("hotspot_summary").is_none(),
        "health --score should not render hotspot summaries"
    );
    assert!(
        json.get("vital_signs").is_none(),
        "health --score should not render vital signs"
    );
}

#[test]
fn health_score_save_snapshot_keeps_hotspot_vital_signs() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"health-score-snapshot","version":"1.0.0","type":"module"}"#,
    );
    write_file(
        &root.join("src/index.ts"),
        "export function risky(x: number) { if (x > 1) { if (x > 2) { if (x > 3) { if (x > 4) { if (x > 5) { return x; } } } } } return 0; }\n",
    );
    git(root, &["init"]);
    git(root, &["config", "user.email", "review@example.test"]);
    git(root, &["config", "user.name", "Review"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
    write_file(
        &root.join("src/index.ts"),
        "export function risky(x: number) { if (x > 1) { if (x > 2) { if (x > 3) { if (x > 4) { if (x > 5) { if (x > 6) { return x; } } } } } } return 0; }\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "increase churn"]);

    let score_only = run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let score_json = parse_json(&score_only);
    assert!(
        !score_json["health_score"]["penalties"]
            .as_object()
            .expect("score penalties")
            .contains_key("hotspots"),
        "plain --score should not compute churn-backed hotspot penalties"
    );

    let snapshot = run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--save-snapshot",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let snapshot_json = parse_json(&snapshot);
    assert!(
        snapshot_json["health_score"]["penalties"]
            .as_object()
            .expect("snapshot score penalties")
            .contains_key("hotspots"),
        "snapshot score should include the hotspot penalty when hotspot vitals were computed"
    );

    let snapshot_dir = root.join(".fallow/snapshots");
    let snapshot_path = std::fs::read_dir(&snapshot_dir)
        .expect("read snapshot dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("snapshot json should be saved");
    let saved_snapshot: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(snapshot_path).expect("read snapshot"))
            .expect("parse snapshot json");
    assert_eq!(
        saved_snapshot["vital_signs"]["hotspot_count"].as_u64(),
        Some(1),
        "--score --save-snapshot should still save hotspot vital signs"
    );

    let trend = run_fallow_in_root(
        "health",
        root,
        &[
            "--trend",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let trend_json = parse_json(&trend);
    let trend_metrics = trend_json["health_trend"]["metrics"]
        .as_array()
        .expect("trend metrics");
    assert!(
        trend_metrics
            .iter()
            .any(|metric| metric["name"] == "hotspot_count"),
        "--trend should compare hotspot counts from complete snapshot data"
    );
}

#[test]
fn health_score_flag_with_config_does_not_render_coverage_gaps() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "warn"
  }
}"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--score",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "health --score should still succeed");

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_none(),
        "config-enabled coverage gaps should not override explicit section selection"
    );
}

#[test]
fn health_baseline_partial_overflow_does_not_emit_stale_baseline_warning() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"baseline-health-repro","type":"module"}"#,
    );
    write_file(
        &dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2020","module":"ES2020","strict":true},"include":["src"]}"#,
    );
    write_file(
        &dir.path().join("src/index.ts"),
        r#"export function alpha(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}"#,
    );

    let baseline_path = dir.path().join("health-baseline.json");
    let baseline_path_str = baseline_path
        .to_str()
        .expect("baseline path should be valid UTF-8");

    let save = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--save-baseline",
            baseline_path_str,
        ],
    );
    let save_output = redact_all(&format!("{}\n{}", save.stdout, save.stderr), dir.path());
    assert!(
        save.code == 0 || save.code == 1,
        "save baseline should not crash: {save_output}"
    );
    assert!(
        baseline_path.exists(),
        "save baseline should create the baseline file: {save_output}"
    );
    assert!(
        save_output.contains("Saved health baseline to"),
        "save baseline should confirm the write: {save_output}"
    );

    write_file(
        &dir.path().join("src/index.ts"),
        r#"export function alpha(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}

export function beta(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}"#,
    );

    let load = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--baseline",
            baseline_path_str,
        ],
    );
    let combined = redact_all(&format!("{}\n{}", load.stdout, load.stderr), dir.path());
    assert_eq!(
        load.code, 1,
        "baseline load should still report the overflowing findings: {combined}"
    );
    assert!(
        combined.contains("alpha") && combined.contains("beta"),
        "expected overflow run to still report both functions: {combined}"
    );
    assert!(
        !combined.contains("Warning: health baseline has"),
        "partial-overflow baseline should not look stale: {combined}"
    );
}

#[test]
fn health_score_flag_with_config_error_fails_without_rendering_coverage_gaps() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "error"
  }
}
"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--score",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "coverage-gaps=error should still fail score-only health runs"
    );

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_none(),
        "gate-only coverage gaps should not be rendered in score-only output"
    );
}

#[test]
fn health_file_scores_flag() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("file_scores").is_some(),
        "health --file-scores should include file_scores"
    );
}

#[test]
fn health_file_scores_include_vue_sfc_files() {
    let output = run_fallow(
        "health",
        "vue-split-type-value-export",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "health should score Vue SFC files");

    let json = parse_json(&output);
    let file_scores = json["file_scores"]
        .as_array()
        .expect("health --file-scores should include file_scores");

    assert!(
        file_scores.iter().any(|score| {
            score.get("path").and_then(serde_json::Value::as_str) == Some("src/App.vue")
        }),
        "Vue SFC files should be included in file_scores: {file_scores:?}"
    );
}

#[test]
fn health_complexity_reports_vue_sfc_functions() {
    let output = run_fallow(
        "health",
        "vue-split-type-value-export",
        &[
            "--complexity",
            "--max-cyclomatic",
            "0",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should report Vue SFC complexity findings"
    );

    let json = parse_json(&output);
    let findings = json["findings"]
        .as_array()
        .expect("health --complexity should include findings");

    assert!(
        findings.iter().any(|finding| {
            finding.get("path").and_then(serde_json::Value::as_str) == Some("src/App.vue")
                && finding.get("name").and_then(serde_json::Value::as_str) == Some("isStatus")
        }),
        "Vue SFC functions should surface as health findings: {findings:?}"
    );
}

#[test]
fn health_coverage_gaps_flag_reports_runtime_gaps() {
    let output = run_fallow(
        "health",
        "coverage-gaps",
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "health --coverage-gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json
        .get("coverage_gaps")
        .expect("health --coverage-gaps should include coverage_gaps");
    let files = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array");
    let exports = coverage["exports"]
        .as_array()
        .expect("coverage_gaps.exports should be an array");

    let file_names: Vec<String> = files
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert!(
        file_names
            .iter()
            .any(|path| path.ends_with("src/setup-only.ts")),
        "setup-only.ts should remain untested even when referenced by test setup: {file_names:?}"
    );
    assert!(
        file_names
            .iter()
            .any(|path| path.ends_with("src/fixture-only.ts")),
        "fixture-only.ts should remain untested even when referenced by a fixture: {file_names:?}"
    );
    assert!(
        !file_names
            .iter()
            .any(|path| path.ends_with("src/covered.ts")),
        "covered.ts should not be reported as an untested file: {file_names:?}"
    );

    let export_names: Vec<_> = exports
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"covered"),
        "covered should not be reported as an untested export: {export_names:?}"
    );
    assert!(
        !export_names.contains(&"indirectlyCovered"),
        "exports already reported as dead code should be excluded from coverage gaps: {export_names:?}"
    );
}

#[test]
fn health_coverage_gaps_config_error_enforces_without_flag() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "error"
  }
}
"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "coverage-gaps=error should fail health even without --coverage-gaps"
    );

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_some(),
        "config-enabled coverage gaps should be present in the report"
    );
}

#[test]
fn health_coverage_gaps_production_excludes_dead_test_helpers() {
    let output = run_fallow(
        "health",
        "production-mode",
        &[
            "--production",
            "--coverage-gaps",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "runtime coverage gaps default to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("runtime coverage_gaps should be an object");

    let export_names: Vec<_> = coverage["exports"]
        .as_array()
        .expect("coverage_gaps.exports should be an array")
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"testHelper"),
        "exports already reported as dead code should not also be reported as coverage gaps: {export_names:?}"
    );
    assert!(
        export_names.contains(&"app") && export_names.contains(&"helper"),
        "runtime coverage gaps should still report runtime exports lacking test reachability: {export_names:?}"
    );

    let summary = coverage["summary"]
        .as_object()
        .expect("coverage_gaps.summary should be an object");
    assert_eq!(
        summary["untested_exports"].as_u64(),
        Some(2),
        "runtime coverage gaps should exclude dead exports from the export count"
    );
}

#[test]
fn health_coverage_gaps_suppressed_file_excluded() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    copy_dir_recursive(&fixture_path("coverage-gaps"), root);

    // Add suppression comment to setup-only.ts
    write_file(
        &root.join("src/setup-only.ts"),
        r#"// fallow-ignore-file coverage-gaps
export function viaSetup(): string {
  return "setup";
}
"#,
    );

    let output = common::run_fallow_in_root(
        "health",
        root,
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );

    let json = parse_json(&output);
    let coverage = json
        .get("coverage_gaps")
        .expect("coverage_gaps should be present");
    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("files array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();

    assert!(
        !file_paths
            .iter()
            .any(|path| path.ends_with("src/setup-only.ts")),
        "setup-only.ts should be excluded when suppressed with fallow-ignore-file: {file_paths:?}"
    );

    let export_names: Vec<_> = coverage["exports"]
        .as_array()
        .expect("exports array")
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"viaSetup"),
        "viaSetup export should be excluded when file is suppressed: {export_names:?}"
    );
}

#[test]
fn health_coverage_gaps_workspace_scope_limits_results() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "coverage-gaps-workspace",
  "private": true,
  "workspaces": ["packages/*"],
  "dependencies": {
    "vitest": "^3.2.4"
  }
}"#,
    );

    write_file(
        &root.join("packages/app/package.json"),
        r#"{
  "name": "app",
  "main": "src/main.ts"
}"#,
    );
    write_file(
        &root.join("packages/app/src/main.ts"),
        r#"import { covered } from "./covered";
import { appGap } from "./app-gap";

export const app = `${covered()}:${appGap()}`;
"#,
    );
    write_file(
        &root.join("packages/app/src/covered.ts"),
        r#"export function covered(): string {
  return "covered";
}
"#,
    );
    write_file(
        &root.join("packages/app/src/app-gap.ts"),
        r#"export function appGap(): string {
  return "app-gap";
}
"#,
    );
    write_file(
        &root.join("packages/app/tests/covered.test.ts"),
        r#"import { describe, expect, it } from "vitest";
import { covered } from "../src/covered";

describe("covered", () => {
  it("covers app runtime code selectively", () => {
    expect(covered()).toBe("covered");
  });
});
"#,
    );

    write_file(
        &root.join("packages/shared/package.json"),
        r#"{
  "name": "shared",
  "main": "src/index.ts"
}"#,
    );
    write_file(
        &root.join("packages/shared/src/index.ts"),
        r#"import { sharedGap } from "./shared-gap";

export const shared = sharedGap();
"#,
    );
    write_file(
        &root.join("packages/shared/src/shared-gap.ts"),
        r#"export function sharedGap(): string {
  return "shared-gap";
}
"#,
    );

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--coverage-gaps",
            "--workspace",
            "app",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "workspace-scoped health --coverage-gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("workspace-scoped coverage_gaps should be an object");

    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert!(
        file_paths.iter().all(|path| path.contains("packages/app/")),
        "workspace scope should only report app package files: {file_paths:?}"
    );
    assert!(
        file_paths
            .iter()
            .any(|path| path.ends_with("packages/app/src/app-gap.ts")),
        "app gap should be reported in workspace scope: {file_paths:?}"
    );
    assert!(
        !file_paths
            .iter()
            .any(|path| path.contains("packages/shared")),
        "shared package gaps should be excluded from app workspace scope: {file_paths:?}"
    );
}

#[test]
fn health_workspace_scopes_vital_signs_and_health_score() {
    // Regression: --workspace scoped findings/file_scores correctly but left
    // vital_signs and health_score at monorepo-wide values, masking a
    // significant divergence between project- and workspace-level health.
    // Issue #184.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-health-scope",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join(".fallowrc.json"),
        r#"{"duplicates":{"min_tokens":10,"min_lines":3}}"#,
    );
    // app: small, simple package
    write_file(
        &root.join("packages/app/package.json"),
        r#"{ "name": "app", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/app/src/index.ts"),
        r"export const greet = (name: string): string => `hello ${name}`;
",
    );
    // lib: a larger package (more files contribute to LOC / file count)
    write_file(
        &root.join("packages/lib/package.json"),
        r#"{ "name": "lib", "main": "src/index.ts" }"#,
    );
    for i in 0..5 {
        write_file(
            &root.join(format!("packages/lib/src/util_{i}.ts")),
            &format!("export const fn_{i} = (a: number, b: number): number => a + b + {i};\n"),
        );
    }
    write_file(
        &root.join("packages/lib/src/index.ts"),
        r#"export * from "./util_0";
export * from "./util_1";
export * from "./util_2";
export * from "./util_3";
export * from "./util_4";
"#,
    );
    let duplicated_lib_function = r"export function duplicated(input: number): number {
  const first = input + 1;
  const second = first * 2;
  const third = second - 3;
  const fourth = third / 4;
  const fifth = fourth + 5;
  return fifth;
}
";
    write_file(
        &root.join("packages/lib/src/dup_a.ts"),
        duplicated_lib_function,
    );
    write_file(
        &root.join("packages/lib/src/dup_b.ts"),
        duplicated_lib_function,
    );

    // `--score` forces hotspot analysis, which requires a git repo.
    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    let monorepo = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(monorepo.code, 0, "monorepo health run should succeed");
    let monorepo_json = parse_json(&monorepo);

    let snapshot_path = root.join(".fallow/app-snapshot.json");
    let snapshot_arg = snapshot_path.to_string_lossy().to_string();
    let scoped = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--workspace",
            "app",
            "--save-snapshot",
            &snapshot_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(scoped.code, 0, "workspace-scoped health run should succeed");
    let scoped_json = parse_json(&scoped);

    let monorepo_files = monorepo_json["summary"]["files_analyzed"]
        .as_u64()
        .expect("monorepo summary.files_analyzed");
    let scoped_files = scoped_json["summary"]["files_analyzed"]
        .as_u64()
        .expect("scoped summary.files_analyzed");
    assert!(
        scoped_files < monorepo_files,
        "summary.files_analyzed must scope to workspace (monorepo: {monorepo_files}, scoped: {scoped_files})"
    );

    let monorepo_loc = monorepo_json["vital_signs"]["total_loc"]
        .as_u64()
        .expect("monorepo vital_signs.total_loc");
    let scoped_loc = scoped_json["vital_signs"]["total_loc"]
        .as_u64()
        .expect("scoped vital_signs.total_loc");
    assert!(
        scoped_loc < monorepo_loc,
        "vital_signs.total_loc must scope to workspace (monorepo: {monorepo_loc}, scoped: {scoped_loc})"
    );

    let monorepo_duplication = monorepo_json["vital_signs"]["duplication_pct"]
        .as_f64()
        .expect("monorepo vital_signs.duplication_pct");
    let scoped_duplication = scoped_json["vital_signs"]["duplication_pct"]
        .as_f64()
        .expect("scoped vital_signs.duplication_pct");
    assert!(
        monorepo_duplication > scoped_duplication,
        "workspace score must not inherit duplication from another workspace (monorepo: {monorepo_duplication}, scoped: {scoped_duplication})"
    );
    assert!(
        scoped_duplication.abs() < f64::EPSILON,
        "app workspace has no duplicates, so scoped duplication should be zero"
    );
    assert_eq!(
        scoped_json["health_score"]["penalties"]["duplication"].as_f64(),
        Some(0.0),
        "app health score should not carry lib's duplication penalty"
    );

    let snapshot: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&snapshot_path).expect("read saved app snapshot"),
    )
    .expect("parse saved app snapshot");
    assert_eq!(
        snapshot["counts"]["total_lines"], scoped_json["vital_signs"]["counts"]["total_lines"],
        "snapshot count totals must use the same workspace scope as JSON vital signs"
    );
}

#[test]
fn health_group_by_package_emits_per_workspace_envelope() {
    // Regression: --group-by package was accepted by `fallow health` but the
    // resolver was silently discarded; consumers got monorepo-wide output
    // with no `grouped_by` or `groups` keys. Issue #184.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-grouped",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join(".fallowrc.json"),
        r#"{"duplicates":{"min_tokens":10,"min_lines":3}}"#,
    );
    write_file(
        &root.join("packages/alpha/package.json"),
        r#"{ "name": "alpha", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/alpha/src/index.ts"),
        "export const a = (n: number): number => n * 2;\n",
    );
    write_file(
        &root.join("packages/beta/package.json"),
        r#"{ "name": "beta", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/beta/src/index.ts"),
        "export const b = (n: number): number => n + 1;\n",
    );
    let duplicated_beta_function = r"export function duplicated(input: number): number {
  const first = input + 1;
  const second = first * 2;
  const third = second - 3;
  const fourth = third / 4;
  const fifth = fourth + 5;
  return fifth;
}
";
    write_file(
        &root.join("packages/beta/src/dup_a.ts"),
        duplicated_beta_function,
    );
    write_file(
        &root.join("packages/beta/src/dup_b.ts"),
        duplicated_beta_function,
    );

    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--group-by",
            "package",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "grouped health run should succeed");
    let json = parse_json(&output);

    assert_eq!(
        json["grouped_by"].as_str(),
        Some("package"),
        "grouped_by should be 'package'"
    );
    let groups = json["groups"]
        .as_array()
        .expect("groups should be an array");
    let keys: Vec<&str> = groups.iter().filter_map(|g| g["key"].as_str()).collect();
    assert!(
        keys.contains(&"alpha"),
        "groups must include alpha workspace: {keys:?}"
    );
    assert!(
        keys.contains(&"beta"),
        "groups must include beta workspace: {keys:?}"
    );

    for group in groups {
        let key = group["key"].as_str().unwrap_or("?");
        assert!(
            group.get("vital_signs").is_some(),
            "group {key} must carry per-group vital_signs"
        );
        assert!(
            group.get("health_score").is_some(),
            "group {key} must carry per-group health_score"
        );
        assert!(
            group["files_analyzed"].as_u64().is_some(),
            "group {key} must report files_analyzed"
        );
    }
    let alpha = groups
        .iter()
        .find(|g| g["key"] == "alpha")
        .expect("alpha group");
    let beta = groups
        .iter()
        .find(|g| g["key"] == "beta")
        .expect("beta group");
    assert_eq!(
        alpha["vital_signs"]["duplication_pct"].as_f64(),
        Some(0.0),
        "alpha must not inherit beta's duplicate-code score input"
    );
    assert!(
        beta["vital_signs"]["duplication_pct"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "beta should carry its own duplicate-code score input"
    );
    assert_eq!(
        alpha["health_score"]["penalties"]["duplication"].as_f64(),
        Some(0.0),
        "alpha health score should not be penalized for beta duplication"
    );
    assert!(
        beta["health_score"]["penalties"]["duplication"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "beta health score should include its duplicate-code penalty"
    );

    // Top-level vital_signs / health_score remain monorepo-wide so consumers
    // that ignore grouping still see the project headline.
    assert!(
        json["vital_signs"].is_object(),
        "top-level vital_signs must remain populated alongside groups"
    );
    assert!(
        json["health_score"].is_object(),
        "top-level health_score must remain populated alongside groups"
    );
}

#[test]
fn health_group_by_package_tags_sarif_results_with_group() {
    // Regression: ship per-finding `properties.group` on SARIF (and the
    // top-level `group` field on CodeClimate) so CI surfaces like GitHub
    // Code Scanning and GitLab Code Quality can partition findings per
    // workspace package without dropping out of the SARIF/CodeClimate
    // pipeline. Companion to the JSON envelope work in #184.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-grouped-sarif",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join("packages/alpha/package.json"),
        r#"{ "name": "alpha", "main": "src/index.ts" }"#,
    );
    // Functions branchy enough to exceed the very-low cyclomatic threshold below.
    write_file(
        &root.join("packages/alpha/src/index.ts"),
        r"export const branchy = (n: number): number => {
  if (n > 0) return 1;
  if (n < 0) return -1;
  if (n === 42) return 42;
  return 0;
};
",
    );
    write_file(
        &root.join("packages/beta/package.json"),
        r#"{ "name": "beta", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/beta/src/index.ts"),
        r"export const branchy = (n: number): number => {
  if (n > 0) return 1;
  if (n < 0) return -1;
  if (n === 42) return 42;
  return 0;
};
",
    );

    let sarif = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--group-by",
            "package",
            "--format",
            "sarif",
            "--quiet",
        ],
    );
    let sarif_json = parse_json(&sarif);
    let runs = sarif_json["runs"]
        .as_array()
        .expect("SARIF runs should be an array");
    let mut sarif_groups: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    let mut sarif_results = 0usize;
    for run in runs {
        if let Some(results) = run["results"].as_array() {
            for r in results {
                sarif_results += 1;
                if let Some(g) = r["properties"]["group"].as_str() {
                    sarif_groups.insert(g.to_owned());
                }
            }
        }
    }
    assert!(
        sarif_results > 0,
        "SARIF should contain at least one result"
    );
    assert!(
        sarif_groups.contains("alpha") && sarif_groups.contains("beta"),
        "SARIF results should tag alpha and beta groups: {sarif_groups:?}"
    );

    let cc = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--group-by",
            "package",
            "--format",
            "codeclimate",
            "--quiet",
        ],
    );
    let cc_json = parse_json(&cc);
    let issues = cc_json
        .as_array()
        .expect("CodeClimate output should be an array");
    let mut cc_groups: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for issue in issues {
        if let Some(g) = issue["group"].as_str() {
            cc_groups.insert(g.to_owned());
        }
    }
    assert!(
        !issues.is_empty(),
        "CodeClimate should emit at least one issue"
    );
    assert!(
        cc_groups.contains("alpha") && cc_groups.contains("beta"),
        "CodeClimate issues should tag alpha and beta groups: {cc_groups:?}"
    );
}

#[test]
fn health_group_by_non_monorepo_emits_single_json_error() {
    // Regression: panel review caught that `--group-by package --format json`
    // on a non-monorepo emitted TWO top-level JSON objects (the hotspot-needs-git
    // error + the group-by-needs-monorepo error), producing invalid JSON for any
    // pipeline doing `jq .`. Resolver validation now runs upfront so misconfig
    // fails before the rest of the pipeline.
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{ "name": "single", "main": "src/index.ts" }"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");

    let output = common::run_fallow_in_root(
        "health",
        root,
        &["--group-by", "package", "--format", "json", "--quiet"],
    );
    assert_ne!(
        output.code, 0,
        "non-monorepo --group-by package should fail"
    );

    // Critical: stdout must be exactly ONE JSON object, parseable by `jq .`.
    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be a single valid JSON object");
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
fn health_coverage_gaps_changed_since_scopes_results() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    copy_dir_recursive(&fixture_path("coverage-gaps"), root);

    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    write_file(
        &root.join("src/fixture-only.ts"),
        r#"export function viaFixture(): string {
  return "fixture-only-updated";
}
"#,
    );
    git(root, &["add", "src/fixture-only.ts"]);
    git(root, &["commit", "-m", "update fixture gap"]);

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--coverage-gaps",
            "--changed-since",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "changed-since coverage gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("changed-since coverage_gaps should be an object");

    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert_eq!(
        file_paths.len(),
        1,
        "changed-since should limit file gaps to changed files: {file_paths:?}"
    );
    assert!(
        file_paths[0].ends_with("src/fixture-only.ts"),
        "changed-since should report the changed fixture-only file, got: {file_paths:?}"
    );

    let summary = coverage["summary"]
        .as_object()
        .expect("coverage_gaps.summary should be an object");
    assert_eq!(
        summary["runtime_files"].as_u64(),
        Some(1),
        "changed-since should recompute runtime scope summary for changed files only"
    );
}

// ---------------------------------------------------------------------------
// Human output snapshot
// ---------------------------------------------------------------------------

#[test]
fn health_human_output_snapshot() {
    // Use --max-cyclomatic 10 so the 14-branch classify() function exceeds the threshold
    // and produces actual output to snapshot (default threshold of 20 would show nothing)
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--complexity", "--max-cyclomatic", "10", "--quiet"],
    );
    let root = fixture_path("complexity-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("health_human_complexity", redacted);
}

// ---------------------------------------------------------------------------
// Plugin-scoped hidden directory traversal
// ---------------------------------------------------------------------------

#[test]
fn health_file_scores_include_plugin_scoped_hidden_dirs_for_react_router() {
    // `fallow health --file-scores` must analyze React Router's `.client` /
    // `.server` convention folders; otherwise its file-level metrics ignore a
    // real chunk of the project.
    let output = run_fallow(
        "health",
        "react-router-conventions",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr was: {}", output.stderr);

    let json = parse_json(&output);
    let files_analyzed = json["summary"]["files_analyzed"]
        .as_u64()
        .expect("files_analyzed is a number");
    assert!(
        files_analyzed >= 5,
        "expected files_analyzed >= 5 (root + routes + .client + .server), got {files_analyzed}"
    );

    let scored_paths: Vec<&str> = json["file_scores"]
        .as_array()
        .expect("file_scores array")
        .iter()
        .filter_map(|fs| fs["path"].as_str())
        .collect();
    assert!(
        scored_paths.contains(&"app/.client/analytics.ts"),
        "expected app/.client/analytics.ts in file_scores: {scored_paths:?}"
    );
}
