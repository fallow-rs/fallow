//! The unmatched config pattern entries (`ignoreFindings`,
//! `ignoreDependencies`) of `fallow_api::run_audit`, the audit behind the MCP
//! typed route. They must agree with `fallow audit --format json`, which
//! carries them in `dead_code.workspace_diagnostics[]`.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use fallow_api::{
    AnalysisOptions, AuditGate, AuditOptions, run_audit, serialize_audit_programmatic_json,
};
use serde_json::Value;

use crate::common::{commit, git, write};

/// `(kind, pattern)` of each unmatched config pattern in the dead-code
/// section of an audit envelope.
fn unmatched_entries(report: &Value) -> Vec<(String, String)> {
    report
        .pointer("/dead_code/workspace_diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let kind = entry["kind"].as_str()?;
            kind.ends_with("-unmatched").then(|| {
                (
                    kind.to_owned(),
                    entry["pattern"].as_str().unwrap_or_default().to_owned(),
                )
            })
        })
        .collect()
}

#[test]
fn programmatic_audit_reports_unmatched_config_patterns_like_the_cli() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).expect("create project");
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"audit-config-patterns","private":true,"main":"src/index.ts",
            "dependencies":{"@acme/lib":"1.0.0"}}"#,
    );
    write(
        &root,
        ".fallowrc.json",
        r#"{"ignoreDependencies":["@acme/*","@acm/*"],"ignoreFindings":["src/hiden.ts"]}"#,
    );
    write(
        &root,
        "src/index.ts",
        "import '@acme/lib';\nexport const main = 1;\n",
    );
    write(&root, "src/orphan.ts", "export const orphan = 1;\n");
    commit(&root, "base");
    write(
        &root,
        "src/orphan.ts",
        "export const orphan = 1;\nexport const second = 2;\n",
    );
    commit(&root, "change");

    let report = run_audit(&AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_owned()),
        gate: AuditGate::NewOnly,
        ..AuditOptions::default()
    })
    .and_then(serialize_audit_programmatic_json)
    .expect("run the programmatic audit");

    assert_eq!(
        unmatched_entries(&report),
        vec![
            (
                "ignore-findings-pattern-unmatched".to_owned(),
                "src/hiden.ts".to_owned()
            ),
            (
                "ignore-dependencies-glob-unmatched".to_owned(),
                "@acm/*".to_owned()
            ),
        ],
        "{:#}",
        report["dead_code"]["workspace_diagnostics"]
    );
}
