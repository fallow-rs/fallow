#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[cfg(unix)]
use crate::common::run_fallow_raw;
use crate::common::{
    CommandOutput, canonical_report_without_gate_outcomes, fallow_bin, fixture_path, parse_json,
    redact_all, run_fallow, run_fallow_combined, run_fallow_in_root,
};
use std::path::Path;
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
    assert!(groups.iter().all(|group| group["spread"].is_number()));
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
fn dupes_near_reports_gapped_function_similarity() {
    let dir = tempdir().expect("temp dir");
    let shared_prefix = "export function calculate(input: number): number {\n  const doubled = input * 2;\n  const normalized = doubled + input;\n";
    let shared_suffix = "\n  const bounded = Math.max(adjusted, 0);\n  const rounded = Math.round(bounded);\n  const weighted = rounded * normalized;\n  const clamped = Math.min(weighted, 1000);\n  const staged = clamped + doubled;\n  const balanced = staged - input;\n  const projected = balanced * 2;\n  const limited = Math.min(projected, 2000);\n  const restored = limited + normalized;\n  const checked = Math.max(restored, doubled);\n  const combined = checked + bounded;\n  const smoothed = Math.round(combined / 2);\n  const finalized = smoothed + staged;\n  return finalized + normalized;\n}\n";
    let first = format!("{shared_prefix}  const adjusted = normalized + 2;{shared_suffix}");
    let second = format!(
        "{shared_prefix}  const offset = normalized > 10 ? 3 : 2;\n  const adjusted = normalized + offset;{shared_suffix}"
    );
    std::fs::write(dir.path().join("a.ts"), first).expect("write first");
    std::fs::write(dir.path().join("b.ts"), second).expect("write second");

    let output = run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--near",
            "--min-tokens",
            "20",
            "--min-lines",
            "3",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&output);

    assert!(json["clone_groups"].as_array().is_some_and(|groups| {
        groups
            .iter()
            .any(|group| group["similarity"].as_f64().is_some())
    }));
}

#[test]
fn dupes_config_ignored_clone_resurfaces_after_added_copy() {
    let dir = tempdir().expect("temp dir");
    let source = "export function shared(value: number): number {\n  const doubled = value * 2;\n  const shifted = doubled + 3;\n  return shifted * 4;\n}\n";
    std::fs::write(dir.path().join("a.ts"), source).expect("write first");
    std::fs::write(dir.path().join("b.ts"), source).expect("write second");
    let args = [
        "--min-tokens",
        "5",
        "--min-lines",
        "2",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    let initial = parse_json(&run_fallow_in_root("dupes", dir.path(), &args));
    let group = initial["clone_groups"]
        .as_array()
        .and_then(|groups| groups.first())
        .expect("initial clone group");
    let fingerprint = group["fingerprint"].as_str().expect("fingerprint");
    let ignored_key = format!("{fingerprint}:2");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        serde_json::json!({ "duplicates": { "ignoredClones": [ignored_key] } }).to_string(),
    )
    .expect("write config");

    let ignored = parse_json(&run_fallow_in_root("dupes", dir.path(), &args));
    assert!(
        ignored["clone_groups"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(ignored["stats"]["clone_groups_ignored"], 1);

    let human = run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--min-tokens", "5", "--min-lines", "2", "--no-cache"],
    );
    assert!(
        human.stderr.contains(
            "note: hid 1 reviewed clone group\n  (duplicates.ignoredClones: remove a key to review it again)"
        ),
        "the reviewed-clones note keeps the count and the config key together across its two lines. stderr: {}",
        human.stderr
    );
    assert!(human.stderr.contains("No code duplication found"));

    std::fs::write(dir.path().join("c.ts"), source).expect("write added copy");
    let resurfaced = parse_json(&run_fallow_in_root("dupes", dir.path(), &args));
    assert!(
        resurfaced["clone_groups"]
            .as_array()
            .is_some_and(|groups| !groups.is_empty())
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

/// `--top` and `--group-by` are refused together instead of one being dropped.
///
/// The pair used to be accepted and `--top` silently ignored: the run exited 0
/// and reported every clone group after being asked for N, with
/// `clone_groups_omitted` at 0 to confirm nothing had been withheld. A refusal
/// costs one flag and cannot be mistaken for a measurement.
#[test]
fn dupes_refuses_top_together_with_group_by() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &[
            "--top",
            "1",
            "--group-by",
            "directory",
            "--format",
            "json",
            "--quiet",
        ],
    );

    assert_eq!(
        output.code, 2,
        "an unserviceable flag pair is invalid input. stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert_eq!(json["exit_code"], 2);
    let message = json["message"].as_str().unwrap();
    assert!(
        message.contains("--top") && message.contains("--group-by"),
        "the message must name both flags, was: {message}"
    );
    // The reason moved onto `help`: as one sentence the refusal was 279
    // characters that soft-wrapped to four terminal lines and buried the
    // actionable half at the end of the fourth.
    let help = json["help"].as_str().unwrap();
    assert!(
        help.contains("per-bucket stats"),
        "the remedy must say why the pair cannot be served, was: {help}"
    );
    assert!(
        help.contains("run one flag or the other"),
        "the remedy must name the way out, was: {help}"
    );
    assert!(
        message.len() < 60,
        "the actionable half must fit one terminal line, was {} chars: {message}",
        message.len()
    );
    assert!(
        json.get("clone_groups").is_none(),
        "a refused run must not also emit a duplication report"
    );
}

/// The refusal reaches the human surface too, and neither flag alone trips it.
#[test]
fn dupes_accepts_top_and_group_by_on_their_own() {
    let refused = run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--group-by", "directory", "--quiet"],
    );
    assert_eq!(refused.code, 2, "stderr:\n{}", refused.stderr);
    assert!(
        refused
            .stderr
            .contains("--top and --group-by cannot be combined"),
        "stderr should carry the refusal, was: {}",
        refused.stderr
    );

    let grouped = run_fallow(
        "dupes",
        "duplicate-code",
        &["--group-by", "directory", "--format", "json", "--quiet"],
    );
    assert_eq!(grouped.code, 0, "stderr:\n{}", grouped.stderr);
    assert!(
        parse_json(&grouped)["clone_groups"].is_array(),
        "--group-by alone still reports the full grouped run"
    );

    let capped = run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--format", "json", "--quiet"],
    );
    assert_eq!(capped.code, 0, "stderr:\n{}", capped.stderr);
    assert!(
        parse_json(&capped)["clone_groups"]
            .as_array()
            .unwrap()
            .len()
            <= 1,
        "--top alone still caps the rendered groups"
    );
}

