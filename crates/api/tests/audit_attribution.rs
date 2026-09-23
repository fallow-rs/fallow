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
    audit_with_gate(root, AuditGate::NewOnly)
}

fn audit_with_gate(root: &Path, gate: AuditGate) -> Value {
    let options = AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_string()),
        gate,
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

/// A repository whose base commit imports `lodash` from `src/a.ts`.
fn used_dependency_repository() -> tempfile::TempDir {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"dep-fixture","private":true,"main":"src/a.ts","dependencies":{"lodash":"4.17.21"}}"#,
    );
    write(
        &root,
        "src/a.ts",
        "import lodash from \"lodash\";\nconsole.log(lodash);\n",
    );
    commit(&root, "base");
    dir
}

/// A source edit that makes a dependency unused, with no manifest edit, is
/// out of audit scope: the audit does not report it and the `new-only` gate
/// does not fail on it. `fallow dead-code` still reports it.
#[test]
fn a_dependency_that_a_source_edit_made_unused_is_out_of_scope() {
    let dir = used_dependency_repository();
    let root = dir.path().join("project");
    write(&root, "src/a.ts", "console.log(1);\n");
    commit(&root, "remove the import");

    let report = audit(&root);

    assert_eq!(
        report["dead_code"]["unused_dependencies"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "{report:#}"
    );
    assert_ne!(report["verdict"], "fail", "{report:#}");
}

#[test]
fn a_dependency_that_a_source_edit_made_unused_is_introduced_when_the_manifest_changed() {
    let dir = used_dependency_repository();
    let root = dir.path().join("project");
    write(&root, "src/a.ts", "console.log(1);\n");
    write(
        &root,
        "package.json",
        r#"{"name":"dep-fixture","private":true,"description":"changed","main":"src/a.ts","dependencies":{"lodash":"4.17.21"}}"#,
    );
    commit(&root, "remove the import and edit the manifest");

    let report = audit(&root);

    assert_eq!(
        introduced_by_name(&report, "unused_dependencies", "package_name"),
        vec![("lodash".to_string(), true)],
        "{report:#}"
    );
    assert_eq!(report["verdict"], "fail", "{report:#}");
}

/// Checks the typed output shape only: the typed output has a
/// `base_snapshot`, and a stale suppression is marked `introduced: false`.
#[test]
fn the_typed_output_keeps_the_base_snapshot() {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"reuse-fixture","private":true,"main":"src/index.ts"}"#,
    );
    write(
        &root,
        "src/index.ts",
        "import { used } from \"./util\";\nconsole.log(used);\n",
    );
    let util = "/** @expected-unused */\nexport const used = 1;\n";
    write(&root, "src/util.ts", util);
    commit(&root, "base");
    write(&root, "src/util.ts", &format!("{util}\n\n"));
    commit(&root, "whitespace");

    let options = AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        base: Some("HEAD~1".to_string()),
        gate: AuditGate::NewOnly,
        ..AuditOptions::default()
    };
    let output = run_audit(&options).expect("run the programmatic audit");
    assert!(output.base_snapshot.is_some());
    let report = serialize_audit_programmatic_json(output).expect("serialize the audit");

    let stale = report["dead_code"]["stale_suppressions"]
        .as_array()
        .expect("stale suppressions array");
    assert_eq!(stale.len(), 1, "{report:#}");
    assert_eq!(stale[0]["introduced"], false, "{report:#}");
}

/// An edit that only removes a `@expected-unused` tag changes no token, but
/// it changes the findings. The head run must not stand in for the base, so
/// the new unused export is introduced and the `new-only` gate fails.
#[test]
fn removing_an_expected_unused_tag_introduces_the_unused_export() {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"tag-fixture","private":true,"main":"src/index.ts"}"#,
    );
    write(
        &root,
        "src/index.ts",
        "import { used } from \"./util\";\nconsole.log(used);\n",
    );
    write(
        &root,
        "src/util.ts",
        "export const used = 1;\n/** @expected-unused */\nexport const x = 1;\n",
    );
    commit(&root, "base");
    write(
        &root,
        "src/util.ts",
        "export const used = 1;\n/** */\nexport const x = 1;\n",
    );
    commit(&root, "remove the tag");

    let report = audit(&root);

    assert_eq!(
        introduced_by_name(&report, "unused_exports", "export_name"),
        vec![("x".to_string(), true)],
        "{report:#}"
    );
    assert_eq!(report["verdict"], "fail", "{report:#}");
}

/// A repository whose head commit adds one unused and one misconfigured
/// dependency override to `package.json`. The config sets the two rules to
/// `base` and a per-file override for `package.json` sets them to `manifest`.
fn dependency_override_repository(base: &str, manifest: &str) -> tempfile::TempDir {
    let dir = repository();
    let root = dir.path().join("project");
    write(
        &root,
        ".fallowrc.json",
        &format!(
            r#"{{
  "rules": {{
    "unused-dependency-overrides": "{base}",
    "misconfigured-dependency-overrides": "{base}"
  }},
  "overrides": [{{
    "files": ["package.json"],
    "rules": {{
      "unused-dependency-overrides": "{manifest}",
      "misconfigured-dependency-overrides": "{manifest}"
    }}
  }}]
}}
"#
        ),
    );
    write(
        &root,
        "package.json",
        r#"{"name":"override-fixture","private":true,"main":"src/index.ts"}"#,
    );
    write(&root, "src/index.ts", "export {};\n");
    commit(&root, "base");
    write(
        &root,
        "package.json",
        r#"{"name":"override-fixture","private":true,"main":"src/index.ts","overrides":{"never-installed":"^1.0.0","":"^1.0.0"}}"#,
    );
    commit(&root, "add dependency overrides");
    dir
}

/// Both gates judge the introduced findings by the same per-file severity.
fn assert_gates_agree(report_all: &Value, report_new_only: &Value, expected: &str) {
    for (gate, report) in [("all", report_all), ("new-only", report_new_only)] {
        for kind in [
            "unused_dependency_overrides",
            "misconfigured_dependency_overrides",
        ] {
            assert_eq!(
                report["dead_code"][kind].as_array().map(Vec::len),
                Some(1),
                "gate {gate} must report one {kind} finding: {report:#}"
            );
        }
        assert_eq!(report["verdict"], expected, "gate {gate}: {report:#}");
    }
}

#[test]
fn a_manifest_override_to_warn_passes_both_gates() {
    let dir = dependency_override_repository("error", "warn");
    let root = dir.path().join("project");

    let report_all = audit_with_gate(&root, AuditGate::All);
    let report_new_only = audit_with_gate(&root, AuditGate::NewOnly);

    assert_gates_agree(&report_all, &report_new_only, "warn");
}

#[test]
fn a_manifest_override_to_error_fails_both_gates() {
    let dir = dependency_override_repository("warn", "error");
    let root = dir.path().join("project");

    let report_all = audit_with_gate(&root, AuditGate::All);
    let report_new_only = audit_with_gate(&root, AuditGate::NewOnly);

    assert_gates_agree(&report_all, &report_new_only, "fail");
}
