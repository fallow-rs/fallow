#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};

const FIXTURE: &str = "duplicate-export-aliases";

fn reported_paths(json: &serde_json::Value) -> Vec<String> {
    json["duplicate_export_aliases"]
        .as_array()
        .expect("duplicate_export_aliases array")
        .iter()
        .map(|finding| finding["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn json_reports_aliases_and_honors_ignore_exports() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let paths = reported_paths(&json);

    assert!(paths.contains(&"src/worker.ts".to_string()), "{paths:?}");
    // `.fallowrc.json` lists `default` for `src/ignored.ts` in `ignoreExports`.
    assert!(!paths.contains(&"src/ignored.ts".to_string()), "{paths:?}");
    assert_eq!(json["summary"]["duplicate_export_aliases"], paths.len());

    let worker = json["duplicate_export_aliases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["path"] == "src/worker.ts")
        .unwrap();
    assert_eq!(worker["local_name"], "layer");
    assert_eq!(
        worker["export_names"],
        serde_json::json!(["layer", "default"])
    );
    assert_eq!(worker["effective_severity"], "warn");
    assert_eq!(worker["locations"].as_array().unwrap().len(), 2);
    assert!(
        worker["finding_id"]
            .as_str()
            .unwrap()
            .starts_with("dc1:duplicate-export-alias:")
    );
    assert_eq!(worker["actions"][0]["type"], "remove-duplicate");
    assert_eq!(worker["actions"][0]["auto_fixable"], false);
    // The JSON layer merges the suppress comments of all findings on one
    // line, so the comment also names `unused-export` for `layer`.
    assert!(
        worker["actions"][1]["comment"]
            .as_str()
            .unwrap()
            .contains("duplicate-export-alias")
    );
}

#[test]
fn warn_default_keeps_exit_code_zero_and_filter_flag_limits_output() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--duplicate-export-aliases",
        ],
    );
    assert_eq!(output.code, 0, "a warn finding does not fail the run");
    let json = parse_json(&output);
    assert!(json["unused_exports"].as_array().is_none_or(Vec::is_empty));
    assert!(!reported_paths(&json).is_empty());
}

#[test]
fn rule_off_in_config_removes_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = crate::common::fixture_path(FIXTURE);
    for entry in walk(&root) {
        let target = dir.path().join(entry.strip_prefix(&root).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, target).unwrap();
    }
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "rules": { "duplicate-export-aliases": "off" } }"#,
    )
    .unwrap();
    let output = crate::common::run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    assert!(
        json["duplicate_export_aliases"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}
