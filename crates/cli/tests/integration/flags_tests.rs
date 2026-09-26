#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{
    fixture_path, git, git_command, run_fallow, run_fallow_combined, run_fallow_in_root,
};

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
    assert_eq!(json["retirement"]["summary"]["distinct_flags"], 8);
    assert!(
        json["feature_flags"]
            .as_array()
            .expect("feature_flags")
            .iter()
            .all(|flag| flag["flag_name"] != "FEATURE_KILL_SWITCH"),
        "a const flag is not a per-site finding"
    );

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

    let empty = retirement_row(&json, "FEATURE_EMPTY_ARM");
    assert_eq!(reasons(empty), vec!["single-read-site", "empty-branch"]);
    let same = retirement_row(&json, "FEATURE_SAME");
    assert_eq!(
        reasons(same),
        vec!["single-read-site", "identical-branches"]
    );
    assert_eq!(same["evidence"][1]["path"], "src/branches.tsx");
    assert_eq!(same["evidence"][1]["line"], 6);

    let constant = retirement_row(&json, "FEATURE_KILL_SWITCH");
    assert_eq!(constant["kind"], "constant");
    assert_eq!(
        reasons(constant),
        vec!["single-read-site", "literal-constant"]
    );
    assert_eq!(constant["sites"][0]["role"], "definition");
    assert_eq!(constant["sites"][1]["role"], "read");
    assert_eq!(
        constant["evidence"][1]["detail"],
        "const FEATURE_KILL_SWITCH = false"
    );

    let legacy = retirement_row(&json, "legacy-banner");
    assert_eq!(reasons(legacy), vec!["defined-never-read"]);
    assert_eq!(legacy["read_sites"], 0);
    assert_eq!(legacy["sites"][0]["role"], "definition");
    let sale = retirement_row(&json, "summer-sale");
    assert!(
        reasons(sale).is_empty(),
        "an imported definition is read: {sale}"
    );
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
        json["retirement"]["summary"]["distinct_flags"], 8,
        "the summary counts every flag in scope"
    );
}