/// `--top` narrows the rendered vector, never the measurement.
///
/// The envelope used to mix two scopes in one object: `clone_groups` and
/// `clone_instances` were recomputed from the truncated vector while
/// `files_with_clones` and `duplication_percentage` still described the whole
/// corpus, so a consumer reading all four got numbers that cannot come from the
/// same run. All four now describe the corpus, and the truncation is disclosed
/// through `clone_groups_shown` / `clone_groups_omitted` instead.
#[test]
fn dupes_top_keeps_corpus_stats_and_discloses_the_split() {
    let full = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--quiet"],
    ));
    let limited = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--format", "json", "--quiet"],
    ));

    let corpus_groups = full["stats"]["clone_groups"].as_u64().unwrap();
    assert!(
        corpus_groups > 1,
        "fixture must produce more than one clone group for --top to omit any"
    );

    for field in [
        "clone_groups",
        "clone_families",
        "clone_instances",
        "files_with_clones",
        "duplication_percentage",
    ] {
        assert_eq!(
            limited["stats"][field], full["stats"][field],
            "stats.{field} must describe the measured corpus, not the truncated vector"
        );
    }

    let shown = limited["clone_groups_shown"].as_u64().unwrap();
    let omitted = limited["clone_groups_omitted"].as_u64().unwrap();
    assert_eq!(
        shown,
        limited["clone_groups"].as_array().unwrap().len() as u64,
        "clone_groups_shown must count the groups actually rendered"
    );
    assert!(omitted > 0, "--top 1 must omit the remaining groups");
    assert_eq!(
        shown + omitted,
        corpus_groups,
        "clone_groups_shown + clone_groups_omitted must equal stats.clone_groups"
    );

    assert_eq!(
        full["clone_groups_omitted"].as_u64().unwrap(),
        0,
        "a run without --top omits nothing"
    );
}

/// `--top` narrows the family array too, and that has to be recoverable.
///
/// `--top N` truncates `clone_groups[]` and rebuilds `clone_families[]` from
/// what survives, so the family array collapsed with no corpus-wide counter to
/// compare it against and no shown/omitted pair: a consumer could not recover
/// the true family count by any means. `stats.clone_families` now measures the
/// corpus and the pair discloses the split, exactly as on the group axis.
#[test]
fn dupes_top_discloses_the_family_split_it_truncates() {
    let full = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--quiet"],
    ));
    let limited = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--format", "json", "--quiet"],
    ));

    let corpus_families = full["stats"]["clone_families"].as_u64().unwrap();
    assert!(
        corpus_families > 1,
        "fixture must produce more than one clone family for --top to withhold any"
    );
    assert_eq!(
        corpus_families,
        full["clone_families"].as_array().unwrap().len() as u64,
        "an uncapped run measures exactly the families it carries"
    );
    assert_eq!(
        full["clone_families_omitted"].as_u64().unwrap(),
        0,
        "a run without --top withholds no family"
    );

    let shown = limited["clone_families_shown"].as_u64().unwrap();
    let omitted = limited["clone_families_omitted"].as_u64().unwrap();
    assert_eq!(
        shown,
        limited["clone_families"].as_array().unwrap().len() as u64,
        "clone_families_shown must count the families actually carried"
    );
    assert!(
        omitted > 0,
        "the capped run carries fewer families than the corpus holds"
    );
    assert_eq!(
        shown + omitted,
        corpus_families,
        "clone_families_shown + clone_families_omitted must equal stats.clone_families"
    );
}

/// The default human header must not present a cap as the project total.
///
/// Without `--top` the header named every measured group and a footer named
/// the rest. Under `--top N` both vanished: the header printed `N` as though
/// the project had `N` clone groups, and nothing said otherwise.
#[test]
fn dupes_human_header_names_the_corpus_under_top() {
    let full = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--quiet"],
    ));
    let corpus_groups = full["stats"]["clone_groups"].as_u64().unwrap();
    assert!(
        corpus_groups > 1,
        "fixture must produce more than one clone group for --top to withhold any"
    );

    let capped = run_fallow("dupes", "duplicate-code", &["--top", "1", "--quiet"]);
    assert!(
        capped
            .stdout
            .contains(&format!("Duplicates ({corpus_groups} clone groups)")),
        "the capped header must name the measured corpus: {}",
        capped.stdout
    );
    assert!(
        capped.stdout.contains(&format!(
            "... {} of {corpus_groups} clone groups withheld by a display limit",
            corpus_groups - 1
        )),
        "the capped report must name the groups it withheld: {}",
        capped.stdout
    );
    assert!(
        capped.stdout.contains("clone families withheld by --top"),
        "the capped report must name the families it withheld: {}",
        capped.stdout
    );
    assert!(
        !capped.stdout.contains("the same display limit"),
        "the group and family axes are narrowed by different limits: {}",
        capped.stdout
    );
}

/// `--top 0` renders no clone group, and both human surfaces used to read that
/// empty array as a clean project.
///
/// The run still measured the corpus: `stats.clone_groups` is unchanged by a
/// display cap, so a green "no duplication found" over a non-zero count is the
/// exact false-clean state the shown/omitted split exists to prevent. `dead-code
/// --top 0` already kept its sections, so the two commands disagreed.
#[test]
fn dupes_top_zero_never_reports_a_clean_project() {
    let full = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--quiet"],
    ));
    let corpus_groups = full["stats"]["clone_groups"].as_u64().unwrap();
    assert!(
        corpus_groups > 0,
        "fixture must produce clone groups for --top 0 to withhold"
    );

    let capped = run_fallow("dupes", "duplicate-code", &["--top", "0"]);
    let combined = format!("{}{}", capped.stdout, capped.stderr);
    assert!(
        !combined.contains("No code duplication found"),
        "a fully capped listing must not read as a clean project: {combined}"
    );
    assert!(
        capped
            .stdout
            .contains(&format!("Duplicates ({corpus_groups} clone groups)")),
        "the header must still name the measured corpus: {}",
        capped.stdout
    );
    assert!(
        capped.stdout.contains(&format!(
            "... {corpus_groups} of {corpus_groups} clone groups withheld by a display limit"
        )),
        "the footer must say every measured group was withheld: {}",
        capped.stdout
    );

    let summary = run_fallow("dupes", "duplicate-code", &["--top", "0", "--summary"]);
    let summary_combined = format!("{}{}", summary.stdout, summary.stderr);
    assert!(
        !summary_combined.contains("No duplication found"),
        "the summary block must not read as a clean project either: {summary_combined}"
    );
    assert!(
        summary
            .stdout
            .contains(&format!("Corpus totals: {corpus_groups} clone groups")),
        "the summary must state the corpus it measured: {}",
        summary.stdout
    );
}

