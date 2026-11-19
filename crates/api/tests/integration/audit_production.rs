//! `fallow_api::run_audit` takes its production mode from
//! `AuditOptions::production` and `AnalysisOptions::production_override`.
//! It does not read the legacy `AnalysisOptions::production` bool.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{AnalysisOptions, AuditOptions, run_audit, serialize_audit_programmatic_json};

use crate::common::{commit, git, write};

/// The test file of the head commit. Production mode leaves test files out,
/// so the dead-code section names it only outside production mode.
const TEST_FILE: &str = "src/orphan.test.ts";

fn repository(dir: &Path) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).expect("project dir");
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"audit-production","main":"src/index.ts"}"#,
    );
    write(&root, ".fallowrc.json", r#"{"production": false}"#);
    write(&root, "src/index.ts", "export const ready = true;\n");
    commit(&root, "base");
    write(&root, TEST_FILE, "export const orphan = 1;\n");
    commit(&root, "head");
    root
}

fn dead_code_text(root: &Path, audit_production: bool, analysis_production: bool) -> String {
    let output = run_audit(&AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            production: analysis_production,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_owned()),
        production: audit_production,
        css: Some(false),
        duplication: Some(false),
        health: Some(false),
        ..AuditOptions::default()
    })
    .and_then(serialize_audit_programmatic_json)
    .expect("audit runs");
    let dead_code = &output["dead_code"];
    assert!(
        dead_code["summary"].is_object(),
        "the audit has no dead-code section: {output:#}"
    );
    dead_code.to_string()
}

#[test]
fn audit_ignores_analysis_production_when_audit_production_is_false() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = repository(dir.path());
    let text = dead_code_text(&root, false, true);
    assert!(text.contains(TEST_FILE), "{text}");
}

#[test]
fn audit_production_turns_production_mode_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = repository(dir.path());
    let text = dead_code_text(&root, true, false);
    assert!(!text.contains(TEST_FILE), "{text}");
}