#[test]
fn retirement_human_output_lists_the_candidates() {
    let out = run_fallow("flags", "flags-retirement", &["--no-cache", "--retirement"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("Retirement candidates (6 of 8 flags)"),
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

/// The text of the human "Retirement candidates" section.
fn retirement_section(args: &[&str]) -> String {
    let mut all = vec!["--no-cache", "--retirement"];
    all.extend_from_slice(args);
    let out = run_fallow("flags", "flags-retirement", &all);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let start = out
        .stdout
        .find("Retirement candidates")
        .unwrap_or_else(|| panic!("no retirement section: {}", out.stdout));
    out.stdout[start..].to_string()
}

#[test]
fn retirement_human_top_counts_candidates_only() {
    // By name, the first 6 rows hold 5 candidates and FEATURE_WIDE, which
    // has no reason. The sixth candidate, legacy-banner, comes after it.
    let section = retirement_section(&["--sort", "name", "--flag-age", "off", "--top", "6"]);
    assert!(
        section.contains("Retirement candidates (6 of 8 flags)"),
        "{section}"
    );
    assert!(section.contains("legacy-banner"), "{section}");
    assert!(!section.contains("FEATURE_WIDE"), "{section}");

    let section = retirement_section(&["--sort", "name", "--flag-age", "off", "--top", "2"]);
    assert!(
        section.contains("Retirement candidates (6 of 8 flags)"),
        "{section}"
    );
    assert!(section.contains("FEATURE_KILL_SWITCH"), "{section}");
    assert!(!section.contains("FEATURE_SAME"), "{section}");
    assert!(
        section.contains("Showing 2 of 6 candidates (--top 2)."),
        "{section}"
    );
}

#[test]
fn retirement_human_empty_state_names_the_active_filters() {
    let section = retirement_section(&["--reason", "test-only", "--min-age", "100000"]);
    assert!(
        section.contains("No retirement candidate matches --reason and --min-age."),
        "{section}"
    );
    assert!(
        !section.contains("No flag has a retirement reason"),
        "{section}"
    );
}

#[test]
fn retirement_min_age_needs_a_flag_age() {
    let out = run_fallow(
        "flags",
        "flags-retirement",
        &[
            "--no-cache",
            "--retirement",
            "--flag-age",
            "off",
            "--min-age",
            "1",
        ],
    );
    assert_eq!(out.code, 2, "stdout: {} stderr: {}", out.stdout, out.stderr);
    assert!(out.stderr.contains("--min-age"), "stderr: {}", out.stderr);
}

/// 2026-09-25T00:00:00Z: pins `export_age_days` in the snapshots.
const VENDOR_CLOCK_EPOCH: &str = "1790294400";

fn vendor_format(format: &str) -> String {
    let state = vendor_state_path();
    let root = fixture_path("flags-vendor");
    let out = crate::common::run_fallow_raw_with_env(
        &[
            "flags",
            "--root",
            root.to_str().expect("utf-8 path"),
            "--no-cache",
            "--quiet",
            "--retirement",
            "--flag-age",
            "off",
            "--flag-state",
            state.as_str(),
            "--format",
            format,
        ],
        &[("FALLOW_CLOCK_EPOCH", VENDOR_CLOCK_EPOCH)],
    );
    assert_eq!(out.code, 0, "stdout: {} stderr: {}", out.stdout, out.stderr);
    out.stdout
}

#[test]
fn retirement_compact_prints_one_line_per_reason() {
    let stdout = vendor_format("compact");
    assert!(
        stdout.contains("feature-flag-sdk:src/index.ts:10:beta-typo"),
        "the per-site lines stay: {stdout}"
    );
    for line in [
        "flag-retire:missing-in-vendor:src/index.ts:10:beta-typo",
        "flag-retire:fully-rolled-out:src/checkout.ts:2:new-checkout",
        "flag-retire:vendor-only:flag-state.json:15:removed-long-ago",
    ] {
        assert!(stdout.lines().any(|l| l == line), "{line} in {stdout}");
    }
}

#[test]
fn retirement_sarif_adds_one_note_per_candidate() {
    let sarif: serde_json::Value =
        serde_json::from_str(&vendor_format("sarif")).expect("SARIF JSON");
    let run = &sarif["runs"][0];
    let rule_ids: Vec<&str> = run["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .filter_map(|rule| rule["id"].as_str())
        .collect();
    assert_eq!(
        rule_ids,
        vec!["fallow/feature-flag", "fallow/flag-retirement-candidate"]
    );
    let candidates: Vec<&serde_json::Value> = run["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|result| result["ruleId"] == "fallow/flag-retirement-candidate")
        .collect();
    assert_eq!(
        candidates.len(),
        6,
        "every flag in the fixture is a candidate"
    );
    assert!(candidates.iter().all(|result| result["level"] == "note"));
}

#[test]
fn retirement_codeclimate_adds_one_issue_per_reason() {
    let issues: serde_json::Value =
        serde_json::from_str(&vendor_format("codeclimate")).expect("CodeClimate JSON");
    let retirement: Vec<&serde_json::Value> = issues
        .as_array()
        .expect("issues")
        .iter()
        .filter(|issue| issue["check_name"] == "fallow/flag-retirement")
        .collect();
    assert_eq!(retirement.len(), 8, "one issue per reason: {issues}");
    let mut fingerprints: Vec<&str> = retirement
        .iter()
        .filter_map(|issue| issue["fingerprint"].as_str())
        .collect();
    fingerprints.sort_unstable();
    fingerprints.dedup();
    assert_eq!(fingerprints.len(), 8, "fingerprints are unique");
}

#[test]
fn retirement_markdown_adds_the_candidate_table() {
    let stdout = vendor_format("markdown");
    assert!(stdout.contains("### Feature flags (6)"), "{stdout}");
    assert!(
        stdout.contains("### Retirement candidates (6 of 6 flags)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("| `beta-typo` | - | 1 | single-read-site, missing-in-vendor |"),
        "{stdout}"
    );
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

    let human = run_fallow_in_root("flags", dir.path(), &["--no-cache", "--retirement"]);
    assert_eq!(human.code, 0, "stderr: {}", human.stderr);
    assert!(human.stdout.contains("FEATURE_X"), "{}", human.stdout);
    assert!(
        !human.stdout.contains("Age is a lower bound"),
        "no row has an age: {}",
        human.stdout
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

fn vendor_state_path() -> String {
    fixture_path("flags-vendor")
        .join("flag-state.json")
        .to_string_lossy()
        .into_owned()
}

fn vendor_json(args: &[&str]) -> serde_json::Value {
    let state = vendor_state_path();
    let mut all = vec![
        "--no-cache",
        "--format",
        "json",
        "--quiet",
        "--retirement",
        "--flag-age",
        "off",
        "--flag-state",
        state.as_str(),
    ];
    all.extend_from_slice(args);
    let out = run_fallow("flags", "flags-vendor", &all);
    assert_eq!(out.code, 0, "stdout: {} stderr: {}", out.stdout, out.stderr);
    serde_json::from_str(&out.stdout).expect("valid JSON")
}

#[test]
fn flag_state_gives_the_exact_vendor_reasons() {
    let json = vendor_json(&[]);
    let report = &json["retirement"];
    assert_eq!(report["vendor_state"]["source"], "launchdarkly");
    assert_eq!(
        report["vendor_state"]["exported_at"],
        "2026-09-20T00:00:00Z"
    );
    assert_eq!(report["vendor_state"]["flags"], 4);

    let rolled = retirement_row(&json, "new-checkout");
    assert_eq!(reasons(rolled), vec!["fully-rolled-out"]);
    assert_eq!(rolled["vendor"]["key"], "web.new-checkout");
    assert_eq!(rolled["vendor"]["state"], "rolled_out");
    assert_eq!(
        rolled["evidence"][0]["detail"],
        "launchdarkly state rolled_out, serves one variation"
    );
    assert_eq!(rolled["evidence"][0]["path"], "src/checkout.ts");

    let archived = retirement_row(&json, "old-banner");
    assert_eq!(
        reasons(archived),
        vec!["single-read-site", "archived-in-vendor"]
    );
    let typo = retirement_row(&json, "beta-typo");
    assert_eq!(reasons(typo), vec!["single-read-site", "missing-in-vendor"]);
    assert!(typo.get("vendor").is_none(), "{typo}");

    let live = retirement_row(&json, "live-experiment");
    assert_eq!(reasons(live), vec!["single-read-site"]);
    assert_eq!(live["vendor"]["state"], "experiment");

    let gate = retirement_row(&json, "statsig-gate");
    assert_eq!(
        reasons(gate),
        vec!["single-read-site"],
        "a launchdarkly export does not judge a Statsig flag"
    );

    let orphan = retirement_row(&json, "removed-long-ago");
    assert_eq!(orphan["kind"], "vendor_export");
    assert_eq!(reasons(orphan), vec!["vendor-only"]);
    assert_eq!(orphan["sites"].as_array().map(Vec::len), Some(0));
    assert_eq!(orphan["evidence"][0]["path"], "flag-state.json");
    assert_eq!(orphan["evidence"][0]["line"], 15);
    assert_eq!(report["summary"]["by_reason"]["vendor-only"], 1);
}

#[test]
fn flag_state_reason_filter_accepts_the_vendor_codes() {
    let json = vendor_json(&["--reason", "vendor-only", "--reason", "missing-in-vendor"]);
    let mut names: Vec<&str> = json["retirement"]["flags"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|row| row["flag_name"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["beta-typo", "removed-long-ago"]);
}

#[test]
fn flag_state_needs_retirement() {
    let state = vendor_state_path();
    let out = run_fallow(
        "flags",
        "flags-vendor",
        &["--no-cache", "--flag-state", state.as_str()],
    );
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
}

#[test]
fn a_malformed_flag_state_exits_2_with_an_error_code() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join("state.json");
    std::fs::write(
        &state,
        r#"{"schema_version":1,"source":"x","exported_at":"2026-01-01","flags":[{"key":"a","state":"paused"}]}"#,
    )
    .expect("write");
    let out = run_fallow(
        "flags",
        "flags-vendor",
        &[
            "--no-cache",
            "--format",
            "json",
            "--retirement",
            "--flag-state",
            state.to_str().expect("utf-8 path"),
        ],
    );
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
    let error: serde_json::Value = serde_json::from_str(&out.stdout).expect("JSON error");
    assert_eq!(error["error"], true, "{error}");
    assert_eq!(error["code"], "FALLOW_FLAG_STATE_INVALID", "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("unknown variant")),
        "{error}"
    );
    assert!(error["help"].as_str().is_some(), "{error}");
}

#[test]
fn a_narrowed_run_adds_no_vendor_only_rows() {
    let json = vendor_json(&["--changed-since", "HEAD"]);
    let has_vendor_only = json["retirement"]["flags"]
        .as_array()
        .expect("rows")
        .iter()
        .any(|row| row["kind"] == "vendor_export");
    assert!(!has_vendor_only, "{json}");
}

#[test]
fn an_old_flag_state_export_gets_a_warning() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join("state.json");
    std::fs::write(
        &state,
        r#"{"schema_version":1,"source":"launchdarkly","exported_at":"2020-01-01","flags":[]}"#,
    )
    .expect("write");
    let out = crate::common::run_fallow_raw_with_env(
        &[
            "flags",
            "--root",
            fixture_path("flags-vendor").to_str().expect("utf-8 path"),
            "--no-cache",
            "--retirement",
            "--flag-age",
            "off",
            "--flag-state",
            state.to_str().expect("utf-8 path"),
        ],
        &[("FALLOW_CLOCK_EPOCH", VENDOR_CLOCK_EPOCH)],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr
            .contains("the launchdarkly flag state export is 2459 days old"),
        "stderr: {}",
        out.stderr
    );
}

/// A project with the env flags `FEATURE_A` and `FEATURE_B`. It is outside a
/// git repository, so the gate tests measure no age.
fn gate_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("src")).expect("src");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"gate","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    write_gate_flags(dir.path(), &["FEATURE_A", "FEATURE_B"]);
    dir
}

fn write_gate_flags(root: &std::path::Path, names: &[&str]) {
    let mut body = String::new();
    for (i, name) in names.iter().enumerate() {
        use std::fmt::Write as _;
        let _ = writeln!(
            body,
            "export const f{i} = (): boolean => Boolean(process.env.{name});"
        );
    }
    std::fs::write(root.join("src/index.ts"), body).expect("source");
}

fn gate_run(root: &std::path::Path, args: &[&str]) -> crate::common::CommandOutput {
    let mut all = vec!["--no-cache", "--flag-age", "off"];
    all.extend_from_slice(args);
    run_fallow_in_root("flags", root, &all)
}

#[test]
fn regression_gate_fails_when_a_flag_is_added() {
    let project = gate_project();
    let baseline = project.path().join("flags-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf-8 path");
    let saved = gate_run(
        project.path(),
        &["--retirement", "--save-regression-baseline", baseline_arg],
    );
    assert_eq!(saved.code, 0, "stderr: {}", saved.stderr);
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&baseline).expect("baseline"))
            .expect("baseline JSON");
    assert_eq!(stored["flags"]["distinct_flags"], 2, "{stored}");
    assert_eq!(stored["flags"]["total_flags"], 2, "{stored}");

    let gate = [
        "--retirement",
        "--fail-on-regression",
        "--regression-baseline",
        baseline_arg,
    ];

    write_gate_flags(project.path(), &["FEATURE_A", "FEATURE_B", "FEATURE_C"]);
    let added = gate_run(project.path(), &gate);
    assert_eq!(added.code, 1, "stderr: {}", added.stderr);
    assert!(
        added.stderr.contains("Flags regression detected"),
        "stderr: {}",
        added.stderr
    );

    let mut json_args = gate.to_vec();
    json_args.extend_from_slice(&["--format", "json", "--quiet"]);
    let json_out = gate_run(project.path(), &json_args);
    assert_eq!(json_out.code, 1, "stderr: {}", json_out.stderr);
    let json: serde_json::Value = serde_json::from_str(&json_out.stdout).expect("JSON");
    let regression = &json["retirement"]["regression"];
    assert_eq!(regression["status"], "exceeded", "{regression}");
    assert_eq!(regression["metrics"][0]["metric"], "distinct_flags");
    assert_eq!(regression["metrics"][0]["delta"], 1);

    let mut tolerant = gate.to_vec();
    tolerant.extend_from_slice(&["--tolerance", "1"]);
    let tolerated = gate_run(project.path(), &tolerant);
    assert_eq!(tolerated.code, 0, "stderr: {}", tolerated.stderr);

    write_gate_flags(project.path(), &["FEATURE_A", "FEATURE_C"]);
    let swapped = gate_run(project.path(), &gate);
    assert_eq!(
        swapped.code, 0,
        "one flag added and one retired: {}",
        swapped.stderr
    );
}

#[test]
fn regression_options_without_retirement_warn_and_pass() {
    let project = gate_project();
    let out = gate_run_plain(project.path(), &["--fail-on-regression"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(
        out.stderr
            .contains("--fail-on-regression has no effect on fallow flags without --retirement"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        !project.path().join(".fallowrc.json").exists(),
        "no baseline is written"
    );
}

fn gate_run_plain(root: &std::path::Path, args: &[&str]) -> crate::common::CommandOutput {
    let mut all = vec!["--no-cache"];
    all.extend_from_slice(args);
    run_fallow_in_root("flags", root, &all)
}

#[test]
fn regression_gate_needs_a_baseline_file() {
    let project = gate_project();
    let out = gate_run(project.path(), &["--retirement", "--fail-on-regression"]);
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("--regression-baseline"),
        "{}",
        out.stderr
    );

    let to_config = gate_run(
        project.path(),
        &["--retirement", "--save-regression-baseline"],
    );
    assert_eq!(to_config.code, 2, "stderr: {}", to_config.stderr);
    assert!(
        to_config.stderr.contains("needs a PATH"),
        "{}",
        to_config.stderr
    );
}

#[test]
fn max_flag_age_fails_on_an_old_flag() {
    let repo = aged_flags_repo();
    let old = run_fallow_in_root(
        "flags",
        repo.path(),
        &["--no-cache", "--retirement", "--max-flag-age", "30"],
    );
    assert_eq!(old.code, 1, "stderr: {}", old.stderr);
    assert!(
        old.stderr.contains(
            "Flag age check failed: 1 flag is older than 30 days: FEATURE_OLD (100 days)"
        ),
        "stderr: {}",
        old.stderr
    );

    let json = run_fallow_in_root(
        "flags",
        repo.path(),
        &[
            "--no-cache",
            "--retirement",
            "--max-flag-age",
            "100",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(json.code, 0, "stderr: {}", json.stderr);
    let json: serde_json::Value = serde_json::from_str(&json.stdout).expect("JSON");
    assert_eq!(json["retirement"]["max_flag_age"]["exceeded"], false);
    assert_eq!(json["retirement"]["max_flag_age"]["max_days"], 100);
}

#[test]
fn max_flag_age_needs_a_flag_age() {
    let out = run_fallow(
        "flags",
        "flags-retirement",
        &[
            "--no-cache",
            "--retirement",
            "--flag-age",
            "off",
            "--max-flag-age",
            "30",
        ],
    );
    assert_eq!(out.code, 2, "stderr: {}", out.stderr);
    assert!(out.stderr.contains("--max-flag-age"), "{}", out.stderr);
}

#[test]
fn retirement_output_snapshots_for_every_format() {
    for format in ["human", "compact", "markdown", "codeclimate"] {
        insta::assert_snapshot!(
            format!("flags_retirement_vendor_{format}"),
            vendor_format(format)
        );
    }
    insta::assert_snapshot!(
        "flags_retirement_vendor_sarif",
        crate::common::redact_version(&vendor_format("sarif"))
    );
    let json: serde_json::Value = serde_json::from_str(&vendor_format("json")).expect("JSON");
    insta::assert_snapshot!(
        "flags_retirement_vendor_json",
        serde_json::to_string_pretty(&json["retirement"]).expect("pretty JSON")
    );
}

#[test]
fn the_api_route_builds_the_same_retirement_block_as_the_cli() {
    let cli = vendor_json(&[]);
    let options = fallow_api::FeatureFlagsOptions {
        analysis: fallow_api::AnalysisOptions {
            root: Some(fixture_path("flags-vendor")),
            no_cache: true,
            ..fallow_api::AnalysisOptions::default()
        },
        top: None,
        retirement: Some(fallow_api::FeatureFlagsRetirementOptions {
            flag_age: fallow_types::flag_retirement::FlagAgeMode::Off,
            flag_state: Some(std::path::PathBuf::from(vendor_state_path())),
            ..fallow_api::FeatureFlagsRetirementOptions::default()
        }),
    };
    let api = fallow_api::run_feature_flags(&options)
        .and_then(fallow_api::serialize_feature_flags_programmatic_json)
        .expect("API run");
    assert_eq!(api["retirement"], cli["retirement"]);
    assert_eq!(api["feature_flags"], cli["feature_flags"]);
}
