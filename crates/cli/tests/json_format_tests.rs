#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, run_fallow, run_fallow_combined, run_fallow_raw, run_fallow_raw_with_env,
};

fn normalize_volatile_fields(value: &mut serde_json::Value) {
    if let Some(object) = value.as_object_mut() {
        object.remove("elapsed_ms");
        if let Some(telemetry) = object
            .get_mut("_meta")
            .and_then(|meta| meta.get_mut("telemetry"))
            .and_then(serde_json::Value::as_object_mut)
        {
            telemetry.remove("analysis_run_id");
        }
        for child in object.values_mut() {
            normalize_volatile_fields(child);
        }
    } else if let Some(items) = value.as_array_mut() {
        for item in items {
            normalize_volatile_fields(item);
        }
    }
}

#[test]
fn json_report_is_compact_by_default() {
    let output = run_fallow(
        "dead-code",
        "basic-project",
        &["--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 1, "fixture should report issues");
    assert!(
        output.stdout.ends_with('\n') && !output.stdout.ends_with("\n\n"),
        "JSON output should end in exactly one line feed"
    );
    assert_eq!(
        output.stdout.lines().count(),
        1,
        "default JSON output should be one compact line"
    );
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("compact output should remain valid JSON");
}

#[test]
fn pretty_flag_indents_the_same_json_value() {
    let compact = run_fallow(
        "dead-code",
        "basic-project",
        &["--format", "json", "--quiet"],
    );
    let pretty = run_fallow(
        "dead-code",
        "basic-project",
        &["--format", "json", "--pretty", "--quiet"],
    );

    assert_eq!(
        pretty.code, compact.code,
        "presentation must not change exit code"
    );
    assert!(
        pretty.stdout.ends_with('\n') && !pretty.stdout.ends_with("\n\n"),
        "pretty JSON should end in exactly one line feed"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent JSON across multiple lines"
    );

    let mut compact_value: serde_json::Value =
        serde_json::from_str(&compact.stdout).expect("compact JSON should parse");
    let mut pretty_value: serde_json::Value =
        serde_json::from_str(&pretty.stdout).expect("pretty JSON should parse");
    normalize_volatile_fields(&mut compact_value);
    normalize_volatile_fields(&mut pretty_value);
    assert_eq!(
        pretty_value, compact_value,
        "formatting must not change data"
    );
}

#[test]
fn pretty_flag_rejects_non_json_output() {
    let output = run_fallow("dead-code", "basic-project", &["--pretty", "--quiet"]);

    assert_eq!(output.code, 2, "invalid presentation flags should exit 2");
    assert!(
        output.stdout.is_empty(),
        "usage errors should not use stdout"
    );
    assert!(
        output.stderr.contains("--pretty requires JSON output")
            && output.stderr.contains("--format json --pretty"),
        "error should explain how to select JSON or remove --pretty: {}",
        output.stderr
    );
}

#[test]
fn fused_short_json_format_overrides_environment() {
    let root = fixture_path("basic-project");
    let output = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            root.to_str().expect("fixture path should be UTF-8"),
            "-fjson",
            "--pretty",
            "--quiet",
        ],
        &[("FALLOW_FORMAT", "human")],
    );

    assert_eq!(
        output.code, 1,
        "explicit -fjson should win over the environment"
    );
    assert!(
        output.stdout.lines().count() > 1,
        "--pretty should indent JSON"
    );
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("fused short format should produce valid JSON");
}

#[test]
fn json_format_precedence_is_resolved_before_pretty_validation() {
    let root = fixture_path("basic-project");
    let root = root.to_str().expect("fixture path should be UTF-8");

    let env_json = run_fallow_raw_with_env(
        &["dead-code", "--root", root, "--pretty", "--quiet"],
        &[("FALLOW_FORMAT", "json")],
    );
    let explicit_human = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            root,
            "--format",
            "human",
            "--pretty",
            "--quiet",
        ],
        &[("FALLOW_FORMAT", "json")],
    );
    let equals_json = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format=json",
        "--pretty",
        "--quiet",
    ]);

    assert!(
        env_json.stdout.lines().count() > 1,
        "environment JSON should allow --pretty"
    );
    assert_eq!(
        explicit_human.code, 2,
        "explicit human output should reject --pretty"
    );
    assert!(
        explicit_human
            .stderr
            .contains("--pretty requires JSON output"),
        "explicit format should override the environment"
    );
    assert!(
        equals_json.stdout.lines().count() > 1,
        "--format=json should allow --pretty"
    );
}

