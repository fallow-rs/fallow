//! Run notices: a flag without effect and an environment value that fallow
//! cannot read travel in `workspace_diagnostics`. The human output prints a
//! note on stderr; `--quiet` and the machine formats print none.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw_with_env};

/// A small project. `config` is the content of `.fallowrc.json`.
fn project(config: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "run-notices", "private": true, "type": "module", "main": "src/index.js"}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    fs::write(
        dir.join("src/index.js"),
        "import { used } from './lib.js';\nconsole.log(used);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.js"),
        "export const used = 1;\nexport const unused = 2;\n",
    )
    .unwrap();
    tmp
}

/// The same project as a git repository with a second commit that adds an
/// unused export.
fn audited_project() -> TempDir {
    let tmp = project("{}");
    let dir = tmp.path();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "base");
    fs::write(
        dir.join("src/lib.js"),
        "export const used = 1;\nexport const unused = 2;\nexport const added = 3;\n",
    )
    .unwrap();
    commit_all(dir, "change");
    tmp
}

fn root_arg(tmp: &TempDir) -> String {
    tmp.path().to_str().unwrap().to_owned()
}

fn notices<'a>(diagnostics: &'a Value, kind: &str) -> Vec<&'a Value> {
    diagnostics
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry["kind"] == kind)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn inert_dupes_baseline_note_is_quiet_in_json() {
    let tmp = project(r#"{"duplicates": {"enabled": false}}"#);
    let root = root_arg(&tmp);
    let baseline = tmp.path().join("dupes-baseline.json");
    let output = run_fallow_raw_with_env(
        &[
            "--root",
            &root,
            "--dupes-baseline",
            baseline.to_str().unwrap(),
            "--format",
            "json",
        ],
        &[],
    );

    assert!(output.code == 0 || output.code == 1, "{}", output.stdout);
    assert!(
        !output.stderr.contains("--dupes-baseline has no effect"),
        "a machine format prints no note: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let inert = notices(&json["workspace_diagnostics"], "inert-flag");
    assert_eq!(inert.len(), 1, "{json:#}");
    assert_eq!(inert[0]["flag"], "--dupes-baseline");
    assert_eq!(inert[0]["analysis"], "duplication");
    assert_eq!(inert[0]["key"], "duplicates.enabled");
    assert_eq!(inert[0]["path"], ".", "no host path: {json:#}");
}

#[test]
fn audit_json_has_root_workspace_diagnostics_only_when_not_empty() {
    let tmp = audited_project();
    let root = root_arg(&tmp);
    let args = [
        "audit", "--root", &root, "--base", "HEAD~1", "--format", "json", "--quiet",
    ];

    let clean = parse_json(&run_fallow_raw_with_env(&args, &[]));
    assert!(
        clean.get("workspace_diagnostics").is_none(),
        "a run without a notice has no root member: {clean:#}"
    );

    let noticed = parse_json(&run_fallow_raw_with_env(
        &args,
        &[("FALLOW_PRODUCTION", "maybe")],
    ));
    let ignored = notices(&noticed["workspace_diagnostics"], "env-value-ignored");
    assert_eq!(ignored.len(), 1, "{noticed:#}");
    assert_eq!(ignored[0]["name"], "FALLOW_PRODUCTION");
}

#[test]
fn env_value_ignored_names_the_variable_not_the_value() {
    let tmp = project("{}");
    let root = root_arg(&tmp);
    let output = run_fallow_raw_with_env(
        &["dead-code", "--root", &root, "--format", "json", "--quiet"],
        &[("FALLOW_PRODUCTION", "maybe-later")],
    );

    assert!(output.code == 0 || output.code == 1, "{}", output.stdout);
    assert!(
        !output.stdout.contains("maybe-later") && !output.stderr.contains("maybe-later"),
        "the output never holds the value: {}",
        output.stdout
    );
    let json = parse_json(&output);
    let ignored = notices(&json["workspace_diagnostics"], "env-value-ignored");
    assert_eq!(ignored.len(), 1, "{json:#}");
    assert_eq!(ignored[0]["name"], "FALLOW_PRODUCTION");
    assert_eq!(ignored[0]["path"], ".", "no host path: {json:#}");
    assert_eq!(ignored[0]["degrades_analysis"], true);
}

#[test]
fn a_human_run_prints_one_note_per_notice_and_quiet_prints_none() {
    let tmp = project("{}");
    let root = root_arg(&tmp);
    let env = [("FALLOW_PRODUCTION", "maybe")];

    let human = run_fallow_raw_with_env(&["dead-code", "--root", &root], &env);
    assert_eq!(
        human
            .stderr
            .matches("Note: The value of FALLOW_PRODUCTION is not a bool")
            .count(),
        1,
        "{}",
        human.stderr
    );

    let quiet = run_fallow_raw_with_env(&["dead-code", "--root", &root, "--quiet"], &env);
    assert!(
        !quiet.stderr.contains("FALLOW_PRODUCTION"),
        "{}",
        quiet.stderr
    );
}

#[test]
fn production_on_trace_path_is_an_inert_flag_note() {
    let tmp = project("{}");
    let root = root_arg(&tmp);
    let output = run_fallow_raw_with_env(
        &[
            "trace",
            "--root",
            &root,
            "--path",
            "src/index.js",
            "src/lib.js",
            "--production",
        ],
        &[],
    );

    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("Note: --production has no effect: this command does not read"),
        "{}",
        output.stderr
    );
}

#[test]
fn list_does_not_count_a_run_notice_as_a_workspace_diagnostic() {
    let tmp = project("{}");
    fs::create_dir_all(tmp.path().join("node_modules")).unwrap();
    let root = root_arg(&tmp);
    let output = run_fallow_raw_with_env(
        &["list", "--root", &root],
        &[("FALLOW_PRODUCTION", "maybe")],
    );

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        !output.stderr.contains("workspace discovery diagnostic")
            && !output.stderr.contains("No workspaces discovered")
            && !output.stderr.contains("FALLOW_PRODUCTION"),
        "{}",
        output.stderr
    );
}
