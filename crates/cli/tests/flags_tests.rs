mod common;

use common::run_fallow;

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
    // The empty-result hint goes to stderr (continuation of the result line).
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
    // Built-in env prefixes (sourced from extract) are surfaced.
    assert!(
        out.stderr.contains("FEATURE_*") && out.stderr.contains("TOGGLE_*"),
        "built-in env prefixes should be listed: {}",
        out.stderr
    );
    // Built-in SDK providers, including the current expanded set, are surfaced.
    assert!(
        out.stderr.contains("LaunchDarkly") && out.stderr.contains("Vercel Flags"),
        "built-in SDK providers should be listed: {}",
        out.stderr
    );
    // The config knobs and docs path are pointed at.
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
    // Terse acknowledgement of the user's custom config, correctly pluralized.
    assert!(
        out.stderr.contains("with your custom flag config")
            && out.stderr.contains("2 custom SDK patterns")
            && out.stderr.contains("1 custom env prefix"),
        "custom config should get a terse acknowledgement: {}",
        out.stderr
    );
    // No nag block: the full detector enumeration must NOT appear.
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
