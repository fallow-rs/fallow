//! `run_audit` reports the unused code that a change creates outside the
//! changed files, as `fallow audit --gate new-only` does.

use std::path::Path;
use std::process::Command;

use fallow_config::AuditGate;

use super::run_audit;
use crate::{AnalysisOptions, AuditOptions, AuditVerdict};

fn git(root: &Path, args: &[&str]) {
    let status = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
        .args(args)
        .current_dir(root)
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed");
}

fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

/// A base commit where `src/index.ts` uses `src/lib.ts`, and `src/old.ts`
/// holds an export that nothing uses.
fn orphan_fixture() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("project");
    let root = project.path();
    write(
        root,
        "package.json",
        r#"{"name":"orphan","type":"module","main":"src/index.ts"}"#,
    );
    write(
        root,
        "src/index.ts",
        "import { used } from './lib';\nimport { kept } from './old';\nconsole.log(used(), kept);\n",
    );
    write(root, "src/lib.ts", "export const used = (): number => 1;\n");
    write(
        root,
        "src/old.ts",
        "export const kept = 1;\nexport const stale = 2;\n",
    );
    git(root, &["init", "-q"]);
    commit(root);
    project
}

fn audit(root: &Path, gate: AuditGate) -> crate::AuditProgrammaticOutput {
    run_audit(&AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD".to_string()),
        gate: Some(gate),
        dead_code: Some(true),
        health: Some(false),
        duplication: Some(false),
        ..AuditOptions::default()
    })
    .expect("audit output")
}

fn unused_file_paths(output: &crate::AuditProgrammaticOutput) -> Vec<String> {
    let dead_code = output.dead_code.as_ref().expect("dead code");
    dead_code
        .output
        .results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&dead_code.root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn new_only_reports_a_file_that_the_change_made_unused() {
    let project = orphan_fixture();
    let root = project.path();
    write(
        root,
        "src/index.ts",
        "import { kept } from './old';\nconsole.log(kept);\n",
    );

    let output = audit(root, AuditGate::NewOnly);

    assert_eq!(output.verdict, AuditVerdict::Fail);
    assert_eq!(unused_file_paths(&output), ["src/lib.ts"]);
    assert_eq!(output.attribution.dead_code_introduced, 1);
}

#[test]
fn new_only_keeps_an_old_unused_export_outside_the_change_out_of_the_report() {
    let project = orphan_fixture();
    let root = project.path();
    write(
        root,
        "src/index.ts",
        "import { used } from './lib';\nimport { kept } from './old';\nconsole.log(used(), kept, 1);\n",
    );

    let output = audit(root, AuditGate::NewOnly);

    assert_eq!(output.verdict, AuditVerdict::Pass);
    let dead_code = output.dead_code.as_ref().expect("dead code");
    assert!(dead_code.output.results.unused_exports.is_empty());
    assert_eq!(output.attribution.dead_code_introduced, 0);
}

#[test]
fn gate_all_keeps_the_report_to_the_changed_files() {
    let project = orphan_fixture();
    let root = project.path();
    write(
        root,
        "src/index.ts",
        "import { kept } from './old';\nconsole.log(kept);\n",
    );

    let output = audit(root, AuditGate::All);

    assert!(unused_file_paths(&output).is_empty());
    let dead_code = output.dead_code.as_ref().expect("dead code");
    assert!(dead_code.output.results.unused_exports.is_empty());
}

#[test]
fn new_only_reports_a_file_that_a_config_change_made_unreachable() {
    let project = orphan_fixture();
    let root = project.path();
    write(
        root,
        ".fallowrc.json",
        r#"{"entry":["src/index.ts","scripts/extra.ts"]}"#,
    );
    write(root, "scripts/extra.ts", "console.log('extra');\n");
    commit(root);
    write(root, ".fallowrc.json", r#"{"entry":["src/index.ts"]}"#);

    let output = audit(root, AuditGate::NewOnly);

    assert_eq!(output.verdict, AuditVerdict::Fail);
    assert_eq!(unused_file_paths(&output), ["scripts/extra.ts"]);
}

#[test]
fn new_only_keeps_old_unused_code_out_when_a_config_change_keeps_the_entries() {
    let project = orphan_fixture();
    let root = project.path();
    write(root, ".fallowrc.json", r#"{"entry":["src/index.ts"]}"#);
    commit(root);
    write(
        root,
        ".fallowrc.json",
        r#"{"entry":["src/index.ts"],"ignorePatterns":["dist/**"]}"#,
    );

    let output = audit(root, AuditGate::NewOnly);

    assert_eq!(output.verdict, AuditVerdict::Pass);
    assert_eq!(output.attribution.dead_code_introduced, 0);
}

#[test]
fn new_only_keeps_old_unused_code_out_when_the_change_turns_a_rule_on() {
    let project = orphan_fixture();
    let root = project.path();
    write(
        root,
        ".fallowrc.json",
        r#"{"rules":{"unused-exports":"off"}}"#,
    );
    commit(root);
    write(
        root,
        ".fallowrc.json",
        r#"{"rules":{"unused-exports":"error"}}"#,
    );

    let output = audit(root, AuditGate::NewOnly);

    assert_eq!(output.verdict, AuditVerdict::Pass);
    assert_eq!(output.attribution.dead_code_introduced, 0);
}