/// The human summary block must disclose the same split the envelope does, on
/// both axes.
///
/// `Clone families` and `Clone groups` count the rendered vectors while
/// `Duplicated lines` and `Duplication rate` describe the measured corpus, so
/// under `--top` the four aligned numbers come from two different scopes. The
/// note used to name the group axis only, which left the truncated family
/// count reading as a corpus number sitting right above a notice that names
/// groups.
#[test]
fn dupes_summary_discloses_both_axes_a_display_limit_withheld() {
    let full = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--quiet"],
    ));
    let corpus_groups = full["stats"]["clone_groups"].as_u64().unwrap();
    let corpus_families = full["stats"]["clone_families"].as_u64().unwrap();
    assert!(
        corpus_groups > 1 && corpus_families > 1,
        "fixture must produce more than one clone group and family for --top to withhold any"
    );

    let limited = parse_json(&run_fallow(
        "dupes",
        "duplicate-code",
        &["--top", "1", "--format", "json", "--quiet"],
    ));
    let families_omitted = limited["clone_families_omitted"].as_u64().unwrap();
    assert!(families_omitted > 0, "--top 1 must withhold a family");

    let summary = run_fallow(
        "dupes",
        "duplicate-code",
        &["--summary", "--top", "1", "--quiet"],
    );
    assert!(
        summary
            .stdout
            .contains(&format!("{} more clone group", corpus_groups - 1)),
        "the capped summary must name the withheld groups: {}",
        summary.stdout
    );
    assert!(
        summary
            .stdout
            .contains(&format!("{families_omitted} more clone famil")),
        "the capped summary must name the withheld families: {}",
        summary.stdout
    );
    assert!(
        summary.stdout.contains(&format!(
            "Corpus totals: {corpus_groups} clone groups, {corpus_families} clone families"
        )),
        "the capped summary must say which scope the measured stats describe: {}",
        summary.stdout
    );

    let uncapped = run_fallow("dupes", "duplicate-code", &["--summary", "--quiet"]);
    assert!(
        !uncapped.stdout.contains("withheld"),
        "an uncapped summary withholds nothing and must stay silent: {}",
        uncapped.stdout
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
fn dupes_baseline_survives_line_shift_and_reports_extra_copy() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"dupes-baseline-shift","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let clone = "export function shared(value) {\n  if (value > 1) {\n    return value * 2;\n  }\n  return value + 1;\n}\n";
    std::fs::write(dir.path().join("src/one.ts"), clone).unwrap();
    std::fs::write(dir.path().join("src/two.ts"), clone).unwrap();

    let baseline_path = dir.path().join("dupes-baseline.json");
    let thresholds = ["--min-tokens", "10", "--min-lines", "2"];
    let mut save_args = vec!["--save-baseline", baseline_path.to_str().unwrap()];
    save_args.extend(thresholds);
    save_args.extend(["--format", "json", "--quiet", "--no-cache"]);
    let saved = run_fallow_in_root("dupes", dir.path(), &save_args);
    let saved_json = parse_json(&saved);
    assert!(
        has_clone_group_with_files(&saved_json, &["src/one.ts", "src/two.ts"]),
        "fixture should produce a clone group. stdout: {} stderr: {}",
        saved.stdout,
        saved.stderr
    );

    std::fs::write(
        dir.path().join("src/one.ts"),
        format!("// unrelated new comment\n\n{clone}"),
    )
    .unwrap();

    let mut compare_args = vec!["--baseline", baseline_path.to_str().unwrap()];
    compare_args.extend(thresholds);
    compare_args.extend(["--format", "json", "--quiet", "--no-cache"]);
    let shifted = run_fallow_in_root("dupes", dir.path(), &compare_args);
    let shifted_json = parse_json(&shifted);
    assert!(
        shifted_json["clone_groups"].as_array().unwrap().is_empty(),
        "a line shift must not resurface a baselined clone. stdout: {} stderr: {}",
        shifted.stdout,
        shifted.stderr
    );

    std::fs::write(dir.path().join("src/three.ts"), clone).unwrap();
    let copied = run_fallow_in_root("dupes", dir.path(), &compare_args);
    let copied_json = parse_json(&copied);
    assert!(
        has_clone_group_with_files(&copied_json, &["src/three.ts"]),
        "a fresh copy in a third file must be reported. stdout: {} stderr: {}",
        copied.stdout,
        copied.stderr
    );
}

/// Structurally distinct clone bodies, one per pair, so each duplicated pair
/// forms its own clone group instead of collapsing into a shared family.
const CLONE_BODIES: [&str; 8] = [
    "export function alpha(value) {\n  if (value > 1) {\n    return value * 2;\n  }\n  return value + 1;\n}\n",
    "export function beta(items) {\n  let total = 0;\n  for (const item of items) {\n    total += item.size;\n  }\n  return total;\n}\n",
    "export function gamma(kind) {\n  switch (kind) {\n    case \"a\":\n      return 1;\n    case \"b\":\n      return 2;\n    default:\n      return 0;\n  }\n}\n",
    "export function delta(run) {\n  try {\n    return run();\n  } catch (error) {\n    console.error(error);\n    return null;\n  }\n}\n",
    "export function epsilon(queue) {\n  let seen = 0;\n  while (queue.length > 0) {\n    queue.pop();\n    seen += 1;\n  }\n  return seen;\n}\n",
    "export function zeta(name, size) {\n  const record = {\n    name,\n    size,\n    label: name + size,\n  };\n  return record;\n}\n",
    "export function eta(rows) {\n  return rows\n    .filter((row) => row.active)\n    .map((row) => row.id)\n    .join(\", \");\n}\n",
    "export function theta(mode, fallback) {\n  const chosen = mode === \"wide\" ? \"w\" : mode === \"tall\" ? \"t\" : fallback;\n  const suffix = chosen.length > 1 ? \"!\" : \"?\";\n  return `${chosen}${suffix}`;\n}\n",
];

