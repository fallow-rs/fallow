//! `fallow_api::run_audit` reads the `audit` config section as `fallow audit`
//! does: `audit.gate`, `audit.css` and `audit.cssDeep` apply when the option is
//! unset, and an explicit option wins. The baseline keys of the section, which
//! the typed audit cannot apply, fail with a stable code instead of being
//! ignored.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::{Path, PathBuf};

use fallow_api::{
    AUDIT_CONFIG_BASELINE_UNSUPPORTED, AnalysisOptions, AuditGate, AuditOptions, run_audit,
    serialize_audit_programmatic_json,
};
use serde_json::Value;

use crate::common::{commit, git, write};

fn options(root: &Path, base: &str) -> AuditOptions {
    AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some(base.to_string()),
        ..AuditOptions::default()
    }
}

fn audit(options: &AuditOptions) -> Value {
    run_audit(options)
        .and_then(serialize_audit_programmatic_json)
        .expect("run the programmatic audit")
}

/// A repository whose head commit edits a file that was already unused, so
/// the only dead-code finding in the changed files is inherited. `audit` is
/// the `audit` config section.
fn inherited_finding_repository(audit: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"audit-gate","private":true}"#,
    );
    write(
        &root,
        ".fallowrc.json",
        &format!(r#"{{"audit": {audit}, "entry": ["src/index.js"]}}"#),
    );
    write(&root, "src/index.js", "export const ready = true;\n");
    write(&root, "src/orphan.js", "export const orphan = 1;\n");
    commit(&root, "base");
    write(&root, "src/orphan.js", "export const orphan = 2;\n");
    commit(&root, "head");
    (dir, root)
}

/// A repository whose head commit adds a raw color value (a local styling
/// finding) and an unused theme token (a cross-file styling finding).
fn styling_repository(audit: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"audit-css","private":true,"devDependencies":{"tailwindcss":"^4.0.0"}}"#,
    );
    write(
        &root,
        ".fallowrc.json",
        &format!(r#"{{"audit": {audit}, "entry": ["src/index.js"]}}"#),
    );
    write(&root, "src/index.js", "export const ready = true;\n");
    write(
        &root,
        "src/styles.css",
        "@theme {\n  --color-danger: red;\n}\n",
    );
    commit(&root, "base");
    write(
        &root,
        "src/styles.css",
        "@theme {\n  --color-danger: red;\n  --shadow-glow: 0 0 8px red;\n}\n.btn { color: red; }\n",
    );
    commit(&root, "head");
    (dir, root)
}

