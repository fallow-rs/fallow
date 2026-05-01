mod common;

use common::{parse_json, run_fallow, run_fallow_in_root};
use std::path::Path;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

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
fn go_feature_flags_flow_through_flags_command() {
    let dir = tempdir().expect("create temp dir");
    write_file(
        &dir.path().join("go.mod"),
        "module example.com/go-flags\n\ngo 1.25.0\n",
    );
    write_file(
        &dir.path().join("main.go"),
        r#"package main

import "os"

type LDClient interface {
    BoolVariation(flagKey string, ctx any, fallback bool) bool
}

func main() {
    if os.Getenv("FEATURE_NEW_CHECKOUT") != "" {
        run()
    }
}

func enabled(client LDClient) bool {
    return client.BoolVariation("beta-search", nil, false)
}
"#,
    );

    let out = run_fallow_in_root("flags", dir.path(), &["--no-cache", "--format", "json"]);
    let json = parse_json(&out);
    let flags = json["feature_flags"]
        .as_array()
        .expect("feature_flags array");

    assert_eq!(
        flags.len(),
        2,
        "expected Go env + SDK flags, got: {flags:#?}"
    );
    assert!(
        flags.iter().any(|flag| {
            flag["flag_name"] == "FEATURE_NEW_CHECKOUT" && flag["kind"] == "environment_variable"
        }),
        "Go env flag should be reported: {flags:#?}"
    );
    assert!(
        flags.iter().any(|flag| {
            flag["flag_name"] == "beta-search"
                && flag["kind"] == "sdk_call"
                && flag["sdk_name"] == "LaunchDarkly"
        }),
        "Go SDK flag should be reported: {flags:#?}"
    );
}

#[test]
fn go_feature_flags_honor_custom_patterns() {
    let dir = tempdir().expect("create temp dir");
    write_file(
        &dir.path().join("go.mod"),
        "module example.com/go-flags-custom\n\ngo 1.25.0\n",
    );
    write_file(
        &dir.path().join(".fallowrc.json"),
        r#"{
  "flags": {
    "envPrefixes": ["MYAPP_ENABLE_"],
    "sdkPatterns": [
      { "function": "IsFeatureActive", "nameArg": 0, "provider": "Internal" }
    ]
  }
}"#,
    );
    write_file(
        &dir.path().join("main.go"),
        r#"package main

import "os"

func main() {
    _ = os.Getenv("MYAPP_ENABLE_V2")
    _ = IsFeatureActive("rollout-a")
}
"#,
    );

    let out = run_fallow_in_root("flags", dir.path(), &["--no-cache", "--format", "json"]);
    let json = parse_json(&out);
    let flags = json["feature_flags"]
        .as_array()
        .expect("feature_flags array");

    assert!(
        flags.iter().any(|flag| {
            flag["flag_name"] == "MYAPP_ENABLE_V2" && flag["kind"] == "environment_variable"
        }),
        "Go custom env-prefix flag should be reported: {flags:#?}"
    );
    assert!(
        flags.iter().any(|flag| {
            flag["flag_name"] == "rollout-a"
                && flag["kind"] == "sdk_call"
                && flag["sdk_name"] == "Internal"
        }),
        "Go custom SDK flag should be reported: {flags:#?}"
    );
}
