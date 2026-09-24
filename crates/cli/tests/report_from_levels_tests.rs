#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! `fallow report --from` takes each finding level from the saved envelope.
//!
//! A dead-code finding carries `effective_severity`, so the level does not
//! depend on the config at render time, also for an `overrides[].rules`
//! split and for prop-drilling, thin-wrapper and duplicate-prop-shape
//! findings. An old saved report without the field falls back to the rules of
//! the config. When no config is found for that fallback, a note says so.

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use common::{CommandOutput, fixture_path, parse_json, run_fallow_raw};
use serde_json::Value;

const SPLIT_CONFIG: &str = r#"{
  "entry": ["src/index.ts"],
  "rules": { "unused-exports": "error", "unused-dependencies": "off" },
  "overrides": [
    { "files": ["src/legacy/**"], "rules": { "unused-exports": "warn" } }
  ]
}
"#;

const NOTE: &str = "no fallow config found";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write fixture file");
}

/// A project with one unused export in `src/core.ts` (rule `error`) and one
/// in `src/legacy/old.ts` (override `warn`).
fn split_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"report-levels","private":true}"#,
    );
    write(root, ".fallowrc.json", SPLIT_CONFIG);
    write(
        root,
        "src/index.ts",
        "import { used } from './core';\nimport { kept } from './legacy/old';\nused(kept);\n",
    );
    write(
        root,
        "src/core.ts",
        "export const used = (x: number): number => x;\nexport const coreUnused = 1;\n",
    );
    write(
        root,
        "src/legacy/old.ts",
        "export const kept = 1;\nexport const legacyUnused = 2;\n",
    );
    dir
}

fn run_ok(args: &[&str]) -> CommandOutput {
    let output = run_fallow_raw(args);
    assert!(
        matches!(output.code, 0 | 1),
        "fallow {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    output
}

fn utf8(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

/// Save the dead-code JSON of `root` to a file next to it.
fn save_json(root: &Path, extra: &[&str], out: &Path) -> Value {
    let mut args = vec![
        "dead-code",
        "--root",
        utf8(root),
        "--quiet",
        "--format",
        "json",
    ];
    args.extend_from_slice(extra);
    let output = run_ok(&args);
    std::fs::write(out, &output.stdout).expect("write saved report");
    parse_json(&output)
}

/// Render a saved report from a root without a config.
fn report_from(saved: &Path, root: &Path, format: &str, extra: &[&str]) -> CommandOutput {
    let mut args = vec![
        "report",
        "--from",
        utf8(saved),
        "--root",
        utf8(root),
        "--quiet",
        "--format",
        format,
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    assert_eq!(
        output.code, 0,
        "report --from --format {format} failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    output
}

fn sarif_level(sarif: &Value, rule_id: &str, path: &str) -> String {
    sarif["runs"]
        .as_array()
        .expect("SARIF runs")
        .iter()
        .flat_map(|run| run["results"].as_array().into_iter().flatten())
        .find(|result| {
            result["ruleId"] == rule_id
                && result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.ends_with(path))
        })
        .unwrap_or_else(|| panic!("no SARIF {rule_id} result for {path} in {sarif}"))["level"]
        .as_str()
        .expect("SARIF level")
        .to_owned()
}

fn codeclimate_severity(issues: &Value, check_name: &str, path: &str) -> String {
    issues
        .as_array()
        .expect("CodeClimate array")
        .iter()
        .find(|issue| {
            issue["check_name"] == check_name
                && issue["location"]["path"]
                    .as_str()
                    .is_some_and(|found| found.ends_with(path))
        })
        .unwrap_or_else(|| panic!("no CodeClimate {check_name} issue for {path} in {issues}"))
        ["severity"]
        .as_str()
        .expect("severity")
        .to_owned()
}

/// Remove every `effective_severity` key, like a report from an older version.
fn strip_severity(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("effective_severity");
            map.values_mut().for_each(strip_severity);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_severity),
        _ => {}
    }
}

fn empty_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("empty root")
}

fn saved_path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

#[test]
fn override_split_survives_report_from_without_config() {
    let project = split_project();
    let store = empty_root();
    let saved = saved_path(&store, "saved.json");
    save_json(project.path(), &[], &saved);
    let render_root = empty_root();

    let sarif = parse_json(&report_from(&saved, render_root.path(), "sarif", &[]));
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", "src/core.ts"),
        "error"
    );
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", "src/legacy/old.ts"),
        "warning"
    );

    let issues = parse_json(&report_from(&saved, render_root.path(), "codeclimate", &[]));
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-export", "src/core.ts"),
        "major"
    );
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-export", "src/legacy/old.ts"),
        "minor"
    );
}