/// Thresholds small enough that every fixture body registers as a clone.
const DUPES_THRESHOLDS: [&str; 4] = ["--min-tokens", "10", "--min-lines", "2"];

/// Write one duplicated pair. Each file ends with a line the other copy does
/// not share, so the detected clone fragment stops at a statement boundary and
/// stays parsable: a fragment truncated mid-construct tokenizes to nothing and
/// every such group would then share one content fingerprint.
fn write_clone_pair(root: &std::path::Path, index: usize, body: &str) {
    let dir = root.join(format!("src/pair{index}"));
    std::fs::create_dir_all(&dir).expect("create pair directory");
    std::fs::write(
        dir.join("a.ts"),
        format!("{body}export const tailA{index} = {index};\n"),
    )
    .expect("write first copy");
    std::fs::write(
        dir.join("b.ts"),
        format!("{body}export const tailB{index} = {};\n", index + 100),
    )
    .expect("write second copy");
}

fn dupes_baseline_args<'a>(baseline: &'a str, flag: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![flag, baseline];
    args.extend(DUPES_THRESHOLDS);
    args.extend(["--no-cache"]);
    args.extend_from_slice(extra);
    args
}

/// A project whose duplication baseline is saved while `saved` duplicated
/// pairs exist, and whose sources then keep only `remaining` of them, so
/// `saved - remaining` baseline entries match nothing on the next run.
fn rotted_dupes_project(saved: usize, remaining: usize) -> tempfile::TempDir {
    let dir = tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"dupes-baseline-staleness","version":"1.0.0","private":true}"#,
    )
    .expect("write package");
    for (index, body) in CLONE_BODIES.iter().enumerate().take(saved) {
        write_clone_pair(root, index, body);
    }
    let baseline = root.join("dupes-baseline.json");
    let baseline_path = baseline.to_str().expect("temp path is UTF-8").to_owned();
    let save = run_fallow_in_root(
        "dupes",
        root,
        &dupes_baseline_args(
            &baseline_path,
            "--save-baseline",
            &["--format", "json", "--quiet"],
        ),
    );
    let groups = parse_json(&save)["clone_groups"]
        .as_array()
        .map_or(0, Vec::len);
    assert_eq!(
        groups, saved,
        "the saved baseline must hold exactly {saved} clone groups: {} {}",
        save.stdout, save.stderr
    );
    for index in remaining..saved {
        std::fs::remove_dir_all(root.join(format!("src/pair{index}"))).expect("drop a pair");
    }
    dir
}

/// Run `dupes --baseline` against a prepared fixture with extra arguments.
fn run_dupes_with_baseline(root: &std::path::Path, extra: &[&str]) -> crate::common::CommandOutput {
    let baseline = root.join("dupes-baseline.json");
    let baseline_path = baseline.to_str().expect("temp path is UTF-8").to_owned();
    run_fallow_in_root(
        "dupes",
        root,
        &dupes_baseline_args(&baseline_path, "--baseline", extra),
    )
}

#[test]
fn partially_stale_dupes_baseline_warns_on_human_output() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(project.path(), &[]);
    assert!(
        output.stderr.contains(
            "Warning: duplication baseline is partially stale: 3 of 4 entries matched no current clone group"
        ),
        "a mostly rotten duplication baseline must say so on stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("--save-baseline"),
        "the warning must point at the re-save command: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "the warning must not change the exit code: {}",
        output.stderr
    );
}

#[test]
fn dupes_baseline_staleness_is_silent_below_threshold() {
    let project = rotted_dupes_project(5, 4);
    let output = run_dupes_with_baseline(project.path(), &[]);
    assert!(
        output
            .stderr
            .contains("Comparing against duplication baseline"),
        "the baseline still loads: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "one stale entry out of five is below the warning threshold: {}",
        output.stderr
    );
}

#[test]
fn cleaned_dupes_project_does_not_warn_about_a_fully_stale_baseline() {
    let project = rotted_dupes_project(4, 0);
    let output = run_dupes_with_baseline(project.path(), &[]);
    assert!(
        !output.stderr.contains("matched 0 current clone groups"),
        "a run with no clone groups cannot tell rot from success: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "neither staleness branch fires without clone groups to compare: {}",
        output.stderr
    );
}

#[test]
fn dupes_zero_overlap_still_warns_when_the_run_has_clone_groups() {
    let project = rotted_dupes_project(4, 4);
    for (index, body) in CLONE_BODIES.iter().enumerate().skip(4) {
        write_clone_pair(project.path(), index - 4, body);
    }
    let output = run_dupes_with_baseline(project.path(), &[]);
    assert!(
        output.stderr.contains(
            "Warning: duplication baseline has 4 entries but matched 0 current clone groups"
        ),
        "the zero-overlap wording is unchanged: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "the two branches are mutually exclusive: {}",
        output.stderr
    );
}

/// Production mode drops story files before analysis, so the comparison sees a
/// narrowed project and cannot judge a whole-project baseline.
#[test]
fn production_scoped_dupes_run_does_not_warn_about_baseline_staleness() {
    let dir = tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"dupes-production-baseline","version":"1.0.0","private":true}"#,
    )
    .expect("write package");
    for (index, body) in CLONE_BODIES.iter().enumerate().take(2) {
        write_clone_pair(root, index, body);
    }
    for (index, body) in CLONE_BODIES.iter().enumerate().take(4).skip(2) {
        let pair = root.join(format!("src/pair{index}"));
        std::fs::create_dir_all(&pair).expect("create pair directory");
        std::fs::write(
            pair.join("a.ts"),
            format!("{body}export const tailA{index} = {index};\n"),
        )
        .expect("write source copy");
        std::fs::write(
            pair.join("a.stories.ts"),
            format!("{body}export const tailB{index} = {};\n", index + 100),
        )
        .expect("write story copy");
    }
    let baseline = root.join("dupes-baseline.json");
    let baseline_path = baseline.to_str().expect("temp path is UTF-8").to_owned();
    let save = run_fallow_in_root(
        "dupes",
        root,
        &dupes_baseline_args(
            &baseline_path,
            "--save-baseline",
            &["--format", "json", "--quiet"],
        ),
    );
    assert_eq!(
        parse_json(&save)["clone_groups"]
            .as_array()
            .map_or(0, Vec::len),
        4,
        "the fixture must save four clone groups: {} {}",
        save.stdout,
        save.stderr
    );

    let output = run_dupes_with_baseline(root, &["--production"]);
    assert!(
        !output.stderr.contains("partially stale"),
        "production mode drops the story copies, so it cannot judge a whole-project baseline: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current clone groups"),
        "a production run must not advise a re-save either: {}",
        output.stderr
    );
}

