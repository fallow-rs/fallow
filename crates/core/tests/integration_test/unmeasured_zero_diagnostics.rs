//! Integration tests for the diagnostics that name a zero nothing measured.
//!
//! Three counters used to read as findings: a project with no `node_modules`
//! produced resolution and plugin results as if it were installed, and the
//! boundary and policy detectors reported zero violations when they had never
//! run at all. The distinction that matters is CONFIGURED versus not: a
//! project that sets `boundary-violation: off` chose that silence and can see
//! the choice in `fallow config`, so that case is deliberately not reported.

use std::path::Path;

use fallow_config::{Severity, WorkspaceDiagnostic};

use super::common::create_config_with_cache;

fn diagnostic_ids(root: &Path) -> Vec<String> {
    fallow_config::workspace_diagnostics_for(root)
        .iter()
        .map(|diagnostic: &WorkspaceDiagnostic| diagnostic.kind.id().to_string())
        .collect()
}

fn write_minimal_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "unmeasured-zero", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/index.ts"),
        "export const run = (): string => \"run\";\n",
    )
    .expect("write entry module");
}

#[test]
fn a_project_without_node_modules_is_reported() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_minimal_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("analysis succeeds without node_modules");

    assert!(
        diagnostic_ids(&config.root)
            .iter()
            .any(|id| id == "node-modules-missing"),
        "an uninstalled tree must say so, not silently degrade resolution"
    );
}

#[test]
fn an_installed_project_reports_no_missing_node_modules() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_minimal_project(&root);
    std::fs::create_dir_all(root.join("node_modules")).expect("create node_modules");
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(
        !diagnostic_ids(&config.root)
            .iter()
            .any(|id| id == "node-modules-missing"),
        "an installed tree must not be reported"
    );
}

#[test]
fn unconfigured_boundaries_and_rule_packs_are_reported() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_minimal_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    let ids = diagnostic_ids(&config.root);
    assert!(
        ids.iter().any(|id| id == "boundaries-not-configured"),
        "zero boundary violations from zero boundaries is not a measurement, got {ids:?}"
    );
    assert!(
        ids.iter().any(|id| id == "rule-packs-not-configured"),
        "zero policy violations from zero rule packs is not a measurement, got {ids:?}"
    );
}

#[test]
fn a_disabled_check_is_the_users_own_zero_and_is_not_reported() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_minimal_project(&root);
    let mut config = create_config_with_cache(root, temp.path().join("cache"));
    config.rules.boundary_violation = Severity::Off;
    config.rules.policy_violation = Severity::Off;

    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    let ids = diagnostic_ids(&config.root);
    assert!(
        !ids.iter().any(|id| id == "boundaries-not-configured"),
        "a check the user switched off is already visible in the config, got {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id == "rule-packs-not-configured"),
        "a check the user switched off is already visible in the config, got {ids:?}"
    );
}

/// Write the issue #4430 shape: a server secret reaches a module that a
/// browser bundle could import, but no file declares `"use client"`.
fn write_project_without_client_roots(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "ucr-repro", "private": true, "type": "module", "main": "src/main.js" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/config.js"),
        "export const apiKey = process.env.API_SECRET\n",
    )
    .expect("write config module");
    std::fs::write(
        root.join("src/app.js"),
        "import { apiKey } from './config.js'\n\nexport function App() {\n\treturn apiKey\n}\n",
    )
    .expect("write app module");
    std::fs::write(
        root.join("src/main.js"),
        "import { App } from './app.js'\nconsole.log(App())\n",
    )
    .expect("write entry module");
}

#[test]
fn an_active_client_server_leak_rule_without_client_roots_is_reported() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_project_without_client_roots(&root);
    let mut config = create_config_with_cache(root, temp.path().join("cache"));
    config.rules.security_client_server_leak = Severity::Warn;

    let results = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(results.security_findings.is_empty());
    let ids = diagnostic_ids(&config.root);
    assert!(
        ids.iter()
            .any(|id| id == "client-server-leak-no-client-roots"),
        "zero leaks from zero \"use client\" files is not a measurement, got {ids:?}"
    );
}

#[test]
fn a_client_root_suppresses_the_no_client_roots_diagnostic() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_project_without_client_roots(&root);
    std::fs::write(
        root.join("src/app.js"),
        "'use client'\nimport { apiKey } from './config.js'\n\nexport function App() {\n\treturn apiKey\n}\n",
    )
    .expect("write client module");
    let mut config = create_config_with_cache(root, temp.path().join("cache"));
    config.rules.security_client_server_leak = Severity::Warn;

    let results = fallow_core::analyze(&config).expect("analysis succeeds");

    assert_eq!(results.security_findings.len(), 1);
    let ids = diagnostic_ids(&config.root);
    assert!(
        !ids.iter()
            .any(|id| id == "client-server-leak-no-client-roots"),
        "a project with a \"use client\" file gave the rule a starting point, got {ids:?}"
    );
}

#[test]
fn a_disabled_client_server_leak_rule_is_not_reported() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_project_without_client_roots(&root);
    let mut config = create_config_with_cache(root, temp.path().join("cache"));
    config.rules.security_client_server_leak = Severity::Off;

    let _ = fallow_core::analyze(&config).expect("analysis succeeds");

    let ids = diagnostic_ids(&config.root);
    assert!(
        !ids.iter()
            .any(|id| id == "client-server-leak-no-client-roots"),
        "a rule that is off checks nothing by choice, got {ids:?}"
    );
}
