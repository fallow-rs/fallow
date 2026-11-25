#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix --production` and exports that test-side files import
//! (issue #5251).
//!
//! A production run leaves out test, story, fixture and bench files, so an
//! export that only these files import is unused in production. The finding
//! stays, but the removal of the `export` keyword breaks the import in the
//! test-side file. The fixer keeps such an export with
//! `skip_reason: "test_importers"`. An import from a file that no entry
//! point reaches does not count. A helper below `src/` that only tests import
//! is not excluded in production, so its import still credits the export, and
//! the export is not reported at all.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

const SOURCE: &str = "export const a = () => 1;\n\
export const onlyTest = () => 2;\n\
export const storyOnly = () => 3;\n\
export const fixtureOnly = () => 4;\n\
export const helperOnly = () => 5;\n\
export const orphanOnly = () => 6;\n\
export const unused = () => 7;\n\
export const factoryOnly = () => 8;\n";

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn write_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{
  "name": "t",
  "main": "src/index.ts",
  "devDependencies": { "vitest": "^3.0.0", "storybook": "^8.0.0", "@storybook/react": "^8.0.0" }
}"#,
    );
    write(
        root,
        ".storybook/main.ts",
        "export default { stories: ['../src/**/*.stories.tsx'] };\n",
    );
    write(root, "src/index.ts", "export { a } from \"./a\";\n");
    write(root, "src/a.ts", SOURCE);
    write(
        root,
        "test/a.test.ts",
        "import { onlyTest } from \"../src/a\";\n\
         import { helper } from \"./helpers\";\n\
         import { data } from \"../src/__fixtures__/data\";\n\
         import { make } from \"../src/testing/factory\";\n\
         onlyTest();\nhelper();\nmake();\nconsole.log(data);\n",
    );
    write(
        root,
        "test/helpers.ts",
        "import { helperOnly } from \"../src/a\";\nexport const helper = () => helperOnly();\n",
    );
    write(
        root,
        "src/__fixtures__/data.ts",
        "import { fixtureOnly } from \"../a\";\nexport const data = fixtureOnly();\n",
    );
    write(
        root,
        "src/testing/factory.ts",
        "import { factoryOnly } from \"../a\";\nexport const make = () => factoryOnly();\n",
    );
    write(
        root,
        "src/Button.stories.tsx",
        "import { storyOnly } from \"./a\";\nexport default { title: 'Button' };\nexport const Primary = () => storyOnly();\n",
    );
    write(
        root,
        "test/orphan-helper.ts",
        "import { orphanOnly } from \"../src/a\";\nexport const orphan = () => orphanOnly();\n",
    );
}

fn export_entries(envelope: &serde_json::Value) -> Vec<(String, Option<String>)> {
    let mut entries: Vec<(String, Option<String>)> = envelope["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_export" && fix["path"] == "src/a.ts")
        .map(|fix| {
            (
                fix["name"].as_str().unwrap().to_string(),
                fix["skip_reason"].as_str().map(str::to_string),
            )
        })
        .collect();
    entries.sort();
    entries
}

#[test]
fn fix_production_keeps_exports_that_test_side_files_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root(
        "fix",
        root,
        &["--production", "--dry-run", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let envelope = parse_json(&output);
    let kept = Some("test_importers".to_string());
    assert_eq!(
        export_entries(&envelope),
        [
            ("fixtureOnly".to_string(), kept.clone()),
            ("helperOnly".to_string(), kept.clone()),
            ("onlyTest".to_string(), kept.clone()),
            ("orphanOnly".to_string(), None),
            ("storyOnly".to_string(), kept),
            ("unused".to_string(), None),
        ],
        "the fixer keeps each export that a reachable test-side file imports: {envelope:#}"
    );
}

#[test]
fn fix_production_leaves_test_imported_exports_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root(
        "fix",
        root,
        &["--production", "--yes", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let fixed = std::fs::read_to_string(root.join("src/a.ts")).unwrap();
    for name in [
        "onlyTest",
        "storyOnly",
        "fixtureOnly",
        "helperOnly",
        "factoryOnly",
    ] {
        assert!(
            fixed.contains(&format!("export const {name} =")),
            "`{name}` keeps its export:\n{fixed}"
        );
    }
    for name in ["orphanOnly", "unused"] {
        assert!(
            fixed.contains(&format!("\nconst {name} =")),
            "`{name}` loses its export:\n{fixed}"
        );
    }
}

#[test]
fn dead_code_production_reports_test_importers_on_the_finding() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &["--production", "--format", "json", "--quiet"],
    );
    let envelope = parse_json(&output);
    let findings = envelope["unused_exports"].as_array().unwrap();
    let finding = |name: &str| {
        findings
            .iter()
            .find(|finding| finding["export_name"] == name)
            .unwrap_or_else(|| panic!("no finding for {name}: {envelope:#}"))
    };

    let only_test = finding("onlyTest");
    let action = &only_test["actions"][0];
    assert_eq!(action["type"], "remove-export");
    assert_eq!(action["auto_fixable"], false);
    assert!(
        action["note"]
            .as_str()
            .is_some_and(|note| note.contains("test")),
        "the note gives the reason: {action:#}"
    );
    assert_eq!(
        only_test["test_importers"],
        serde_json::json!({ "files": ["test/a.test.ts"], "remaining": 0 })
    );

    let unused = finding("unused");
    assert_eq!(unused["actions"][0]["auto_fixable"], true);
    assert!(unused.get("test_importers").is_none());
    assert!(
        findings
            .iter()
            .all(|finding| finding["export_name"] != "factoryOnly"),
        "a helper below `src/` is not excluded in production, so its import credits the export"
    );
    let orphan = finding("orphanOnly");
    assert_eq!(orphan["actions"][0]["auto_fixable"], true);
    assert!(orphan.get("test_importers").is_none());
}