/// The channel-accurate guard: `dupes` compares and re-saves the baseline
/// before the workspace, diff and positional scope filters run, so those runs
/// still judge the whole project honestly.
#[test]
fn path_scoped_dupes_run_still_judges_the_baseline() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(project.path(), &["src/pair0"]);
    assert!(
        output.stderr.contains(
            "Warning: duplication baseline is partially stale: 3 of 4 entries matched no current clone group"
        ),
        "the comparison ran on the whole project, so the warning is honest: {}",
        output.stderr
    );
}

#[test]
fn quiet_suppresses_the_dupes_partial_staleness_warning() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(project.path(), &["--quiet"]);
    assert!(
        !output
            .stderr
            .contains("Comparing against duplication baseline"),
        "--quiet suppresses the baseline notice: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "--quiet suppresses the staleness warning too: {}",
        output.stderr
    );
}

#[test]
fn fail_on_stale_baseline_exits_one_on_a_stale_dupes_baseline() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(project.path(), &["--fail-on-stale-baseline"]);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 3 of 4 entries"),
        "the gate names the stale share: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("matched no current clone group"),
        "the gate uses the duplication noun: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 1,
        "the opt-in gate fails the run: {}",
        output.stderr
    );
}

/// The duplication percentage is printed in the report, a stale baseline is
/// not, so the threshold gate must not exit first and leave the gate the user
/// opted into unmentioned.
#[test]
fn dupes_stale_baseline_gate_still_prints_behind_the_threshold_gate() {
    let project = rotted_dupes_project(4, 1);
    // A pair the baseline never saw, so duplication survives the baseline
    // filter and the threshold gate fires alongside the stale-baseline gate.
    write_clone_pair(project.path(), 5, CLONE_BODIES[5]);
    let output = run_dupes_with_baseline(
        project.path(),
        &["--threshold", "1", "--fail-on-stale-baseline"],
    );
    assert!(
        output.stderr.contains("exceeds threshold"),
        "the threshold gate still reports: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 3 of 4 entries"),
        "the baseline gate reports as well: {}",
        output.stderr
    );
    assert_eq!(output.code, 1, "both gates fail the run: {}", output.stderr);
}

/// Production mode is the one narrowing channel `dupes` honours, and a run
/// that stands down says so instead of passing quietly.
#[test]
fn dupes_stale_baseline_gate_says_why_it_stood_down_in_production_mode() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(
        project.path(),
        &["--production", "--fail-on-stale-baseline"],
    );
    assert!(
        output
            .stderr
            .contains("--fail-on-stale-baseline did not run"),
        "the run names the reason the gate stood down: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "production mode drops files before the comparison: {}",
        output.stderr
    );
}

#[test]
fn dupes_stale_baseline_gate_leaves_json_output_unchanged() {
    let project = rotted_dupes_project(4, 1);
    let without = run_dupes_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let with = run_dupes_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--fail-on-stale-baseline"],
    );
    assert_eq!(
        canonical_report_without_gate_outcomes(&without),
        canonical_report_without_gate_outcomes(&with),
        "the gate moves nothing in the report but its own armed-ness"
    );
    assert_eq!(
        parse_json(&without)["baseline_staleness"],
        parse_json(&with)["baseline_staleness"],
        "the staleness object stays flag-independent"
    );
    assert_eq!(without.code, 0, "the run is green without the flag");
    assert_eq!(with.code, 1, "the run fails with the flag");
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

/// Standalone `fallow dupes` must apply the duplication threshold gate.
///
/// Regression for #2009: `run_dupes` rendered through
/// `print_dupes_result_with_grouping`, which returned the renderer's exit code
/// without ever consulting `exceeds_threshold`. Combined mode kept gating
/// (it renders via `print_dupes_result`), so the two entry points disagreed
/// and standalone runs exited 0 at 100% duplication.
#[test]
fn dupes_standalone_exits_one_when_duplication_exceeds_threshold() {
    let output = run_fallow("dupes", "duplicate-code", &["--threshold", "1", "--quiet"]);
    assert_eq!(
        output.code, 1,
        "duplication above the threshold must exit 1. stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("exceeds threshold"),
        "expected a threshold diagnostic on stderr, got: {}",
        output.stderr
    );
}

