//! The runner version check of `fallow mutation --plan` and `fallow doctor`.
//!
//! A run stops with `FALLOW_MUTATION_RUNNER_UNSUPPORTED` when the installed
//! Vitest is outside the supported range. The plan and doctor read the
//! installed `package.json` and report the same verdict before a run. The
//! fixture package holds only a `package.json`, so no test needs Node.

use std::fs;
use std::path::Path;

use super::common::{CommandOutput, commit_all, git, parse_json, run_fallow_in_root};

const HELP: &str = "install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range";

/// A git project that declares Vitest, with one source file and one test.
/// `installed` is the version in `node_modules`, or `None` for no install.
fn vitest_project(installed: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"runner-check","private":true,"type":"module","devDependencies":{"vitest":"3.2.4"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(
        root.join("src/add.js"),
        "export const add = (a, b) => a + b;\n",
    )
    .expect("add.js");
    fs::write(
        root.join("src/add.test.js"),
        "import { expect, it } from \"vitest\";\nimport { add } from \"./add.js\";\nit(\"adds\", () => { expect(add(1, 2)).toBe(3); });\n",
    )
    .expect("add.test.js");
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    if let Some(version) = installed {
        fs::create_dir_all(root.join("node_modules/vitest")).expect("vitest");
        fs::write(
            root.join("node_modules/vitest/package.json"),
            format!(r#"{{"name":"vitest","version":"{version}"}}"#),
        )
        .expect("vitest package.json");
    }
    dir
}

fn json_plan(root: &Path) -> (CommandOutput, serde_json::Value) {
    let output = run_fallow_in_root(
        "mutation",
        root,
        &["--all", "--plan", "--format", "json", "--quiet"],
    );
    let value = parse_json(&output);
    (output, value)
}

fn json_doctor(root: &Path) -> (CommandOutput, serde_json::Value) {
    let output = run_fallow_in_root("doctor", root, &["--format", "json", "--quiet"]);
    let value = parse_json(&output);
    (output, value)
}

fn runner_check(doctor: &serde_json::Value) -> &serde_json::Value {
    doctor["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|check| check["id"] == "mutation-runner"))
        .unwrap_or_else(|| panic!("no mutation-runner check: {doctor}"))
}

#[test]
fn plan_json_reports_an_unsupported_vitest_and_still_exits_0() {
    let project = vitest_project(Some("3.2.4"));
    let (output, plan) = json_plan(project.path());
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        plan["runner"],
        serde_json::json!({
            "name": "vitest",
            "package": "vitest",
            "installed_version": "3.2.4",
            "supported_range": ">=4.1.0 <6.0.0",
            "supported": false,
            "message": "vitest 3.2.4 is not supported; the supported range is >=4.1.0 <6.0.0",
            "help": HELP,
        }),
        "{plan}"
    );
}

#[test]
fn plan_json_reports_a_supported_vitest_without_help() {
    let project = vitest_project(Some("4.1.5"));
    let (output, plan) = json_plan(project.path());
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(plan["runner"]["installed_version"], "4.1.5", "{plan}");
    assert_eq!(plan["runner"]["supported"], true, "{plan}");
    assert!(plan["runner"]["message"].is_null(), "{plan}");
    assert!(plan["runner"]["help"].is_null(), "{plan}");
}

#[test]
fn plan_json_reports_a_vitest_that_is_not_installed() {
    let project = vitest_project(None);
    let (output, plan) = json_plan(project.path());
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(plan["runner"]["installed_version"].is_null(), "{plan}");
    assert_eq!(plan["runner"]["supported"], false, "{plan}");
    assert_eq!(
        plan["runner"]["message"], "vitest is not installed; the supported range is >=4.1.0 <6.0.0",
        "{plan}"
    );
}

#[test]
fn plan_human_output_says_that_the_run_cannot_start() {
    let project = vitest_project(Some("3.2.4"));
    let output = run_fallow_in_root("mutation", project.path(), &["--all", "--plan", "--quiet"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.contains(
            "Runner: vitest 3.2.4 is not supported; the supported range is >=4.1.0 <6.0.0. A run stops before the first mutant.\n"
        ),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains(&format!("\n  hint: {HELP}\n")),
        "{}",
        output.stdout
    );

    let supported = vitest_project(Some("4.1.5"));
    let output = run_fallow_in_root(
        "mutation",
        supported.path(),
        &["--all", "--plan", "--quiet"],
    );
    assert!(!output.stdout.contains("Runner:"), "{}", output.stdout);
}

#[test]
fn doctor_warns_about_an_unsupported_vitest_with_the_run_help() {
    let project = vitest_project(Some("3.2.4"));
    let (output, doctor) = json_doctor(project.path());
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(doctor["status"], "warn", "{doctor}");
    let check = runner_check(&doctor);
    assert_eq!(check["status"], "warn", "{doctor}");
    assert_eq!(check["required"], false, "{doctor}");
    assert_eq!(
        check["message"],
        format!(
            "vitest 3.2.4 is not supported; the supported range is >=4.1.0 <6.0.0. A mutation run stops before the first mutant. Fix: {HELP}."
        ),
        "{doctor}"
    );
    assert_eq!(
        check["remediation"]["command"],
        "npm install --save-dev vitest@^4.1.0"
    );
    assert_eq!(check["remediation"]["mutating"], true);

    let human = run_fallow_in_root("doctor", project.path(), &["--quiet"]);
    assert!(
        human
            .stdout
            .contains("mutation-runner: vitest 3.2.4 is not supported"),
        "{}",
        human.stdout
    );
}

#[test]
fn doctor_passes_a_supported_vitest_and_skips_a_project_without_one() {
    let project = vitest_project(Some("4.1.5"));
    let (_, doctor) = json_doctor(project.path());
    let check = runner_check(&doctor);
    assert_eq!(check["status"], "pass", "{doctor}");
    assert!(check.get("remediation").is_none(), "{doctor}");

    let empty = tempfile::tempdir().expect("temp dir");
    fs::write(empty.path().join("package.json"), r#"{"name":"no-runner"}"#).expect("package.json");
    let (_, doctor) = json_doctor(empty.path());
    assert_eq!(runner_check(&doctor)["status"], "skipped", "{doctor}");
}

#[test]
fn doctor_checks_a_vitest_that_a_workspace_package_declares_and_the_root_installs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"monorepo","private":true,"workspaces":["packages/*"]}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("packages/app")).expect("packages/app");
    fs::write(
        root.join("packages/app/package.json"),
        r#"{"name":"app","devDependencies":{"vitest":"3.2.4"}}"#,
    )
    .expect("app package.json");
    fs::create_dir_all(root.join("node_modules/vitest")).expect("vitest");
    fs::write(
        root.join("node_modules/vitest/package.json"),
        r#"{"name":"vitest","version":"3.2.4"}"#,
    )
    .expect("vitest package.json");

    let (_, doctor) = json_doctor(root);
    let check = runner_check(&doctor);
    assert_eq!(check["status"], "warn", "{doctor}");
    assert_eq!(
        check["remediation"]["command"],
        "npm install --save-dev vitest@^4.1.0"
    );
}
