//! Attribution of `fallow_api::run_audit`, the audit behind the MCP typed
//! route. It must agree with `fallow audit`: a finding that moved with a
//! renamed file is inherited, and dependency findings are in scope only when
//! their manifest changed.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use fallow_api::{
    AnalysisOptions, AuditGate, AuditOptions, run_audit, serialize_audit_programmatic_json,
};
use serde_json::Value;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fallow",
            "-c",
            "user.email=fallow@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

fn audit(root: &Path) -> Value {
    let options = AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_string()),
        gate: AuditGate::NewOnly,
        ..AuditOptions::default()
    };
    run_audit(&options)
        .and_then(serialize_audit_programmatic_json)
        .expect("run the programmatic audit")
}

/// The `introduced` flags of every item in one dead-code array, by symbol.
fn introduced_by_name(report: &Value, kind: &str, field: &str) -> Vec<(String, bool)> {
    report["dead_code"][kind]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            (
                item[field].as_str().unwrap_or_default().to_string(),
                item["introduced"].as_bool().unwrap_or(true),
            )
        })
        .collect()
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    dir
}

#[test]
fn a_finding_in_a_renamed_file_is_inherited() {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"rename-fixture","private":true,"main":"src/index.ts"}"#,
    );
    write(
        &root,
        "src/index.ts",
        "import { used } from \"./util\";\nconsole.log(used);\n",
    );
    write(
        &root,
        "src/util.ts",
        "export const used = 1;\nexport const oldUnused = 2;\n",
    );
    commit(&root, "base");
    git(&root, &["mv", "src/util.ts", "src/helpers.ts"]);
    write(
        &root,
        "src/index.ts",
        "import { used } from \"./helpers\";\nconsole.log(used);\n",
    );
    commit(&root, "rename");

    let report = audit(&root);

    assert_eq!(
        introduced_by_name(&report, "unused_exports", "export_name"),
        vec![("oldUnused".to_string(), false)],
        "{report:#}"
    );
    assert_eq!(
        report["attribution"]["dead_code_introduced"], 0,
        "{report:#}"
    );
    assert_eq!(report["verdict"], "pass", "{report:#}");
}

fn dependency_repository() -> tempfile::TempDir {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"dep-fixture","private":true,"main":"src/index.ts","dependencies":{"left-pad":"1.0.0"}}"#,
    );
    write(&root, "src/index.ts", "export {};\n");
    write(&root, "src/util.ts", "export const value = 1;\n");
    commit(&root, "base");
    dir
}

#[test]
fn a_dependency_finding_of_an_unchanged_manifest_is_out_of_scope() {
    let dir = dependency_repository();
    let root = dir.path().join("project");
    write(
        &root,
        "src/util.ts",
        "export const value = 1;\nexport const other = 2;\n",
    );
    commit(&root, "edit a source file");

    let report = audit(&root);

    assert_eq!(
        introduced_by_name(&report, "unused_dependencies", "package_name"),
        Vec::<(String, bool)>::new(),
        "{report:#}"
    );
}

#[test]
fn a_dependency_finding_of_a_changed_manifest_is_in_scope() {
    let dir = dependency_repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"dep-fixture","private":true,"description":"changed","main":"src/index.ts","dependencies":{"left-pad":"1.0.0"}}"#,
    );
    commit(&root, "edit the manifest");

    let report = audit(&root);

    assert_eq!(
        introduced_by_name(&report, "unused_dependencies", "package_name"),
        vec![("left-pad".to_string(), false)],
        "{report:#}"
    );
}