#[test]
fn ci_pretty_requires_an_explicit_json_override_before_opening_output_file() {
    let root = fixture_path("basic-project");
    let output_dir = tempfile::tempdir().expect("tempdir should be created");
    let output_path = output_dir.path().join("report.json");
    let rejected = run_fallow_raw(&[
        "dead-code",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--ci",
        "--pretty",
        "--output-file",
        output_path.to_str().expect("output path should be UTF-8"),
    ]);
    let accepted = run_fallow_raw(&[
        "dead-code",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--ci",
        "--format",
        "json",
        "--pretty",
        "--quiet",
    ]);

    assert_eq!(
        rejected.code, 2,
        "CI defaults to SARIF, so --pretty should be rejected"
    );
    assert!(rejected.stderr.contains("--pretty requires JSON output"));
    assert!(
        !output_path.exists(),
        "validation should happen before opening the output file"
    );
    assert!(
        accepted.stdout.lines().count() > 1,
        "explicit JSON should override the CI format"
    );
}

#[test]
fn pretty_is_global_before_or_after_the_subcommand() {
    let root = fixture_path("basic-project");
    let root = root.to_str().expect("fixture path should be UTF-8");
    let before = run_fallow_raw(&[
        "--format",
        "json",
        "--pretty",
        "dead-code",
        "--root",
        root,
        "--quiet",
    ]);
    let after = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        "json",
        "--pretty",
        "--quiet",
    ]);

    assert!(
        before.stdout.lines().count() > 1,
        "global --pretty should work before the command"
    );
    assert!(
        after.stdout.lines().count() > 1,
        "global --pretty should work after the command"
    );
}

#[test]
fn output_file_preserves_the_selected_json_style_and_one_line_feed() {
    let root = fixture_path("basic-project");
    let output_dir = tempfile::tempdir().expect("tempdir should be created");
    let compact_path = output_dir.path().join("compact.json");
    let pretty_path = output_dir.path().join("pretty.json");

    for (path, pretty) in [(&compact_path, false), (&pretty_path, true)] {
        let mut args = vec![
            "dead-code",
            "--root",
            root.to_str().expect("fixture path should be UTF-8"),
            "--format",
            "json",
            "--quiet",
            "--output-file",
            path.to_str().expect("output path should be UTF-8"),
        ];
        if pretty {
            args.push("--pretty");
        }
        let output = run_fallow_raw(&args);
        assert_eq!(output.code, 1, "fixture should still report issues");
        assert!(
            output.stdout.is_empty(),
            "reports should be redirected to the file"
        );
    }

    let compact = std::fs::read_to_string(&compact_path).expect("compact report should exist");
    let pretty = std::fs::read_to_string(&pretty_path).expect("pretty report should exist");
    assert_eq!(
        compact.lines().count(),
        1,
        "compact file should contain one JSON line"
    );
    assert!(pretty.lines().count() > 1, "pretty file should be indented");
    assert!(compact.ends_with('\n') && !compact.ends_with("\n\n"));
    assert!(pretty.ends_with('\n') && !pretty.ends_with("\n\n"));
}

#[test]
fn combined_json_uses_the_selected_presentation_style() {
    let compact = run_fallow_combined(
        "basic-project",
        &["--format", "json", "--quiet", "--only", "dead-code"],
    );
    let pretty = run_fallow_combined(
        "basic-project",
        &[
            "--format",
            "json",
            "--pretty",
            "--quiet",
            "--only",
            "dead-code",
        ],
    );

    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "combined JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent combined JSON"
    );
    serde_json::from_str::<serde_json::Value>(&compact.stdout).expect("compact JSON should parse");
    serde_json::from_str::<serde_json::Value>(&pretty.stdout).expect("pretty JSON should parse");
}

#[test]
fn health_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "health",
        "basic-project",
        &["--format", "json", "--quiet", "--score"],
    );
    let pretty = run_fallow(
        "health",
        "basic-project",
        &["--format", "json", "--pretty", "--quiet", "--score"],
    );

    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "health JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent health JSON"
    );
}

#[test]
fn duplication_json_uses_the_selected_presentation_style() {
    let compact = run_fallow("dupes", "duplicate-code", &["--format", "json", "--quiet"]);
    let pretty = run_fallow(
        "dupes",
        "duplicate-code",
        &["--format", "json", "--pretty", "--quiet"],
    );

    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "duplication JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent duplication JSON"
    );
}

#[test]
fn config_json_uses_the_selected_presentation_style() {
    let root = fixture_path("basic-project");
    let root = root.to_str().expect("fixture path should be UTF-8");
    let compact = run_fallow_raw(&["config", "--root", root]);
    let pretty = run_fallow_raw(&["config", "--root", root, "--pretty"]);

    assert_eq!(compact.code, 0, "config should render successfully");
    assert_eq!(pretty.code, 0, "config --pretty should render successfully");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "config JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent config JSON"
    );
}

