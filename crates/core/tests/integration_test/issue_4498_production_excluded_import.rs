//! A production test pattern such as `**/test/**` can drop a file that a
//! production file imports (issue #4498). Discovery keeps the patterns as
//! they are, and the dead-code pass records one
//! `production-excluded-file-imported` diagnostic for each pattern that
//! dropped such a file.

use std::fs;
use std::path::Path;

use fallow_config::ResolvedConfig;

use super::common::{create_config, create_config_with_ignore_patterns};

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
        message.contains("**/test/**") && message.contains("--no-production"),
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

/// Analyze `root` in production mode and return the entries of this kind.
fn production_entries(mut config: ResolvedConfig) -> Vec<serde_json::Value> {
    config.production = true;
    let _ = fallow_core::analyze(&config).expect("analysis succeeds");
    reported(&config)
}

#[test]
fn the_message_names_both_production_overrides() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_runner_project(&root);

    let entries = production_entries(create_config(root));

    let message = entries[0]["message"].as_str().expect("message is a string");
    assert!(
        message.contains("--no-production") && message.contains("production: { deadCode: false }"),
        "a config with `production: true` needs the config override too: {message}"
    );
}

#[test]
fn the_sample_is_capped_and_sorted_and_the_count_is_exact() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_file(
        &root,
        "package.json",
        r#"{ "name": "issue-4498-cap", "main": "src/index.ts" }"#,
    );
    let names = ["g", "c", "a", "f", "b", "e", "d"];
    let imports = names
        .map(|name| format!("export * from './test/{name}';"))
        .join("\n");
    write_file(&root, "src/index.ts", &imports);
    for name in names {
        write_file(
            &root,
            &format!("src/test/{name}.ts"),
            &format!("export const {name} = 1;\n"),
        );
    }

    let entries = production_entries(create_config(root));

    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["file_count"], 7);
    assert_eq!(
        entries[0]["files"],
        serde_json::json!([
            "src/test/a.ts",
            "src/test/b.ts",
            "src/test/c.ts",
            "src/test/d.ts",
            "src/test/e.ts"
        ])
    );
    let message = entries[0]["message"].as_str().expect("message is a string");
    assert!(message.contains("and 2 more"), "{message}");
}

#[test]
fn a_dynamic_import_and_a_re_export_both_count() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_file(
        &root,
        "package.json",
        r#"{ "name": "issue-4498-dyn", "main": "src/index.ts" }"#,
    );
    write_file(
        &root,
        "src/index.ts",
        "export { a } from './test/a';\nexport const load = () => import('./test/b');\n",
    );
    write_file(&root, "src/test/a.ts", "export const a = 1;\n");
    write_file(&root, "src/test/b.ts", "export const b = 1;\n");

    let entries = production_entries(create_config(root));

    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(
        entries[0]["files"],
        serde_json::json!(["src/test/a.ts", "src/test/b.ts"])
    );
}

#[test]
fn a_file_that_ignore_patterns_excludes_is_not_counted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_runner_project(&root);

    let entries = production_entries(create_config_with_ignore_patterns(
        root,
        &["src/components/runner/test/**"],
    ));

    assert!(
        entries.is_empty(),
        "the project chose this exclusion: {entries:?}"
    );
}

/// A workspace package whose entry is in a `test` directory. The import of the
/// package name resolves to no file path, so the pass reads the entry fields of
/// the package manifest.
fn write_workspace_project(root: &Path, app_source: &str) {
    write_file(
        root,
        "package.json",
        r#"{ "name": "issue-4498-ws", "private": true, "workspaces": ["packages/*"] }"#,
    );
    write_file(
        root,
        "packages/app/package.json",
        r#"{ "name": "@c/app", "main": "src/index.ts", "dependencies": { "@c/lib": "workspace:*" } }"#,
    );
    write_file(root, "packages/app/src/index.ts", app_source);
    write_file(
        root,
        "packages/lib/package.json",
        r#"{ "name": "@c/lib", "main": "test/index.ts" }"#,
    );
    write_file(
        root,
        "packages/lib/test/index.ts",
        "import { helper } from '../src/helper';\nexport const lib = helper;\n",
    );
    write_file(
        root,
        "packages/lib/src/helper.ts",
        "export const helper = 1;\n",
    );
}

#[test]
fn a_workspace_package_entry_in_a_test_directory_is_reported() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_workspace_project(
        &root,
        "import { lib } from '@c/lib';\nexport const app = lib;\n",
    );

    let entries = production_entries(create_config(root));

    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["pattern"], "**/test/**");
    assert_eq!(entries[0]["file_count"], 1);
    assert_eq!(
        entries[0]["files"],
        serde_json::json!(["packages/lib/test/index.ts"])
    );
}

#[test]
fn one_file_reached_by_two_imports_counts_once() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("project");
    write_workspace_project(
        &root,
        "import { lib } from '@c/lib';\nimport { lib as direct } from '../../lib/test/index';\nexport const app = [lib, direct];\n",
    );

    let entries = production_entries(create_config(root));

    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["file_count"], 1, "{entries:?}");
}
