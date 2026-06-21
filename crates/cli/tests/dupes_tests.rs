#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, parse_json, redact_all, run_fallow, run_fallow_combined, run_fallow_in_root,
};
use tempfile::tempdir;

fn init_git_index(root: &std::path::Path) {
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .expect("git init should run");
    assert!(status.success(), "git init should succeed");
    let status = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .expect("git add should run");
    assert!(status.success(), "git add should succeed");
}

fn has_clone_group_with_files(json: &serde_json::Value, expected: &[&str]) -> bool {
    json["clone_groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| {
            let Some(instances) = group["instances"].as_array() else {
                return false;
            };
            expected.iter().all(|file| {
                instances
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some(*file))
            })
        })
}

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
fn combined_dupes_min_tokens_filter() {
    let output = run_fallow_combined(
        "duplicate-code",
        &["--dupes-min-tokens", "1000", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let groups = json["dupes"]["clone_groups"].as_array().unwrap();
    assert!(
        groups.is_empty(),
        "high combined-mode --dupes-min-tokens should filter out all clones"
    );
}

#[test]
fn combined_dupes_accepts_remaining_config_knobs() {
    let output = run_fallow_combined(
        "duplicate-code",
        &[
            "--dupes-min-lines",
            "1",
            "--dupes-skip-local",
            "--dupes-cross-language",
            "--dupes-ignore-imports",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined mode should accept dupes config knobs, got exit code {}. stderr: {}",
        output.code,
        output.stderr
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
fn dupes_filters_atomic_function_call_clones() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/a")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/b")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-call-filter","type":"module","main":"src/a/call.ts"}"#,
    )
    .unwrap();
    let call = r#"export function alpha() {
  return createComplexWidget(
    currentProject.id,
    currentUser.id,
    activeWorkspace.slug,
    selectedEnvironment.name,
    featureFlags.enableAuditTrail,
    permissions.canPublish,
    billingAccount.plan,
    retryPolicy.maxAttempts,
    retryPolicy.backoffMs,
    notifier.email,
    logger.child({ scope: "workflow" }),
    {
      source: "settings",
      reason: "manual-run",
      requestedBy: currentUser.email,
      correlationId: request.id,
      priority: selectedWorkflow.priority,
      tags: selectedWorkflow.tags,
      metadata: selectedWorkflow.metadata,
      createdAt: clock.now(),
    },
  );
}
"#;
    std::fs::write(dir.path().join("src/a/call.ts"), call).unwrap();
    std::fs::write(
        dir.path().join("src/b/call.ts"),
        call.replace("alpha", "beta"),
    )
    .unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        groups.is_empty(),
        "atomic call clones should be filtered. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
    assert_eq!(json["stats"]["clone_groups"], serde_json::json!(0));
}

#[test]
fn dupes_still_reports_repeated_control_flow() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/a")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/b")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-control-flow","type":"module","main":"src/a/flow.ts"}"#,
    )
    .unwrap();
    let flow = r#"export function alpha(value) {
  const normalized = normalizeValue(value);
  const score = calculateScore(normalized);
  if (score > 90) {
    auditTrail.record("high", normalized.id);
    notifications.send("high-score", normalized.owner);
    metrics.increment("score.high");
    return buildResult(normalized, "high", score);
  }
  if (score > 50) {
    auditTrail.record("medium", normalized.id);
    notifications.send("medium-score", normalized.owner);
    metrics.increment("score.medium");
    return buildResult(normalized, "medium", score);
  }
  auditTrail.record("low", normalized.id);
  notifications.send("low-score", normalized.owner);
  metrics.increment("score.low");
  return buildResult(normalized, "low", score);
}
"#;
    std::fs::write(dir.path().join("src/a/flow.ts"), flow).unwrap();
    std::fs::write(
        dir.path().join("src/b/flow.ts"),
        flow.replace("alpha", "beta"),
    )
    .unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        !groups.is_empty(),
        "non-atomic repeated control flow should still be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn dupes_still_reports_repeated_callback_bodies_inside_calls() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/a")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/b")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-callback-body","type":"module","main":"src/a/routes.ts"}"#,
    )
    .unwrap();
    let route = r#"router.get("/alpha", async (ctx) => {
  const normalized = normalizeValue(ctx.input);
  const score = calculateScore(normalized);
  if (score > 90) {
    auditTrail.record("high", normalized.id);
    notifications.send("high-score", normalized.owner);
    metrics.increment("score.high");
    return buildResult(normalized, "high", score);
  }
  if (score > 50) {
    auditTrail.record("medium", normalized.id);
    notifications.send("medium-score", normalized.owner);
    metrics.increment("score.medium");
    return buildResult(normalized, "medium", score);
  }
  auditTrail.record("low", normalized.id);
  notifications.send("low-score", normalized.owner);
  metrics.increment("score.low");
  return buildResult(normalized, "low", score);
});
"#;
    std::fs::write(dir.path().join("src/a/routes.ts"), route).unwrap();
    std::fs::write(
        dir.path().join("src/b/routes.ts"),
        route.replace("/alpha", "/beta"),
    )
    .unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        !groups.is_empty(),
        "callback bodies inside calls should still be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn dupes_reports_web_format_clone_groups() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-web-formats","type":"module","main":"src/main.ts","dependencies":{"astro":"latest","svelte":"latest","vue":"latest"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/main.ts"),
        "export const entry = true;\n",
    )
    .unwrap();

    let css = r".metric-card {
  display: grid;
  gap: var(--space-3);
  padding: clamp(12px, 2vw, 24px);
  border: 1px solid var(--border-muted);
}
.metric-card__title {
  font-weight: 700;
  color: var(--text-strong);
}
";
    std::fs::write(dir.path().join("src/alpha.css"), css).unwrap();
    std::fs::write(dir.path().join("src/beta.css"), css).unwrap();

    let vue = r#"<template>
  <section class="metric-card">
    <header class="metric-card__title">Revenue</header>
    <p class="metric-card__value">{{ value }}</p>
  </section>