#[test]
fn config_path_rejects_pretty_because_it_is_not_json() {
    let root = fixture_path("basic-project");
    let output = run_fallow_raw(&[
        "config",
        "--path",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--pretty",
    ]);

    assert_eq!(
        output.code, 2,
        "path mode should reject JSON presentation flags"
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.contains("--pretty requires JSON output"));
}

#[test]
fn schema_commands_use_the_selected_presentation_style() {
    let commands: &[(&str, &[&str], &[&str])] = &[
        ("schema", &["schema"], &["schema", "--pretty"]),
        (
            "config-schema",
            &["config-schema"],
            &["config-schema", "--pretty"],
        ),
        (
            "plugin-schema",
            &["plugin-schema"],
            &["plugin-schema", "--pretty"],
        ),
        (
            "rule-pack-schema",
            &["rule-pack-schema"],
            &["rule-pack-schema", "--pretty"],
        ),
        (
            "rule-pack schema",
            &["rule-pack", "schema"],
            &["rule-pack", "schema", "--pretty"],
        ),
    ];

    for (name, compact_args, pretty_args) in commands {
        let compact = run_fallow_raw(compact_args);
        let pretty = run_fallow_raw(pretty_args);

        assert_eq!(compact.code, 0, "{name} should succeed");
        assert_eq!(
            pretty.code, compact.code,
            "presentation must not change exit code"
        );
        assert_eq!(
            compact.stdout.lines().count(),
            1,
            "{name} JSON should be compact by default"
        );
        assert!(
            pretty.stdout.lines().count() > 1,
            "--pretty should indent {name} JSON"
        );
        let compact_value = serde_json::from_str::<serde_json::Value>(&compact.stdout)
            .expect("compact schema output should be valid JSON");
        let pretty_value = serde_json::from_str::<serde_json::Value>(&pretty.stdout)
            .expect("pretty schema output should be valid JSON");
        assert_eq!(
            compact_value, pretty_value,
            "presentation must not change values"
        );
    }
}

#[test]
fn always_json_commands_accept_pretty_without_a_format_override() {
    let root = fixture_path("basic-project");
    let coverage = run_fallow_raw(&[
        "coverage",
        "setup",
        "--json",
        "--pretty",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
    ]);

    assert_eq!(coverage.code, 0, "coverage setup JSON should succeed");
    serde_json::from_str::<serde_json::Value>(&coverage.stdout)
        .expect("coverage setup should remain valid JSON");
    assert!(coverage.stdout.lines().count() > 1);
}

#[test]
fn ci_json_accepts_and_honors_pretty_without_a_format_override() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let body = dir.path().join("comment.md");
    std::fs::write(&body, "Review body").expect("comment body should be written");
    let body = body.to_str().expect("comment path should be UTF-8");

    let compact = run_fallow_raw(&[
        "ci",
        "plan-pr-comment",
        "--body",
        body,
        "--marker-id",
        "json-style-test",
    ]);
    let pretty = run_fallow_raw(&[
        "ci",
        "plan-pr-comment",
        "--body",
        body,
        "--marker-id",
        "json-style-test",
        "--pretty",
    ]);

    assert_eq!(compact.code, 0, "CI JSON should succeed");
    assert_eq!(pretty.code, 0, "CI JSON with --pretty should succeed");
    assert_eq!(compact.stdout.lines().count(), 1);
    assert!(pretty.stdout.lines().count() > 1);
}

#[test]
fn human_only_commands_reject_pretty_even_with_json_format_selected() {
    let template = run_fallow_raw(&["ci-template", "gitlab", "--format", "json", "--pretty"]);
    let root = fixture_path("basic-project");
    let missing = root.join("missing-root");
    let missing = missing.to_str().expect("missing path should be UTF-8");
    let hooks = run_fallow_raw(&[
        "hooks",
        "install",
        "--target",
        "git",
        "--dry-run",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--pretty",
    ]);
    let impact = run_fallow_raw(&[
        "impact", "enable", "--root", missing, "--format", "json", "--pretty",
    ]);
    let coverage = run_fallow_raw(&[
        "coverage",
        "upload-inventory",
        "--root",
        missing,
        "--format",
        "json",
        "--pretty",
    ]);

    for output in [template, hooks, impact, coverage] {
        assert_eq!(output.code, 2);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.contains("--pretty requires JSON output"));
    }
}