/// Non-vacuous control for the gate above: the same fixture under a threshold
/// it does not reach must stay green and stay silent. Without this, a fix that
/// made `dupes` unconditionally exit 1 would pass the positive test.
#[test]
fn dupes_standalone_exits_zero_when_duplication_below_threshold() {
    let output = run_fallow("dupes", "duplicate-code", &["--threshold", "99", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "duplication below the threshold must exit 0. stderr: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("exceeds threshold"),
        "no threshold diagnostic expected below the threshold, got: {}",
        output.stderr
    );
}

/// The gate is a property of the run, not of the renderer, so it must fire
/// identically for machine formats. #2009 was reported against `--format json`.
#[test]
fn dupes_standalone_threshold_gate_applies_to_json_format() {
    let output = run_fallow(
        "dupes",
        "duplicate-code",
        &["--threshold", "1", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 1,
        "json format must gate on the threshold too. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["stats"]["duplication_percentage"]
            .as_f64()
            .expect("stats.duplication_percentage is a number")
            > 1.0,
        "fixture should exceed the 1% threshold for this test to mean anything"
    );
}

/// Pins the standalone/combined parity that #2009 broke: both entry points
/// must reach the same verdict for the same threshold.
#[test]
fn dupes_threshold_verdict_matches_between_standalone_and_combined() {
    let standalone = run_fallow("dupes", "duplicate-code", &["--threshold", "1", "--quiet"]);
    let combined = run_fallow_combined(
        "duplicate-code",
        &["--dupes-threshold", "1", "--quiet", "--skip", "health"],
    );
    assert_eq!(
        standalone.code, 1,
        "standalone dupes should gate. stderr: {}",
        standalone.stderr
    );
    assert_eq!(
        combined.code, 1,
        "combined mode should gate. stderr: {}",
        combined.stderr
    );
    assert!(
        combined.stderr.contains("exceeds threshold"),
        "combined mode should keep its threshold diagnostic, got: {}",
        combined.stderr
    );
}

// --- `baseline_staleness` on the duplication envelope (issue #2673) -------
//
// The duplication envelope carried nothing at all about a loaded baseline
// before 3.27.0, so a CI integration reading JSON could not see a rotting
// duplication baseline by any route.

#[test]
fn dupes_json_envelope_carries_baseline_staleness() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let staleness = parse_json(&output)["baseline_staleness"].clone();
    assert_eq!(staleness["baseline_entries"], 4);
    assert_eq!(staleness["matched_entries"], 1);
    assert_eq!(staleness["stale_entries"], 3);
    assert_eq!(staleness["change_scoped"], false);
    assert_eq!(staleness["stale"], true);
    assert_eq!(staleness["warning"], "partial");
    assert_eq!(staleness["gate_trips"], true);
    assert_eq!(
        staleness["moved_entries"], 0,
        "duplication matches clone groups by fingerprint and never follows a move: {}",
        output.stdout
    );
}

#[test]
fn dupes_baseline_staleness_is_absent_without_a_baseline() {
    let project = rotted_dupes_project(4, 1);
    let output = run_fallow_in_root(
        "dupes",
        project.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert!(
        parse_json(&output).get("baseline_staleness").is_none(),
        "a run with no baseline keeps the duplication wire byte-identical: {}",
        output.stdout
    );
}

#[test]
fn dupes_baseline_staleness_agrees_with_the_exit_gate() {
    let project = rotted_dupes_project(4, 1);
    let staleness = parse_json(&run_dupes_with_baseline(
        project.path(),
        &["--format", "json", "--quiet"],
    ))["baseline_staleness"]
        .clone();
    let gated = run_dupes_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--fail-on-stale-baseline"],
    );
    assert_eq!(
        staleness["gate_trips"].as_bool(),
        Some(gated.code == 1),
        "the published boolean and the exit code cannot disagree: {}",
        gated.stderr
    );
}

/// The grouped duplication envelope is the shape an agent reaches through the
/// MCP `find_dupes` tool's `group_by` parameter.
#[test]
fn the_grouped_dupes_envelope_carries_baseline_staleness() {
    let project = rotted_dupes_project(4, 1);
    let output = run_dupes_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--group-by", "directory"],
    );
    let staleness = parse_json(&output)["baseline_staleness"].clone();
    assert_eq!(staleness["baseline_entries"], 4);
    assert_eq!(staleness["gate_trips"], true);
    assert_eq!(staleness["moved_entries"], 0);
}

// --- scoped note routing ---------------------------------------------------
//
// The two scoped notes render from one shared printer, so each mode has to
// carry in the control it honors. `--no-ignore-imports` and
// `--min-occurrences` are declared on the `dupes` subcommand without `global`,
// so bare `fallow` and `fallow audit` answer both with "unexpected argument";
// bare `fallow` honors the `--dupes-` prefixed spellings; `fallow audit`
// parses those and then rebuilds its `DupesOptions` from `DuplicatesConfig`,
// so only the config keys move what audit reports. These tests run the built
// binary because that relationship lives in clap and in audit's options
// builder, not in the note renderer the unit tests reach.

const WIRING_NOTE: &str = "note: module wiring excluded from clones";
const GATE_NOTE: &str = "note: hid 1 clone group below minOccurrences";

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run bare `fallow` (combined mode) against an explicit root. The shared
/// harness only offers a fixture-directory variant, and these tests build their
/// project in a temp dir so the config can be rewritten between runs.
fn run_combined_in_root(root: &Path, args: &[&str]) -> CommandOutput {
    let mut cmd = std::process::Command::new(fallow_bin());
    cmd.arg("--root")
        .arg(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// A project whose duplication run carries both scoped notes at once: `a.ts` /
/// `b.ts` are a pair the `minOccurrences` gate hides, `c.ts` / `d.ts` / `e.ts`
/// are a reported trio so the module-wiring note fires, and `f.ts` / `g.ts` /
/// `h.ts` share only their import block, so that group surfaces exactly when
/// the wiring opt-out is honored.
///
/// Every knob comes from config rather than flags: audit drops the equivalent
/// flags, so config is the one input all three modes read the same way. The
/// sources stay untracked so audit's changed-file scope covers them.
fn write_scoped_note_fixture(root: &Path, min_occurrences: usize, ignore_imports: bool) {
    let config = serde_json::json!({
        "duplicates": {
            "minTokens": 5,
            "minLines": 2,
            "minOccurrences": min_occurrences,
            "ignoreImports": ignore_imports,
        }
    });
    std::fs::write(root.join(".fallowrc.json"), config.to_string()).expect("write config");
    if root.join("package.json").exists() {
        return;
    }
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"scoped-notes","version":"1.0.0"}"#,
    )
    .expect("write manifest");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "base"],
    );

    let pair = "export function shared(value: number): number {\n  const doubled = value * 2;\n  const shifted = doubled + 3;\n  return shifted * 4;\n}\n";
    let trio = "export function trio(value: number): number {\n  const scaled = value * 7;\n  const nudged = scaled + 11;\n  return nudged * 13;\n}\n";
    let wiring = "import { alpha } from \"./lib\";\nimport { beta } from \"./lib\";\nimport { gamma } from \"./lib\";\nimport { delta } from \"./lib\";\n";
    for file in ["a.ts", "b.ts"] {
        std::fs::write(root.join(file), pair).expect("write pair");
    }
    for file in ["c.ts", "d.ts", "e.ts"] {
        std::fs::write(root.join(file), trio).expect("write trio");
    }
    std::fs::write(
        root.join("lib.ts"),
        "export const alpha = 1;\nexport const beta = 2;\nexport const gamma = 3;\nexport const delta = 4;\n",
    )
    .expect("write wiring target");
    for (file, tail) in [
        (
            "f.ts",
            "export const fSum = alpha + beta + gamma + delta;\n",
        ),
        (
            "g.ts",
            "export const gSum = alpha * beta * gamma * delta;\n",
        ),
        (
            "h.ts",
            "export const hSum = alpha - beta - gamma - delta;\n",
        ),
    ] {
        std::fs::write(root.join(file), format!("{wiring}{tail}")).expect("write wiring");
    }
}

