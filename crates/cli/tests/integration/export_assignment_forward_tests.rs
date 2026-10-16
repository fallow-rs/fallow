//! `export = x` of an imported binding forwards the import as the default
//! export, and the module object is that binding (issue #3392).

use std::fs;
use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

const FORWARDER: &str = "import api from './lib';\nexport = api;\n";

/// A project where `src/index.ts` consumes `src/fwd.ts`, which forwards the
/// default export of `src/lib.ts` with `export = api`.
fn write_project(root: &Path, consumer: &str) {
    fs::create_dir_all(root.join("src")).expect("create source directory");
    let files: &[(&str, &str)] = &[
        (
            "package.json",
            r#"{"name":"export-assignment-forward","version":"1.0.0","main":"src/index.ts"}"#,
        ),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"esModuleInterop":true}}"#,
        ),
        (
            "src/lib.ts",
            "const api = { run() {}, stop() {} };\nexport default api;\n",
        ),
        ("src/fwd.ts", FORWARDER),
        ("src/index.ts", consumer),
    ];
    for (path, contents) in files {
        fs::write(root.join(path), contents).expect("write fixture file");
    }
}

fn dead_code(root: &Path) -> serde_json::Value {
    parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &["--format", "json", "--quiet", "--no-cache"],
    ))
}

fn unused_export_keys(output: &serde_json::Value) -> Vec<String> {
    output["unused_exports"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|entry| format!("{}:{}", entry["path"], entry["export_name"]))
                .collect()
        })
        .unwrap_or_default()
}

fn assert_traced_from_consumer(root: &Path) {
    let trace = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace",
            "src/lib.ts:default",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    let references = trace["direct_references"]
        .as_array()
        .unwrap_or_else(|| panic!("direct_references array: {trace}"));
    assert!(
        references
            .iter()
            .any(|reference| reference["from_file"] == "src/index.ts"),
        "{trace}"
    );
}

#[test]
fn side_effect_import_of_export_assignment_forwarder_reports_no_export() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_project(root, "import './fwd';\n");

    let output = dead_code(root);
    assert!(unused_export_keys(&output).is_empty(), "{output}");
}

#[test]
fn fix_leaves_export_assignment_forwarder_intact() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_project(root, "import './fwd';\n");

    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_ne!(applied.code, 2, "{}", applied.stderr);
    let applied = parse_json(&applied);
    let export_fixes: Vec<_> = applied["fixes"]
        .as_array()
        .map(|fixes| {
            fixes
                .iter()
                .filter(|fix| fix["type"] == "remove_export")
                .collect()
        })
        .unwrap_or_default();
    assert!(export_fixes.is_empty(), "{applied}");
    assert_eq!(
        fs::read_to_string(root.join("src/fwd.ts")).expect("read forwarder"),
        FORWARDER
    );
}

#[test]
fn destructured_require_of_export_assignment_forwarder_credits_default() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_project(root, "const { run } = require('./fwd');\nrun();\n");

    let output = dead_code(root);
    assert!(unused_export_keys(&output).is_empty(), "{output}");
    assert_traced_from_consumer(root);
}

#[test]
fn named_import_from_export_assignment_forwarder_credits_default() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_project(root, "import { run } from './fwd';\nrun();\n");

    let output = dead_code(root);
    assert!(unused_export_keys(&output).is_empty(), "{output}");
    assert_traced_from_consumer(root);
}