#[test]
fn dead_code_without_production_has_no_test_importers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    let envelope = parse_json(&output);
    for finding in envelope["unused_exports"].as_array().unwrap() {
        assert!(finding.get("test_importers").is_none(), "{finding:#}");
    }
}

#[test]
fn the_scan_adds_no_diagnostic_of_a_test_file_to_the_production_report() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);
    write(
        root,
        "test/broken.test.ts",
        "import { onlyTest } from \"../src/a\";\nconst = ;\n",
    );

    let output = run_fallow_in_root("dead-code", root, &["--production", "--format", "json"]);
    let envelope = parse_json(&output);
    let diagnostics = envelope["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.to_string().contains("broken.test.ts")),
        "production mode excludes the test file, so the report names none of it: {diagnostics:#?}"
    );
    assert!(
        !output.stderr.contains("broken.test.ts"),
        "the scan prints no warning: {}",
        output.stderr
    );

    let normal = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    assert!(
        parse_json(&normal).to_string().contains("broken.test.ts"),
        "the fixture must degrade the parse of the test file in a normal run"
    );
}

/// A test reaches the export through a barrel, as a default export, through a
/// namespace import, or as a type only. Each form counts as a test import.
#[test]
fn every_import_form_of_a_test_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "t", "main": "src/index.ts", "devDependencies": { "vitest": "^3.0.0" } }"#,
    );
    write(root, "src/index.ts", "export { used } from \"./b\";\n");
    write(
        root,
        "src/b.ts",
        "export const used = 1;\n\
         export const viaBarrel = 2;\n\
         export const viaNamespace = 3;\n\
         export type OnlyType = { a: number };\n\
         export default function viaDefault() { return 4; }\n",
    );
    write(root, "src/barrel.ts", "export * from \"./b\";\n");
    write(
        root,
        "test/b.test.ts",
        "import viaDefault from \"../src/b\";\n\
         import * as b from \"../src/b\";\n\
         import { viaBarrel } from \"../src/barrel\";\n\
         import type { OnlyType } from \"../src/b\";\n\
         const value: OnlyType = { a: viaDefault() + b.viaNamespace + viaBarrel };\n\
         console.log(value);\n",
    );

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &["--production", "--format", "json", "--quiet"],
    );
    let envelope = parse_json(&output);
    let importers = |key: &str, name: &str| -> serde_json::Value {
        envelope[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["path"] == "src/b.ts" && finding["export_name"] == name)
            .unwrap_or_else(|| panic!("no {key} finding for {name}: {envelope:#}"))["test_importers"]
            .clone()
    };
    let expected = serde_json::json!({ "files": ["test/b.test.ts"], "remaining": 0 });
    for name in ["viaBarrel", "viaNamespace", "default"] {
        assert_eq!(importers("unused_exports", name), expected, "{name}");
    }
    assert_eq!(importers("unused_types", "OnlyType"), expected);
    let type_finding = envelope["unused_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["export_name"] == "OnlyType")
        .unwrap();
    assert_eq!(type_finding["actions"][0]["auto_fixable"], false);
}