/// Both opt-outs took effect: the wiring group and the gated pair are reported
/// and neither note is printed any more.
fn assert_opt_outs_took_effect(output: &CommandOutput) {
    assert!(
        !output.stderr.contains(WIRING_NOTE),
        "the wiring note must stop once wiring is counted. stderr: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("below minOccurrences"),
        "the gate note must stop once the gate is lowered. stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("f.ts"),
        "counting module wiring must surface the wiring clone. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("a.ts"),
        "lowering the gate must surface the pair. stdout: {}",
        output.stdout
    );
}

#[test]
fn dupes_subcommand_notes_route_to_the_subcommand_flags() {
    let dir = tempdir().expect("temp dir");
    write_scoped_note_fixture(dir.path(), 3, true);

    let noted = run_fallow_in_root("dupes", dir.path(), &["--no-cache"]);
    assert!(
        noted.stderr.contains(&format!(
            "{GATE_NOTE}\n  (lower --min-occurrences from 3 to see them)"
        )),
        "stderr: {}",
        noted.stderr
    );
    assert!(
        noted.stderr.contains(&format!(
            "{WIRING_NOTE}\n  (--no-ignore-imports to include it)"
        )),
        "stderr: {}",
        noted.stderr
    );

    assert_opt_outs_took_effect(&run_fallow_in_root(
        "dupes",
        dir.path(),
        &[
            "--no-cache",
            "--no-ignore-imports",
            "--min-occurrences",
            "2",
        ],
    ));
}

#[test]
fn combined_notes_route_to_the_dupes_prefixed_globals() {
    let dir = tempdir().expect("temp dir");
    write_scoped_note_fixture(dir.path(), 3, true);

    let noted = run_combined_in_root(dir.path(), &["--no-cache", "--skip", "health"]);
    assert!(
        noted.stderr.contains(&format!(
            "{GATE_NOTE}\n  (lower --dupes-min-occurrences from 3 to see them)"
        )),
        "stderr: {}",
        noted.stderr
    );
    assert!(
        noted.stderr.contains(&format!(
            "{WIRING_NOTE}\n  (--dupes-no-ignore-imports to include it)"
        )),
        "stderr: {}",
        noted.stderr
    );

    assert_opt_outs_took_effect(&run_combined_in_root(
        dir.path(),
        &[
            "--no-cache",
            "--skip",
            "health",
            "--dupes-no-ignore-imports",
            "--dupes-min-occurrences",
            "2",
        ],
    ));
}

/// A project whose only clone group is gated has NO reported groups, so the
/// note takes the `!has_dupe_groups` branch and audit renders it through
/// `print_audit_min_occurrences_note` rather than the grouping printer. That is
/// the fourth scoped call site, and the only one the three sibling tests above
/// cannot reach.
#[test]
fn audit_routes_the_gate_note_to_the_config_key_when_nothing_is_reported() {
    let dir = tempdir().expect("temp dir");
    write_gated_pair_only_fixture(dir.path(), 3);

    let output = run_fallow_in_root("audit", dir.path(), &["--base", "HEAD", "--no-cache"]);
    assert!(
        output.stderr.contains(&format!(
            "{GATE_NOTE}\n  (lower duplicates.minOccurrences from 3 to see them)"
        )),
        "audit names the key it honors, not a flag it ignores. stderr: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("--dupes-min-occurrences")
            && !output.stderr.contains("--min-occurrences"),
        "naming a flag here would route to a silent no-op. stderr: {}",
        output.stderr
    );
}

/// Only the gated pair, so nothing survives to be reported.
fn write_gated_pair_only_fixture(root: &Path, min_occurrences: usize) {
    let config = serde_json::json!({
        "duplicates": {
            "minTokens": 5,
            "minLines": 2,
            "minOccurrences": min_occurrences,
        }
    });
    std::fs::write(root.join(".fallowrc.json"), config.to_string()).expect("write config");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"gated-only","version":"1.0.0"}"#,
    )
    .expect("write manifest");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "base"],
    );
    let pair = "export function shared(value: number): number {\n  const doubled = value * 2;\n  const shifted = doubled + 3;\n  return shifted * 4;\n}\n";
    for file in ["a.ts", "b.ts"] {
        std::fs::write(root.join(file), pair).expect("write pair");
    }
}

#[test]
fn audit_notes_route_to_the_config_keys_audit_honors() {
    let dir = tempdir().expect("temp dir");
    write_scoped_note_fixture(dir.path(), 3, true);

    let noted = run_fallow_in_root("audit", dir.path(), &["--base", "HEAD", "--no-cache"]);
    assert!(
        noted.stderr.contains(&format!(
            "{GATE_NOTE}\n  (lower duplicates.minOccurrences from 3 to see them)"
        )),
        "stderr: {}",
        noted.stderr
    );
    assert!(
        noted.stderr.contains(&format!(
            "{WIRING_NOTE}\n  (duplicates.ignoreImports: false to include it)"
        )),
        "stderr: {}",
        noted.stderr
    );

    // Audit builds its `DupesOptions` from config alone, so the flags the other
    // two modes honor parse here and change nothing. A note naming them would
    // fail silently, which is why audit names the keys instead.
    let flagged = run_fallow_in_root(
        "audit",
        dir.path(),
        &[
            "--base",
            "HEAD",
            "--no-cache",
            "--dupes-no-ignore-imports",
            "--dupes-min-occurrences",
            "2",
        ],
    );
    assert!(
        flagged.stderr.contains(WIRING_NOTE) && flagged.stderr.contains(GATE_NOTE),
        "the `--dupes-` globals must remain inert under audit. stderr: {}",
        flagged.stderr
    );

    write_scoped_note_fixture(dir.path(), 2, false);
    assert_opt_outs_took_effect(&run_fallow_in_root(
        "audit",
        dir.path(),
        &["--base", "HEAD", "--no-cache"],
    ));
}

