#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! The fixture declares its workspaces only through the config key
//! `workspaces.patterns`. The root `package.json` has no `workspaces` field,
//! so a command that reads only the manifest finds no workspace.

use crate::common::{CommandOutput, parse_json, run_fallow};

const FIXTURE: &str = "workspaces-config-patterns";
const SELECTED_FILE: &str = "apps/deep/one/src/index.ts";

fn run_scoped(subcommand: &str) -> CommandOutput {
    run_fallow(
        subcommand,
        FIXTURE,
        &[
            "--workspace",
            "@test/one",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    )
}

fn scoped_json(subcommand: &str) -> serde_json::Value {
    let output = run_scoped(subcommand);
    assert_eq!(
        output.code, 0,
        "`{subcommand} --workspace` must find the workspace that the config declares: {}",
        output.stderr
    );
    parse_json(&output)
}

fn paths(items: &serde_json::Value) -> Vec<&str> {
    items
        .as_array()
        .expect("array of findings")
        .iter()
        .filter_map(|item| item["path"].as_str())
        .collect()
}

#[test]
fn flags_workspace_selects_a_workspace_declared_by_config_patterns() {
    let json = scoped_json("flags");
    assert_eq!(paths(&json["feature_flags"]), vec![SELECTED_FILE]);
}

#[test]
fn security_workspace_selects_a_workspace_declared_by_config_patterns() {
    let json = scoped_json("security");
    assert_eq!(paths(&json["security_findings"]), vec![SELECTED_FILE]);
}

#[test]
fn suppressions_workspace_selects_a_workspace_declared_by_config_patterns() {
    let json = scoped_json("suppressions");
    assert_eq!(paths(&json["files"]), vec![SELECTED_FILE]);
}

#[test]
fn health_workspace_selects_a_workspace_declared_by_config_patterns() {
    let json = scoped_json("health");
    assert_eq!(json["summary"]["files_analyzed"], 1);
}

fn package_group_keys(subcommand: &str) -> Vec<String> {
    let output = run_fallow(
        subcommand,
        FIXTURE,
        &[
            "--group-by",
            "package",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(
        output.code, 0,
        "`{subcommand} --group-by package` must use the workspaces that the config declares: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let mut keys: Vec<String> = json["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .filter_map(|group| group["key"].as_str().map(str::to_owned))
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn dead_code_group_by_package_uses_workspaces_declared_by_config_patterns() {
    assert_eq!(
        package_group_keys("dead-code"),
        vec!["@test/one", "@test/two"]
    );
}

#[test]
fn health_group_by_package_uses_workspaces_declared_by_config_patterns() {
    assert_eq!(package_group_keys("health"), vec!["@test/one", "@test/two"]);
}
