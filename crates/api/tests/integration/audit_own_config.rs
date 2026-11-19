//! The base pass of `fallow_api::run_audit` under the config of the base
//! commit. With an explicit `config_path`, the config of the base commit is
//! the base version of that file, as in `fallow audit --config`, and not a
//! config file that discovery finds in the base checkout.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use fallow_api::{
    AnalysisOptions, AuditGate, AuditOptions, run_audit, serialize_audit_programmatic_json,
};
use serde_json::Value;

use crate::common::{commit, git, write};

const ENTRY_INDEX_AND_EXTRA: &str = r#"{"entry": ["src/index.ts", "scripts/extra.ts"]}"#;
const ENTRY_INDEX: &str = r#"{"entry": ["src/index.ts"]}"#;

/// `(path, introduced)` of each unused file in the dead-code section.
fn unused_files(report: &Value) -> Vec<(String, bool)> {
    let mut files: Vec<(String, bool)> = report
        .pointer("/dead_code/unused_files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let path = item["path"].as_str()?.replace('\\', "/");
            Some((path, item["introduced"].as_bool().unwrap_or(false)))
        })
        .collect();
    files.sort();
    files
}

/// The change drops `scripts/extra.ts` from the entries of `ci/fallow.json`.
/// The discovered `.fallowrc.json` of the base has only `src/index.ts` as an
/// entry, so only the base version of `ci/fallow.json` shows that the change
/// made `scripts/extra.ts` unused.
#[test]
fn programmatic_audit_reads_the_base_copy_of_an_explicit_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).expect("create project");
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"audit-own-config","version":"1.0.0","private":true}"#,
    );
    write(&root, ".fallowrc.json", ENTRY_INDEX);
    write(&root, "ci/fallow.json", ENTRY_INDEX_AND_EXTRA);
    write(
        &root,
        "src/index.ts",
        "export const main = (): number => 1;\nconsole.log(main());\n",
    );
    write(
        &root,
        "scripts/extra.ts",
        "export const run = (): number => 5;\nconsole.log(run());\n",
    );
    commit(&root, "base");
    write(&root, "ci/fallow.json", ENTRY_INDEX);
    commit(&root, "drop an entry");

    let config_path = root.join("ci/fallow.json");
    let report = run_audit(&AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root),
            config_path: Some(config_path),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_owned()),
        gate: Some(AuditGate::NewOnly),
        ..AuditOptions::default()
    })
    .and_then(serialize_audit_programmatic_json)
    .expect("run the programmatic audit");

    assert_eq!(
        unused_files(&report),
        vec![("scripts/extra.ts".to_owned(), true)],
        "{:#}",
        report["dead_code"]
    );
    assert_eq!(report["verdict"], "fail", "{:#}", report["verdict"]);
}
