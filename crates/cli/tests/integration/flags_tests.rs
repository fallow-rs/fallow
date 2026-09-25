#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{git, git_command, run_fallow, run_fallow_combined, run_fallow_in_root};

#[test]
fn feature_flag_suppression_next_line() {
    let out = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--no-cache", "--format", "json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("valid JSON from flags command");

    let flags = json["feature_flags"]
        .as_array()
        .expect("feature_flags array");

    let flag_names: Vec<&str> = flags
        .iter()
        .filter_map(|f| f["flag_name"].as_str())
        .collect();

    assert!(
        !flag_names.contains(&"FEATURE_DARK_MODE"),
        "FEATURE_DARK_MODE should be suppressed via // fallow-ignore-next-line feature-flag, found: {flag_names:?}"
    );
    assert!(
        flag_names.contains(&"FEATURE_NEW_CHECKOUT"),
        "FEATURE_NEW_CHECKOUT should still be reported (not suppressed), found: {flag_names:?}"
    );
}

#[test]
fn feature_flag_suppression_file_wide() {
    let out = run_fallow(
        "flags",
        "feature-flag-suppression",
        &["--no-cache", "--format", "json"],
    );
    let json: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("valid JSON from flags command");

    let total = json["total_flags"]
        .as_u64()
        .expect("total_flags should be a number");

    assert_eq!(
        total, 1,
        "only 1 flag should remain after suppression (FEATURE_DARK_MODE suppressed)"
    );
}

#[test]
fn empty_result_default_config_surfaces_detectors() {
    let out = run_fallow("flags", "flags-none-default", &["--no-cache"]);

    assert_eq!(out.code, 0, "flags exits 0 on no findings");
    assert!(
        out.stderr.contains("No feature flags detected"),
        "stderr should carry the empty-result line: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("Scanned") && out.stderr.contains("for:"),
        "default config should enumerate the detectors scanned: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("FEATURE_*") && out.stderr.contains("TOGGLE_*"),
        "built-in env prefixes should be listed: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("LaunchDarkly") && out.stderr.contains("Vercel Flags"),
        "built-in SDK providers should be listed: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("flags.sdkPatterns"),
        "should point at flags.sdkPatterns: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("flags.configObjectHeuristics"),
        "should point at flags.configObjectHeuristics: {}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("docs.fallow.tools/cli/flags#configuration"),
        "should link the configuration docs: {}",
        out.stderr
    );
}

#[test]
fn empty_result_quiet_suppresses_hint() {
    let out = run_fallow("flags", "flags-none-default", &["--no-cache", "--quiet"]);

    assert_eq!(out.code, 0, "flags exits 0 on no findings");
    assert!(
        !out.stderr.contains("No feature flags detected"),
        "--quiet suppresses the empty-result line: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("Scanned"),
        "--quiet suppresses the detector hint: {}",
        out.stderr
    );
}

#[test]
fn empty_result_custom_config_is_terse() {
    let out = run_fallow("flags", "flags-none-custom", &["--no-cache"]);

    assert_eq!(out.code, 0, "flags exits 0 on no findings");
    assert!(
        out.stderr.contains("No feature flags detected"),
        "stderr should carry the empty-result line: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("with your custom flag config")
            && out.stderr.contains("2 custom SDK patterns")
            && out.stderr.contains("1 custom env prefix"),
        "custom config should get a terse acknowledgement: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("Using a different SDK"),
        "users with custom config should not be nagged with the discovery block: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("LaunchDarkly"),
        "the built-in provider enumeration should be suppressed for custom config: {}",
        out.stderr
    );
}