fn styling_sub_kinds(report: &Value) -> Vec<String> {
    report["complexity"]["styling_findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter_map(|finding| finding["sub_kind"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

const LOCAL_STYLING: &str = "raw-style-value";
const DEEP_STYLING: &str = "unused-theme-token";

#[test]
fn gate_defaults_to_new_only() {
    let (_dir, root) = inherited_finding_repository("{}");
    let report = audit(&options(&root, "HEAD~1"));
    assert_eq!(report["attribution"]["gate"], "new-only", "{report:#}");
    assert_eq!(report["verdict"], "pass", "{report:#}");
}

#[test]
fn config_gate_all_fails_on_an_inherited_finding() {
    let (_dir, root) = inherited_finding_repository(r#"{"gate": "all"}"#);
    let report = audit(&options(&root, "HEAD~1"));
    assert_eq!(report["attribution"]["gate"], "all", "{report:#}");
    assert_eq!(report["verdict"], "fail", "{report:#}");
}

#[test]
fn gate_option_overrides_the_config() {
    let (_dir, root) = inherited_finding_repository(r#"{"gate": "all"}"#);
    let report = audit(&AuditOptions {
        gate: Some(AuditGate::NewOnly),
        ..options(&root, "HEAD~1")
    });
    assert_eq!(report["attribution"]["gate"], "new-only", "{report:#}");
    assert_eq!(report["verdict"], "pass", "{report:#}");
}

/// The empty changeset short-circuits the run, but it still reports the gate
/// of the config.
#[test]
fn config_gate_applies_to_an_empty_changeset() {
    let (_dir, root) = inherited_finding_repository(r#"{"gate": "all"}"#);
    let report = audit(&options(&root, "HEAD"));
    assert_eq!(report["changed_files_count"], 0, "{report:#}");
    assert_eq!(report["attribution"]["gate"], "all", "{report:#}");
}

#[test]
fn styling_runs_local_and_deep_by_default() {
    let (_dir, root) = styling_repository("{}");
    let kinds = styling_sub_kinds(&audit(&options(&root, "HEAD~1")));
    assert!(kinds.iter().any(|kind| kind == LOCAL_STYLING), "{kinds:?}");
    assert!(kinds.iter().any(|kind| kind == DEEP_STYLING), "{kinds:?}");
}

#[test]
fn config_css_false_skips_styling() {
    let (_dir, root) = styling_repository(r#"{"css": false}"#);
    let kinds = styling_sub_kinds(&audit(&options(&root, "HEAD~1")));
    assert!(kinds.is_empty(), "{kinds:?}");
}

#[test]
fn config_css_deep_false_keeps_local_styling_only() {
    let (_dir, root) = styling_repository(r#"{"cssDeep": false}"#);
    let kinds = styling_sub_kinds(&audit(&options(&root, "HEAD~1")));
    assert!(kinds.iter().any(|kind| kind == LOCAL_STYLING), "{kinds:?}");
    assert!(!kinds.iter().any(|kind| kind == DEEP_STYLING), "{kinds:?}");
}

/// `css_deep` wins over `audit.cssDeep`, as `--css-deep` does.
#[test]
fn css_deep_option_overrides_the_config() {
    let (_dir, root) = styling_repository(r#"{"cssDeep": false}"#);
    let kinds = styling_sub_kinds(&audit(&AuditOptions {
        css_deep: Some(true),
        ..options(&root, "HEAD~1")
    }));
    assert!(kinds.iter().any(|kind| kind == DEEP_STYLING), "{kinds:?}");
}

/// `css` only turns styling off, as `--no-css` does. The CLI has no flag
/// that overrides `audit.css: false`, and neither does the option.
#[test]
fn css_option_turns_styling_off_but_not_on() {
    let (_dir, root) = styling_repository("{}");
    let kinds = styling_sub_kinds(&audit(&AuditOptions {
        css: Some(false),
        ..options(&root, "HEAD~1")
    }));
    assert!(kinds.is_empty(), "{kinds:?}");

    let (_dir, root) = styling_repository(r#"{"css": false}"#);
    let kinds = styling_sub_kinds(&audit(&AuditOptions {
        css: Some(true),
        css_deep: Some(true),
        ..options(&root, "HEAD~1")
    }));
    assert!(kinds.is_empty(), "{kinds:?}");
}

/// An empty changeset passes without reading a baseline, as `fallow audit`
/// does, so a baseline key does not fail it.
#[test]
fn config_baseline_does_not_fail_an_empty_changeset() {
    let (_dir, root) = inherited_finding_repository(r#"{"healthBaseline": "baseline.json"}"#);
    let report = audit(&options(&root, "HEAD"));
    assert_eq!(report["changed_files_count"], 0, "{report:#}");
    assert_eq!(report["verdict"], "pass", "{report:#}");
}

#[test]
fn config_baselines_fail_with_a_stable_code() {
    for key in ["deadCodeBaseline", "healthBaseline", "dupesBaseline"] {
        let (_dir, root) =
            inherited_finding_repository(&format!(r#"{{"{key}": "baseline.json"}}"#));
        let err = run_audit(&options(&root, "HEAD~1")).expect_err("a config baseline must fail");
        assert_eq!(
            err.code.as_deref(),
            Some(AUDIT_CONFIG_BASELINE_UNSUPPORTED),
            "{key}: {err:?}"
        );
        assert_eq!(
            err.context.as_deref(),
            Some(&*format!("audit.{key}")),
            "{err:?}"
        );
        assert_eq!(err.exit_code, 2, "{err:?}");
    }
}