#[test]
fn sarif_rule_default_from_missing_config_gives_a_note() {
    let project = split_project();
    let store = empty_root();
    let saved = saved_path(&store, "saved.json");
    save_json(project.path(), &[], &saved);

    // Without a config, the rule default level of `fallow/unused-export`
    // comes from the default rules (`error`), which differs from the saved
    // `warning` level of the legacy finding.
    let render_root = empty_root();
    let output = report_from(&saved, render_root.path(), "sarif", &[]);
    assert!(
        output.stderr.contains(NOTE) && output.stderr.contains("fallow/unused-export"),
        "expected a note about the rule default levels, stderr:\n{}",
        output.stderr
    );

    // With the project root, the config is found and no note is given.
    let output = report_from(&saved, project.path(), "sarif", &[]);
    assert!(
        !output.stderr.contains(NOTE),
        "no note expected with a config, stderr:\n{}",
        output.stderr
    );
}

#[test]
fn old_report_without_severity_gives_a_note_for_a_missing_config() {
    let project = split_project();
    let store = empty_root();
    let saved = saved_path(&store, "saved.json");
    let mut envelope = save_json(project.path(), &[], &saved);
    strip_severity(&mut envelope);
    let old = saved_path(&store, "old.json");
    std::fs::write(&old, envelope.to_string()).expect("write old report");

    let render_root = empty_root();
    for format in ["codeclimate", "sarif", "pr-comment-github", "review-gitlab"] {
        let output = report_from(&old, render_root.path(), format, &[]);
        assert!(
            output.stderr.contains(NOTE) && output.stderr.contains("2 findings"),
            "{format}: expected a note about the fallback, stderr:\n{}",
            output.stderr
        );
    }

    // The old report stays readable with the config of the original run: the
    // base rule sets the level of each finding without a saved severity.
    let config = project.path().join(".fallowrc.json");
    let output = report_from(
        &old,
        render_root.path(),
        "codeclimate",
        &["--config", utf8(&config)],
    );
    assert!(
        !output.stderr.contains(NOTE),
        "no note expected with --config, stderr:\n{}",
        output.stderr
    );
    let issues = parse_json(&output);
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-export", "src/core.ts"),
        "major"
    );
}

#[test]
fn old_report_renders_when_the_render_config_turns_the_rule_off() {
    let project = split_project();
    let store = empty_root();
    let saved = saved_path(&store, "saved.json");
    let mut envelope = save_json(project.path(), &[], &saved);
    strip_severity(&mut envelope);
    let old = saved_path(&store, "old.json");
    std::fs::write(&old, envelope.to_string()).expect("write old report");
    let config = store.path().join("off.json");
    std::fs::write(&config, r#"{"rules":{"unused-exports":"off"}}"#).expect("write config");

    for format in ["sarif", "codeclimate"] {
        report_from(&old, store.path(), format, &["--config", utf8(&config)]);
    }
}

#[test]
fn non_gating_findings_carry_their_severity_through_report_from() {
    for (rule, rule_id, key) in [
        (
            "prop-drilling",
            "fallow/prop-drilling",
            "prop_drilling_chains",
        ),
        ("thin-wrapper", "fallow/thin-wrapper", "thin_wrappers"),
        (
            "duplicate-prop-shape",
            "fallow/duplicate-prop-shape",
            "duplicate_prop_shapes",
        ),
    ] {
        let store = empty_root();
        let config = store.path().join("config.json");
        std::fs::write(
            &config,
            format!(r#"{{"rules":{{"{rule}":"error","unused-dependencies":"off"}}}}"#),
        )
        .expect("write config");
        let fixture = fixture_path(rule);
        let saved = saved_path(&store, "saved.json");
        let envelope = save_json(&fixture, &["--config", utf8(&config)], &saved);
        let findings = envelope[key].as_array().expect("findings array");
        assert!(
            !findings.is_empty(),
            "{rule}: the fixture must have findings"
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding["effective_severity"] == "error"),
            "{rule}: every finding must carry its rule severity: {findings:?}"
        );

        let direct = run_ok(&[
            "dead-code",
            "--root",
            utf8(&fixture),
            "--config",
            utf8(&config),
            "--quiet",
            "--format",
            "sarif",
        ]);
        let direct = parse_json(&direct);
        let render_root = empty_root();
        let saved_sarif = parse_json(&report_from(&saved, render_root.path(), "sarif", &[]));
        let first_path = |sarif: &Value| -> String {
            sarif["runs"][0]["results"]
                .as_array()
                .expect("results")
                .iter()
                .find(|result| result["ruleId"] == rule_id)
                .unwrap_or_else(|| panic!("{rule}: no SARIF result"))["locations"][0]
                ["physicalLocation"]["artifactLocation"]["uri"]
                .as_str()
                .expect("uri")
                .to_owned()
        };
        let path = first_path(&direct);
        assert_eq!(
            sarif_level(&saved_sarif, rule_id, &path),
            sarif_level(&direct, rule_id, &path),
            "{rule}: report --from without a config must keep the level of the direct run"
        );
    }
}
