//! `DeadCodeOptions::finding_ids` gives the programmatic API the same
//! finding-id query as `fallow dead-code --finding-id`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{
    AnalysisOptions, DeadCodeOptions, run_circular_dependencies, run_dead_code,
    run_dead_code_with_baseline, serialize_dead_code_programmatic_json,
};
use serde_json::Value;

const UNKNOWN_ID: &str = "dc1:unused-export:0000000000000000";

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

/// `src/a.ts` has two unused exports, `unusedA` and `unusedB`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"api-finding-ids","version":"1.0.0","main":"src/index.ts"}"#,
    );
    write(
        root,
        "src/index.ts",
        "import { used } from \"./a\";\nconsole.log(used);\n",
    );
    write(
        root,
        "src/a.ts",
        "export const used = 1;\nexport const unusedA = 2;\nexport const unusedB = 3;\n",
    );
    dir
}

fn options(root: &Path, finding_ids: &[&str]) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        finding_ids: finding_ids.iter().map(|id| (*id).to_owned()).collect(),
        ..DeadCodeOptions::default()
    }
}

fn run(options: &DeadCodeOptions, baseline: Option<&Path>) -> Value {
    run_dead_code_with_baseline(options, baseline)
        .and_then(serialize_dead_code_programmatic_json)
        .expect("run the programmatic dead-code analysis")
}

fn export_id(report: &Value, name: &str) -> String {
    report["unused_exports"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|finding| finding["export_name"] == name)
        .and_then(|finding| finding["finding_id"].as_str())
        .unwrap_or_else(|| panic!("no unused export {name}: {report}"))
        .to_owned()
}

#[test]
fn finding_ids_keep_only_the_requested_findings() {
    let dir = project();
    let root = dir.path();
    let full = run(&options(root, &[]), None);
    let wanted = export_id(&full, "unusedA");

    let report = run(&options(root, &[&wanted, UNKNOWN_ID]), None);

    assert_eq!(report["total_issues"], 1);
    assert_eq!(export_id(&report, "unusedA"), wanted);
    let query = &report["finding_id_query"];
    assert_eq!(query["found"], serde_json::json!([wanted]));
    assert_eq!(query["missing"], serde_json::json!([UNKNOWN_ID]));
    assert_eq!(query["conclusive"], true);
    assert!(full.get("finding_id_query").is_none());
}

#[test]
fn a_baseline_makes_the_answer_inconclusive() {
    let dir = project();
    let root = dir.path();
    let full = run(&options(root, &[]), None);
    let wanted = export_id(&full, "unusedA");
    let baseline = root.join("fallow-baseline.json");
    fs::write(
        &baseline,
        r#"{"unused_files":[],"unused_exports":["src/a.ts:unusedA"],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[]}"#,
    )
    .unwrap();

    let report = run(&options(root, &[&wanted]), Some(&baseline));

    let query = &report["finding_id_query"];
    assert_eq!(query["missing"], serde_json::json!([wanted]));
    assert_eq!(query["filtered"], serde_json::json!([wanted]));
    assert_eq!(query["conclusive"], false);
    let reasons = query["inconclusive_reasons"].as_array().unwrap();
    assert!(reasons.contains(&serde_json::json!("baseline")), "{query}");
}

#[test]
fn a_malformed_finding_id_is_an_error() {
    let dir = project();
    let err = run_dead_code(&options(dir.path(), &["unusedA"])).expect_err("refused");

    assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_FINDING_ID"));
}

#[test]
fn a_family_runner_refuses_finding_ids() {
    let dir = project();
    let err = run_circular_dependencies(&options(dir.path(), &[UNKNOWN_ID])).expect_err("refused");

    assert_eq!(err.code.as_deref(), Some("FALLOW_UNSUPPORTED_OPTION"));
}