/// A flags run reports what it skipped, in the envelope, not only on stderr.
///
/// This command's envelope carried no `workspace_diagnostics` key at all, so a
/// skipped, unreadable, or degraded file was unreachable from both channels:
/// several kinds no longer print a stderr warning, and the JSON had nowhere to
/// put them. Each is a reason a flag is missing from `feature_flags[]`.
#[test]
fn flags_json_reports_the_diagnostics_the_run_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"flagproj","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export const checkout = (): boolean => process.env.FEATURE_CHECKOUT === \"1\";\n",
    )
    .unwrap();
    // A file the parser cannot finish: the flags scan still reads it, and what
    // it could not reach is a reason a flag is absent.
    std::fs::write(root.join("src/broken.ts"), "export const oops = ( => {\n").unwrap();

    let output = run_fallow_in_root(
        "flags",
        root,
        &["--format", "json", "--quiet", "--no-cache"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let diagnostics = json["workspace_diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("flags JSON should carry workspace_diagnostics, got {json}"));
    let kinds: Vec<&str> = diagnostics
        .iter()
        .filter_map(|d| d["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"source-parse-degraded"),
        "the unparseable file must be reported, kinds were {kinds:?}"
    );

    let degraded = diagnostics
        .iter()
        .find(|d| d["kind"] == "source-parse-degraded")
        .unwrap();
    assert_eq!(
        degraded["path"], "src/broken.ts",
        "paths are project-relative on this envelope, entry was {degraded}"
    );
}

/// The unconfigured-detector diagnostics reach this envelope too.
///
/// They are recorded by the dead-code analyze pass, and the flag scan runs
/// that pass to correlate flags with dead exports. They stopped printing on
/// stderr in this release, so before the array existed they were unreachable
/// from both channels on `fallow flags` specifically.
#[test]
fn flags_json_carries_the_analysis_stage_diagnostics_its_scan_records() {
    let json: serde_json::Value = serde_json::from_str(
        &run_fallow(
            "flags",
            "feature-flag-suppression",
            &["--no-cache", "--format", "json"],
        )
        .stdout,
    )
    .expect("valid JSON from flags command");

    let kinds: Vec<&str> = json["workspace_diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("flags JSON should carry workspace_diagnostics, got {json}"))
        .iter()
        .filter_map(|d| d["kind"].as_str())
        .collect();

    assert!(
        kinds.contains(&"boundaries-not-configured")
            && kinds.contains(&"rule-packs-not-configured"),
        "the flag scan runs the dead-code pass, so its diagnostics belong here, kinds were {kinds:?}"
    );
}

/// `--fail-on-stale-baseline` is global: every command that accepts
/// `--baseline` accepts it, and it is inert when no baseline is loaded.
#[test]
fn fail_on_stale_baseline_is_a_global_flag() {
    for subcommand in ["dead-code", "check", "dupes", "health", "audit"] {
        let mut args = vec!["--no-cache", "--quiet", "--fail-on-stale-baseline"];
        if subcommand == "audit" {
            // The fixture lives inside this repository, and a detached CI
            // checkout has no detectable base branch; HEAD keeps this an
            // acceptance check rather than a base-detection check.
            args.extend(["--base", "HEAD"]);
        }
        let out = run_fallow(subcommand, "basic-project", &args);
        assert_ne!(
            out.code, 2,
            "{subcommand} must accept --fail-on-stale-baseline: {}",
            out.stderr
        );
    }
    let bare = run_fallow_combined(
        "basic-project",
        &["--no-cache", "--quiet", "--fail-on-stale-baseline"],
    );
    assert_ne!(
        bare.code, 2,
        "the bare run must accept --fail-on-stale-baseline: {}",
        bare.stderr
    );
}

fn retirement_json(args: &[&str]) -> serde_json::Value {
    let mut all = vec!["--no-cache", "--format", "json", "--quiet", "--retirement"];
    all.extend_from_slice(args);
    let out = run_fallow("flags", "flags-retirement", &all);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    serde_json::from_str(&out.stdout).expect("valid JSON from flags --retirement")
}

fn retirement_row<'v>(json: &'v serde_json::Value, name: &str) -> &'v serde_json::Value {
    json["retirement"]["flags"]
        .as_array()
        .expect("retirement.flags array")
        .iter()
        .find(|row| row["flag_name"] == name)
        .unwrap_or_else(|| panic!("no retirement row for {name}: {json}"))
}

fn reasons(row: &serde_json::Value) -> Vec<&str> {
    row["reasons"]
        .as_array()
        .expect("reasons array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

#[test]
fn retirement_groups_sites_into_one_row_per_flag() {
    let json = retirement_json(&[]);
    assert_eq!(
        json["schema_version"], 8,
        "the block moves no schema version"
    );
    assert_eq!(json["retirement"]["summary"]["distinct_flags"], 3);

    let wide = retirement_row(&json, "FEATURE_WIDE");
    assert_eq!(wide["read_sites"], 2);
    assert_eq!(wide["kind"], "environment_variable");
    assert!(reasons(wide).is_empty(), "two production reads: {wide}");
    assert_eq!(wide["actions"].as_array().map(Vec::len), Some(0));

    let single = retirement_row(&json, "FEATURE_SINGLE");
    assert_eq!(reasons(single), vec!["single-read-site"]);
    assert_eq!(single["actions"][0]["type"], "review-retirement");
    assert_eq!(single["actions"][0]["auto_fixable"], false);

    let test_only = retirement_row(&json, "FEATURE_TEST_ONLY");
    assert_eq!(reasons(test_only), vec!["single-read-site", "test-only"]);
    assert_eq!(test_only["sites"][0]["path"], "src/checkout.test.ts");
    assert_eq!(test_only["sites"][0]["in_test"], true);
}

#[test]
fn retirement_leaves_the_rest_of_the_envelope_unchanged() {
    let plain = run_fallow(
        "flags",
        "flags-retirement",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let mut plain: serde_json::Value = serde_json::from_str(&plain.stdout).expect("plain JSON");
    assert!(plain.get("retirement").is_none(), "the block is opt-in");

    // Age diagnostics are part of the report, so this comparison turns age off.
    let mut with = retirement_json(&["--flag-age", "off"]);
    with.as_object_mut().expect("object").remove("retirement");
    for value in [&mut plain, &mut with] {
        let object = value.as_object_mut().expect("object");
        object.remove("elapsed_ms");
        object.remove("_meta");
    }
    assert_eq!(plain, with);
}

#[test]
fn retirement_reason_filter_keeps_matching_rows_only() {
    let json = retirement_json(&["--reason", "test-only"]);
    let names: Vec<&str> = json["retirement"]["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .filter_map(|row| row["flag_name"].as_str())
        .collect();
    assert_eq!(names, vec!["FEATURE_TEST_ONLY"]);
    assert_eq!(
        json["retirement"]["summary"]["distinct_flags"], 3,
        "the summary counts every flag in scope"
    );
}

#[test]
fn retirement_human_output_lists_the_candidates() {
    let out = run_fallow("flags", "flags-retirement", &["--no-cache", "--retirement"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("Retirement candidates (2 of 3 flags)"),
        "stdout: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("FEATURE_TEST_ONLY"),
        "stdout: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("single-read-site, test-only"),
        "stdout: {}",
        out.stdout
    );
}

#[test]
fn retirement_rejects_formats_without_a_retirement_renderer() {
    let out = run_fallow(
        "flags",
        "flags-retirement",
        &["--no-cache", "--retirement", "--format", "sarif"],
    );
    assert_eq!(out.code, 2, "stdout: {} stderr: {}", out.stdout, out.stderr);
}

/// 2023-11-14T22:13:20Z.
const AGE_BASE_EPOCH: u64 = 1_700_000_000;
const SECS_PER_DAY: u64 = 86_400;

fn commit_at(root: &std::path::Path, path: &str, contents: &str, day: u64) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("dirs");
    std::fs::write(&file, contents).expect("write");
    git(root, &["add", path]);
    let stamp = format!("{} +0000", AGE_BASE_EPOCH + day * SECS_PER_DAY);
    let status = git_command(root)
        .env("GIT_AUTHOR_DATE", &stamp)
        .env("GIT_COMMITTER_DATE", &stamp)
        .args([
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            path,
        ])
        .status()
        .expect("git commit");
    assert!(status.success());
}

/// `FEATURE_OLD` lands on day 0 and `FEATURE_NEW` on day 80. HEAD is day 100.
fn aged_flags_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    git(root, &["init", "--quiet", "--initial-branch=main"]);
    commit_at(
        root,
        "package.json",
        r#"{"name":"aged-flags","main":"src/index.ts"}"#,
        0,
    );
    commit_at(
        root,
        "src/old.ts",
        "export const old = (): boolean => Boolean(process.env.FEATURE_OLD);\n",
        0,
    );
    commit_at(
        root,
        "src/new.ts",
        "export const fresh = (): boolean => Boolean(process.env.FEATURE_NEW);\n",
        80,
    );
    commit_at(
        root,
        "src/index.ts",
        "export { old } from './old';\nexport { fresh } from './new';\n",
        100,
    );
    dir
}

fn retirement_in(root: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let mut all = vec!["--no-cache", "--format", "json", "--quiet", "--retirement"];
    all.extend_from_slice(args);
    let out = run_fallow_in_root("flags", root, &all);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    serde_json::from_str(&out.stdout).expect("valid JSON")
}

#[test]
fn retirement_blame_age_counts_days_against_the_head_commit() {
    let repo = aged_flags_repo();
    let json = retirement_in(repo.path(), &[]);
    let report = &json["retirement"];
    assert_eq!(report["age_mode"], "blame");
    assert_eq!(report["generated_at_clock"], "2024-02-22T22:13:20Z");
    let names: Vec<&str> = report["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .filter_map(|row| row["flag_name"].as_str())
        .collect();
    assert_eq!(names, vec!["FEATURE_OLD", "FEATURE_NEW"], "oldest first");
    let old = retirement_row(&json, "FEATURE_OLD");
    assert_eq!(old["age_days"], 100);
    assert_eq!(old["oldest_surviving_site"]["date"], "2023-11-14");
    assert_eq!(old["first_seen"], serde_json::Value::Null);
    assert_eq!(retirement_row(&json, "FEATURE_NEW")["age_days"], 20);
}

#[test]
fn retirement_min_age_keeps_old_flags_only() {
    let repo = aged_flags_repo();
    let json = retirement_in(repo.path(), &["--min-age", "30"]);
    let names: Vec<&str> = json["retirement"]["flags"]
        .as_array()
        .expect("flags")
        .iter()
        .filter_map(|row| row["flag_name"].as_str())
        .collect();
    assert_eq!(names, vec!["FEATURE_OLD"]);
}

#[test]
fn retirement_pickaxe_reads_first_seen() {
    let repo = aged_flags_repo();
    let json = retirement_in(repo.path(), &["--flag-age", "pickaxe"]);
    assert_eq!(json["retirement"]["age_mode"], "pickaxe");
    let old = retirement_row(&json, "FEATURE_OLD");
    assert_eq!(old["first_seen"]["date"], "2023-11-14");
    assert_eq!(old["age_days"], 100);
}

#[test]
fn retirement_age_off_measures_nothing() {
    let repo = aged_flags_repo();
    let json = retirement_in(repo.path(), &["--flag-age", "off"]);
    assert_eq!(json["retirement"]["age_mode"], "off");
    assert_eq!(
        json["retirement"]["generated_at_clock"],
        serde_json::Value::Null
    );
    assert_eq!(
        retirement_row(&json, "FEATURE_OLD")["age_days"],
        serde_json::Value::Null
    );
}

#[test]
fn retirement_outside_a_repository_reports_why_age_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("src")).expect("src");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"no-git","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const on = (): boolean => Boolean(process.env.FEATURE_X);\n",
    )
    .expect("source");
    let json = retirement_in(dir.path(), &[]);
    let has_age_diagnostic = json["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .any(|d| d["kind"] == "flag-age-unavailable");
    assert!(has_age_diagnostic, "{json}");
    assert_eq!(
        retirement_row(&json, "FEATURE_X")["age_days"],
        serde_json::Value::Null
    );
}

#[test]
fn retirement_age_options_need_retirement() {
    let out = run_fallow(
        "flags",
        "flags-retirement",
        &["--no-cache", "--flag-age", "off"],
    );
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
}
