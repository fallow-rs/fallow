//! A production test pattern such as `**/test/**` can drop a file that a
//! production file imports (issue #4498). Discovery keeps the patterns as
//! they are, and the dead-code pass records one
//! `production-excluded-file-imported` diagnostic for each pattern that
//! dropped such a file.

use std::fs;
use std::path::Path;

use fallow_config::ResolvedConfig;

use super::common::create_config;

const KIND: &str = "production-excluded-file-imported";

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// The serialized entries of this kind that the registry holds for the root.
fn reported(config: &ResolvedConfig) -> Vec<serde_json::Value> {
    fallow_config::workspace_diagnostics_for(&config.root)
        .into_iter()
        .filter(|diagnostic| diagnostic.kind.id() == KIND)
        .map(|diagnostic| {
            let relative = diagnostic.into_root_relative(&config.root);
            serde_json::to_value(&relative).expect("diagnostic serializes")
        })
        .collect()
}

fn write_runner_project(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{ "name": "issue-4498", "main": "src/index.ts" }"#,
    );
    write_file(
        root,
        "src/index.ts",
        "import { Runner } from './components/runner/test/Runner';\nexport const app = Runner;\n",
    );
    write_file(
        root,
        "src/components/runner/test/Runner.ts",
        "import { step } from '../step';\nexport const Runner = step;\n",
    );
    write_file(
        root,
        "src/components/runner/step.ts",
        "export const step = 1;\n",
    );
    write_file(root, "src/app.test.ts", "export const unrelated = 1;\n");
}

#[test]
fn a_production_import_of_a_file_in_a_test_directory_names_the_pattern() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_runner_project(&root);

    let mut config = create_config(root);
    config.production = true;
    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    let entries = reported(&config);
    assert_eq!(entries.len(), 1, "one entry per pattern: {entries:?}");
    let entry = &entries[0];
    assert_eq!(entry["pattern"], "**/test/**");
    assert_eq!(entry["file_count"], 1);
    assert_eq!(
        entry["files"],
        serde_json::json!(["src/components/runner/test/Runner.ts"])
    );
    assert_eq!(entry["path"], "src/components/runner/test/Runner.ts");
    let message = entry["message"].as_str().expect("message is a string");
    assert!(
        message.contains("**/test/**") && message.contains("--production"),
        "the message names the pattern and the override: {message}"
    );
}

#[test]
fn a_run_without_production_mode_reports_nothing() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_runner_project(&root);

    let config = create_config(root);
    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(reported(&config).is_empty());
}

#[test]
fn a_dropped_file_that_no_production_file_imports_is_not_reported() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_file(
        &root,
        "package.json",
        r#"{ "name": "issue-4498-quiet", "main": "src/index.ts" }"#,
    );
    write_file(&root, "src/index.ts", "export const app = 1;\n");
    write_file(
        &root,
        "test/helper.ts",
        "import { app } from '../src/index';\nexport const helper = app;\n",
    );

    let mut config = create_config(root);
    config.production = true;
    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(reported(&config).is_empty());
}