</template>
<style>
.metric-card {
  display: grid;
  gap: var(--space-3);
  padding: 16px;
}
</style>
"#;
    std::fs::write(dir.path().join("src/AlphaCard.vue"), vue).unwrap();
    std::fs::write(dir.path().join("src/BetaCard.vue"), vue).unwrap();

    let svelte = r#"<script>
  export let value = 0;
</script>
<section class="metric-card">
  <header class="metric-card__title">Revenue</header>
  <p class="metric-card__value">{value}</p>
</section>
<style>
.metric-card {
  display: grid;
  gap: var(--space-3);
  padding: 16px;
}
</style>
"#;
    std::fs::write(dir.path().join("src/AlphaPanel.svelte"), svelte).unwrap();
    std::fs::write(dir.path().join("src/BetaPanel.svelte"), svelte).unwrap();

    let astro = r#"---
const value = 42;
---
<section class="metric-card">
  <header class="metric-card__title">Revenue</header>
  <p class="metric-card__value">{value}</p>
</section>
<style>
.metric-card {
  display: grid;
  gap: var(--space-3);
  padding: 16px;
}
</style>
"#;
    std::fs::write(dir.path().join("src/AlphaPage.astro"), astro).unwrap();
    std::fs::write(dir.path().join("src/BetaPage.astro"), astro).unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--min-tokens",
            "10",
            "--min-lines",
            "2",
        ],
    );
    let json = parse_json(&output);
    assert!(
        has_clone_group_with_files(&json, &["src/alpha.css", "src/beta.css"]),
        "CSS clone group should be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
    assert!(
        has_clone_group_with_files(&json, &["src/AlphaCard.vue", "src/BetaCard.vue"]),
        "Vue clone group should be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
    assert!(
        has_clone_group_with_files(&json, &["src/AlphaPanel.svelte", "src/BetaPanel.svelte"]),
        "Svelte clone group should be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
    assert!(
        has_clone_group_with_files(&json, &["src/AlphaPage.astro", "src/BetaPage.astro"]),
        "Astro clone group should be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn dupes_does_not_report_cross_format_clone_groups() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-format-namespace","type":"module","main":"src/main.ts"}"#,
    )
    .unwrap();

    let js = "export function alpha() { color: red; margin: 0; padding: 1; }";
    let css = ".alpha { color: red; margin: 0; padding: 1; }";
    std::fs::write(dir.path().join("src/alpha.ts"), js).unwrap();
    std::fs::write(dir.path().join("src/beta.ts"), js).unwrap();
    std::fs::write(dir.path().join("src/alpha.css"), css).unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--min-tokens",
            "5",
            "--min-lines",
            "1",
        ],
    );
    let json = parse_json(&output);
    assert!(
        has_clone_group_with_files(&json, &["src/alpha.ts", "src/beta.ts"]),
        "same-format JS clone should still be reported. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
    assert!(
        !has_clone_group_with_files(&json, &["src/alpha.ts", "src/alpha.css"]),
        "JS and CSS should not form cross-format clone groups. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn dupes_does_not_report_clone_groups_spanning_sfc_sections() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-sfc-boundary","type":"module","main":"src/main.ts","dependencies":{"vue":"latest"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/main.ts"),
        "export const entry = true;\n",
    )
    .unwrap();
    let component = concat!(
        "<script>let n = 1;</script><style>.a",
        "{",
        "b:c",
        "}</style>"
    );
    std::fs::write(dir.path().join("src/Alpha.vue"), component).unwrap();
    std::fs::write(dir.path().join("src/Beta.vue"), component).unwrap();
    init_git_index(dir.path());

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--min-tokens",
            "9",
            "--min-lines",
            "1",
        ],
    );
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(
        groups.is_empty(),
        "clone groups should not span SFC section boundaries. stdout: {} stderr: {}",
        output.stdout,
        output.stderr
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

#[test]
fn dupes_json_paths_are_relative() {
    let output = run_fallow("dupes", "duplicate-code", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let groups = json["clone_groups"].as_array().unwrap();
    assert!(!groups.is_empty(), "fixture should have clone groups");

    for group in groups {
        for instance in group["instances"].as_array().unwrap() {
            let path = instance["file"].as_str().unwrap();
            assert!(
                !path.starts_with('/'),
                "clone group instance path should be relative, got: {path}"
            );
        }
    }

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

#[test]
fn dupes_compact_output_includes_traceable_clone_metadata() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "compact", "--quiet"],
    );
    assert!(
        output.stdout.contains("code-duplication:"),
        "compact dupes output should use the code-duplication issue tag. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(":fingerprint=dup:"),
        "compact dupes output should include traceable clone fingerprints. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains(",tokens=")
            && output.stdout.contains(",lines=")
            && output.stdout.contains(",instances="),
        "compact dupes output should include parseable clone metadata. stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("clone-group-"),
        "compact dupes output should not rely on ordinal-only clone labels. stdout: {}",
        output.stdout
    );
}

#[test]
fn dupes_human_output_snapshot() {
    let output = run_fallow("dupes", "duplicate-code", &["--quiet"]);
    let root = fixture_path("duplicate-code");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("dupes_human_output", redacted);
}

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
