#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

mod common;

use common::{run_fallow, run_fallow_combined, run_fallow_in_root};

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
        let out = run_fallow(
            subcommand,
            "basic-project",
            &["--no-cache", "--quiet", "--fail-on-stale-baseline"],
        );
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
