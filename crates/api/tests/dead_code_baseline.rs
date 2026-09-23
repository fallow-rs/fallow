//! `fallow_api::run_dead_code_with_baseline` reads a dead-code baseline with
//! the engine function behind `fallow dead-code --baseline`.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{
    AnalysisOptions, DeadCodeOptions, run_dead_code_with_baseline,
    serialize_dead_code_programmatic_json,
};
use serde_json::Value;

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
        r#"{"name":"api-baseline","version":"1.0.0","main":"src/index.ts"}"#,
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

fn options(root: &Path) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        ..DeadCodeOptions::default()
    }
}

fn unused_export_names(report: &Value) -> Vec<String> {
    let mut names: Vec<String> = report["unused_exports"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| finding["export_name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

fn run(root: &Path, baseline: Option<&Path>) -> Value {
    run_dead_code_with_baseline(&options(root), baseline)
        .and_then(serialize_dead_code_programmatic_json)
        .expect("run the programmatic dead-code analysis")
}

#[test]
fn a_dead_code_baseline_hides_the_findings_it_holds() {
    let dir = project();
    let root = dir.path();
    assert_eq!(
        unused_export_names(&run(root, None)),
        ["unusedA", "unusedB"]
    );

    write(
        root,
        "baseline.json",
        r#"{"unused_files":[],"unused_exports":["src/a.ts:unusedA"],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[]}"#,
    );
    let report = run(root, Some(Path::new("baseline.json")));
    assert_eq!(unused_export_names(&report), ["unusedB"]);
}

#[test]
fn a_baseline_of_another_command_hides_nothing() {
    let dir = project();
    let root = dir.path();
    write(
        root,
        "dupes-baseline.json",
        r#"{"kind":"dupes","clone_groups":[]}"#,
    );
    let report = run(root, Some(Path::new("dupes-baseline.json")));
    assert_eq!(unused_export_names(&report), ["unusedA", "unusedB"]);
}

#[test]
fn an_invalid_baseline_is_an_error() {
    let dir = project();
    let root = dir.path();
    write(root, "broken.json", "{ not json");
    let error = run_dead_code_with_baseline(&options(root), Some(Path::new("broken.json")))
        .expect_err("an invalid baseline fails the run");
    assert_eq!(error.exit_code, 2);
    assert_eq!(error.code.as_deref(), Some("FALLOW_BASELINE_INVALID"));
}
