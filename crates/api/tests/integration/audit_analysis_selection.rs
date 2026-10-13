//! `fallow_api::run_audit` skips a domain when `audit.deadCode`,
//! `audit.health` or `audit.dupes` is false, or when the matching
//! `AuditOptions` field is `Some(false)`, the same as `fallow audit --skip`
//! (#3381). The option wins over the config.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use fallow_api::{AnalysisOptions, AuditOptions, run_audit, serialize_audit_programmatic_json};
use serde_json::Value;

use crate::common::{commit, git, write};

fn audit(root: &Path, dead_code: Option<bool>) -> Value {
    audit_with(
        root,
        AuditOptions {
            dead_code,
            ..AuditOptions::default()
        },
    )
}

fn audit_with(root: &Path, selection: AuditOptions) -> Value {
    let options = AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_string()),
        css: Some(false),
        ..selection
    };
    run_audit(&options)
        .and_then(serialize_audit_programmatic_json)
        .expect("run the programmatic audit")
}

/// A repository whose head commit adds one complexity warning and one unused
/// file (a dead-code error). `audit` is the `audit` config section.
fn repository(audit: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"dead-code-toggle","private":true}"#,
    );
    write(
        &root,
        ".fallowrc.json",
        &format!(
            r#"{{"audit": {audit}, "entry": ["src/index.js"],
  "health": {{"maxCyclomatic": 15, "maxCognitive": 15, "maxCrap": 999999999}},
  "rules": {{"complexity-cyclomatic": "warn", "complexity-cognitive": "warn", "complexity-crap": "warn"}}}}"#
        ),
    );
    write(&root, "src/index.js", "export const ready = true;\n");
    commit(&root, "base");
    let mut route = String::from("export const ready = true;\nexport function route(x) {\n");
    for branch in 0..20 {
        writeln!(route, "  if (x === {branch}) return {branch};").unwrap();
    }
    route.push_str("  return -1;\n}\n");
    write(&root, "src/index.js", &route);
    write(&root, "src/orphan.js", "export const orphan = 1;\n");
    commit(&root, "head");
    dir
}

fn assert_skipped(report: &Value) {
    assert_eq!(report["verdict"], "warn", "{report:#}");
    assert!(report.get("dead_code").is_none(), "{report:#}");
    assert_eq!(report["summary"]["dead_code_issues"], 0, "{report:#}");
    assert_eq!(report["summary"]["dead_code_skipped"], true, "{report:#}");
    assert_eq!(
        report["attribution"]["complexity_introduced"], 1,
        "{report:#}"
    );
}

fn assert_ran(report: &Value) {
    assert_eq!(report["verdict"], "fail", "{report:#}");
    assert!(report.get("dead_code").is_some(), "{report:#}");
    assert!(
        report["summary"].get("dead_code_skipped").is_none(),
        "{report:#}"
    );
}

#[test]
fn dead_code_runs_by_default() {
    let dir = repository("{}");
    assert_ran(&audit(&dir.path().join("project"), None));
}

#[test]
fn config_dead_code_false_skips_the_domain() {
    let dir = repository(r#"{"deadCode": false}"#);
    assert_skipped(&audit(&dir.path().join("project"), None));
}

#[test]
fn option_overrides_the_config_in_both_directions() {
    let off = repository("{}");
    assert_skipped(&audit(&off.path().join("project"), Some(false)));

    let on = repository(r#"{"deadCode": false}"#);
    assert_ran(&audit(&on.path().join("project"), Some(true)));
}

#[test]
fn health_and_duplication_follow_the_config_and_the_options() {
    let assert_health_and_duplication_skipped = |report: &Value| {
        assert!(report.get("complexity").is_none(), "{report:#}");
        assert!(report.get("duplication").is_none(), "{report:#}");
        assert!(report.get("dead_code").is_some(), "{report:#}");
        assert_eq!(report["summary"]["complexity_skipped"], true, "{report:#}");
        assert_eq!(report["summary"]["duplication_skipped"], true, "{report:#}");
        assert_eq!(report["summary"]["complexity_findings"], 0, "{report:#}");
    };

    let config = repository(r#"{"health": false, "dupes": false}"#);
    assert_health_and_duplication_skipped(&audit(&config.path().join("project"), None));

    let options = repository("{}");
    assert_health_and_duplication_skipped(&audit_with(
        &options.path().join("project"),
        AuditOptions {
            health: Some(false),
            duplication: Some(false),
            ..AuditOptions::default()
        },
    ));

    let ran = audit(&options.path().join("project"), None);
    assert!(ran.get("complexity").is_some(), "{ran:#}");
    assert!(
        ran["summary"].get("complexity_skipped").is_none(),
        "{ran:#}"
    );
}

#[test]
fn a_selection_without_any_domain_is_an_error() {
    let dir = repository(r#"{"deadCode": false, "health": false, "dupes": false}"#);
    let root = dir.path().join("project");
    let options = AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_string()),
        ..AuditOptions::default()
    };
    let err = run_audit(&options).expect_err("nothing to run");
    assert_eq!(err.code.as_deref(), Some("FALLOW_AUDIT_NO_ANALYSIS"));
}