#[cfg(unix)]
fn symlink_dupes_fixture() -> tempfile::TempDir {
    let block = |name: &str| {
        (0..4)
            .map(|index| {
                format!(
                    "export function {name}{index}(a: number, b: number): number {{\n  const x = a * {index} + b;\n  const y = x - {index} * b;\n  if (x > y) {{\n    return x + y + {index};\n  }}\n  return x - y - {index};\n}}\n"
                )
            })
            .collect::<Vec<_>>()
            .concat()
    };
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"symlink-dupes"}"#,
    )
    .unwrap();
    std::fs::write(src.join("real.ts"), block("linked")).unwrap();
    std::os::unix::fs::symlink("real.ts", src.join("link.ts")).unwrap();
    std::fs::write(src.join("a.ts"), block("copied")).unwrap();
    std::fs::write(src.join("b.ts"), block("copied")).unwrap();
    std::os::unix::fs::symlink("src", dir.path().join("linked_src")).unwrap();
    dir
}

#[cfg(unix)]
fn clone_family_symlink_files(json: &serde_json::Value) -> Vec<String> {
    let mut files: Vec<String> = json["clone_families"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|family| family["groups"].as_array().unwrap())
        .flat_map(|group| group["instances"].as_array().unwrap())
        .filter(|instance| instance["is_symlink"].as_bool().unwrap_or(false))
        .map(|instance| instance["file"].as_str().unwrap().to_owned())
        .collect();
    files.sort();
    files
}

#[cfg(unix)]
fn clone_group_files(json: &serde_json::Value) -> Vec<Vec<(String, bool)>> {
    let mut groups: Vec<Vec<(String, bool)>> = json["clone_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| {
            let mut files: Vec<(String, bool)> = group["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|instance| {
                    (
                        instance["file"].as_str().unwrap().to_owned(),
                        instance["is_symlink"].as_bool().unwrap_or(false),
                    )
                })
                .collect();
            files.sort();
            files
        })
        .collect();
    groups.sort();
    groups
}

#[cfg(unix)]
#[test]
fn dupes_marks_symlinked_instances_and_ignore_symlinks_omits_them() {
    let dir = symlink_dupes_fixture();
    let real = vec![
        ("src/a.ts".to_owned(), false),
        ("src/b.ts".to_owned(), false),
    ];
    let linked = vec![
        ("src/link.ts".to_owned(), true),
        ("src/real.ts".to_owned(), false),
    ];
    let json_args = ["--format", "json", "--quiet"];

    let default = parse_json(&run_fallow_in_root("dupes", dir.path(), &json_args));
    assert_eq!(
        clone_group_files(&default),
        vec![real.clone(), linked.clone()]
    );
    let real_instance = &default["clone_groups"][0]["instances"][0];
    assert!(
        real_instance.get("is_symlink").is_none(),
        "is_symlink is omitted for a real file: {real_instance}"
    );
    assert_eq!(
        clone_family_symlink_files(&default),
        vec!["src/link.ts".to_owned()],
        "clone families carry the symlink mark"
    );
    assert_eq!(
        default["stats"]["total_files"], 4,
        "discovery does not walk the linked_src directory symlink"
    );

    let sarif = run_fallow_in_root("dupes", dir.path(), &["--format", "sarif", "--quiet"]);
    assert!(
        sarif.stdout.contains("\"uri\": \"src/link.ts\"") && !sarif.stdout.contains("linked_src/"),
        "SARIF reports the symlinked instance as a location: {}",
        sarif.stdout
    );
    let codeclimate =
        run_fallow_in_root("dupes", dir.path(), &["--format", "codeclimate", "--quiet"]);
    assert!(
        codeclimate.stdout.contains("\"path\": \"src/link.ts\"")
            && !codeclimate.stdout.contains("linked_src/"),
        "CodeClimate reports the symlinked instance as a location: {}",
        codeclimate.stdout
    );

    let human = run_fallow_in_root("dupes", dir.path(), &["--quiet"]);
    assert!(
        human
            .stdout
            .lines()
            .any(|line| line.contains("src/link.ts:") && line.ends_with(" (symlink)"))
            && !human.stdout.contains("real.ts:1-31 (symlink)"),
        "human output marks the symlinked instance: {}",
        human.stdout
    );
    let compact = run_fallow_in_root("dupes", dir.path(), &["--format", "compact", "--quiet"]);
    assert!(
        compact
            .stdout
            .lines()
            .any(|line| line.starts_with("code-duplication:src/link.ts:")
                && line.ends_with(",symlink=true")),
        "compact output marks the symlinked instance: {}",
        compact.stdout
    );

    let mut ignore_args = json_args.to_vec();
    ignore_args.push("--ignore-symlinks");
    let ignored = parse_json(&run_fallow_in_root("dupes", dir.path(), &ignore_args));
    assert_eq!(clone_group_files(&ignored), vec![real.clone()]);
    assert!(clone_family_symlink_files(&ignored).is_empty());
    assert_eq!(
        ignored["stats"]["total_files"], 3,
        "symlinked files leave the corpus statistics"
    );

    let root = dir.path().to_str().unwrap();
    let combined = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--only",
        "dupes",
        "--format",
        "json",
        "--quiet",
        "--dupes-ignore-symlinks",
    ]));
    assert_eq!(clone_group_files(&combined["dupes"]), vec![real.clone()]);

    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"duplicates":{"ignoreSymlinks":true}}"#,
    )
    .unwrap();
    let from_config = parse_json(&run_fallow_in_root("dupes", dir.path(), &json_args));
    assert_eq!(clone_group_files(&from_config), vec![real.clone()]);

    let mut opt_out_args = json_args.to_vec();
    opt_out_args.push("--no-ignore-symlinks");
    let opted_out = parse_json(&run_fallow_in_root("dupes", dir.path(), &opt_out_args));
    assert_eq!(clone_group_files(&opted_out), vec![real, linked]);
}
