//! Unresolved-import findings anchor on the source specifier, and one
//! suppression above the owning statement covers the whole statement.
//!
//! A multi-line re-export extracts one edge per binding line. Anchoring the
//! deduped finding on a binding line made it impossible to reach zero issues
//! with a single suppression: the comment above the statement never matched
//! the reported line, so the finding survived while the comment was reported
//! stale.

use std::fs;
use std::path::Path;

use super::common::create_config;

const MULTI_LINE_RE_EXPORT: &str = r#"export type {
  Alpha,
  Beta,
  Gamma,
} from "./missing.js";
"#;

fn write_project(root: &Path, index_source: &str) {
    fs::create_dir_all(root.join("src")).expect("create src dir");
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "unresolved-import-anchor",
  "main": "src/index.ts"
}"#,
    )
    .expect("write package.json");
    fs::write(root.join("src/index.ts"), index_source).expect("write source");
}

#[test]
fn multi_line_re_export_finding_anchors_on_the_specifier_line() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    write_project(tmp.path(), MULTI_LINE_RE_EXPORT);

    let config = create_config(tmp.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let findings: Vec<_> = results
        .unresolved_imports
        .iter()
        .filter(|u| u.import.specifier == "./missing.js")
        .collect();
    assert_eq!(
        findings.len(),
        1,
        "one finding for the whole statement, found: {findings:?}"
    );
    assert_eq!(
        findings[0].import.line, 5,
        "the finding anchors on the specifier line, not the first binding line"
    );
}

#[test]
fn one_suppression_above_the_statement_yields_zero_issues_and_zero_stale() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let source = format!("// fallow-ignore-next-line unresolved-import\n{MULTI_LINE_RE_EXPORT}");
    write_project(tmp.path(), &source);

    let config = create_config(tmp.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "one suppression above the statement should cover the whole statement, found: {:?}",
        results.unresolved_imports
    );
    assert!(
        results.stale_suppressions.is_empty(),
        "the consumed suppression must not be reported stale, found: {:?}",
        results.stale_suppressions
    );
}

#[test]
fn single_line_unresolved_import_line_and_suppression_unchanged() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    write_project(
        tmp.path(),
        "import { x } from \"./nonexistent\";\nexport const main = () => x;\n",
    );

    let config = create_config(tmp.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let findings: Vec<_> = results
        .unresolved_imports
        .iter()
        .filter(|u| u.import.specifier == "./nonexistent")
        .collect();
    assert_eq!(findings.len(), 1, "found: {findings:?}");
    assert_eq!(findings[0].import.line, 1);

    let tmp = tempfile::tempdir().expect("create temp dir");
    write_project(
        tmp.path(),
        "// fallow-ignore-next-line unresolved-import\nimport { x } from \"./nonexistent\";\nexport const main = () => x;\n",
    );

    let config = create_config(tmp.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "found: {:?}",
        results.unresolved_imports
    );
    assert!(
        results.stale_suppressions.is_empty(),
        "found: {:?}",
        results.stale_suppressions
    );
}