#[test]
fn config_errors_are_structured_json_without_a_format_override() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let missing = dir.path().join("missing.json");
    let output = run_fallow_raw(&[
        "config",
        "--config",
        missing.to_str().expect("config path should be UTF-8"),
        "--pretty",
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    assert!(output.stdout.lines().count() > 1);
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("config error should be structured JSON");
}

#[test]
fn structured_json_errors_use_the_selected_presentation_style() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let missing = dir.path().join("missing");
    let missing = missing.to_str().expect("path should be UTF-8");
    let compact = run_fallow_raw(&[
        "dead-code",
        "--root",
        missing,
        "--format",
        "json",
        "--quiet",
    ]);
    let pretty = run_fallow_raw(&[
        "dead-code",
        "--root",
        missing,
        "--format",
        "json",
        "--pretty",
        "--quiet",
    ]);

    assert_eq!(compact.code, 2, "invalid root should exit 2");
    assert_eq!(pretty.code, 2, "invalid root should exit 2");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "JSON errors should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent JSON errors"
    );
    assert!(
        compact.stderr.is_empty(),
        "JSON errors should stay on stdout"
    );
    assert!(
        pretty.stderr.is_empty(),
        "JSON errors should stay on stdout"
    );
}

#[test]
fn clap_json_errors_honor_pretty_before_parsing_finishes() {
    let output = run_fallow_raw(&[
        "dead-code",
        "--format",
        "json",
        "--pretty",
        "--not-a-real-flag",
    ]);

    assert_eq!(output.code, 2, "invalid CLI syntax should exit 2");
    assert!(
        output.stdout.lines().count() > 1,
        "--pretty should indent clap JSON errors"
    );
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("clap error should remain structured JSON");
}

#[test]
fn pre_dispatch_json_errors_honor_pretty() {
    let root = fixture_path("basic-project");
    let output = run_fallow_raw(&[
        "config",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--pretty",
        "--fail-on-issues",
    ]);

    assert_eq!(
        output.code, 2,
        "invalid global flag placement should exit 2"
    );
    assert!(
        output.stdout.lines().count() > 1,
        "--pretty should indent pre-dispatch JSON errors"
    );
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("pre-dispatch error should remain structured JSON");
}

#[test]
fn command_level_json_errors_honor_pretty() {
    let root = fixture_path("basic-project");
    let output = run_fallow_raw(&[
        "guard",
        "../outside.ts",
        "--root",
        root.to_str().expect("fixture path should be UTF-8"),
        "--format",
        "json",
        "--pretty",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "invalid guard target should exit 2");
    assert!(
        output.stdout.lines().count() > 1,
        "--pretty should apply to command-level JSON errors"
    );
    serde_json::from_str::<serde_json::Value>(&output.stdout)
        .expect("command error should remain structured JSON");
}

#[test]
fn inspect_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "inspect",
        "basic-project",
        &["--file", "src/utils.ts", "--format", "json", "--quiet"],
    );
    let pretty = run_fallow(
        "inspect",
        "basic-project",
        &[
            "--file",
            "src/utils.ts",
            "--format",
            "json",
            "--pretty",
            "--quiet",
        ],
    );

    assert_eq!(compact.code, 0, "inspect should succeed");
    assert_eq!(pretty.code, 0, "inspect --pretty should succeed");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "inspect JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent inspect JSON"
    );
}

#[test]
fn security_json_uses_the_selected_presentation_style() {
    let compact = run_fallow(
        "security",
        "basic-project",
        &["--format", "json", "--quiet"],
    );
    let pretty = run_fallow(
        "security",
        "basic-project",
        &["--format", "json", "--pretty", "--quiet"],
    );

    assert_eq!(compact.code, 0, "security should succeed");
    assert_eq!(pretty.code, 0, "security --pretty should succeed");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "security JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent security JSON"
    );
}

#[test]
fn impact_json_uses_the_selected_presentation_style() {
    let compact = run_fallow("impact", "basic-project", &["--format", "json", "--quiet"]);
    let pretty = run_fallow(
        "impact",
        "basic-project",
        &["--format", "json", "--pretty", "--quiet"],
    );

    assert_eq!(compact.code, 0, "impact should succeed");
    assert_eq!(pretty.code, 0, "impact --pretty should succeed");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "impact JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent impact JSON"
    );
}

#[test]
fn telemetry_status_json_uses_the_selected_presentation_style() {
    let compact = run_fallow_raw(&["telemetry", "status", "--format", "json"]);
    let pretty = run_fallow_raw(&["telemetry", "status", "--format", "json", "--pretty"]);

    assert_eq!(compact.code, 0, "telemetry status should succeed");
    assert_eq!(pretty.code, 0, "telemetry status --pretty should succeed");
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "telemetry JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent telemetry JSON"
    );
}
