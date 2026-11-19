#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! The typed route and the CLI route of one MCP tool read the
//! `FALLOW_PRODUCTION*` variables of the server process with the precedence
//! of the CLI: the tool parameter, then the variable of the analysis, then
//! `FALLOW_PRODUCTION`, then the config.

use std::path::Path;

use crate::typed_route_env_coverage::McpServer;

/// The findings that production mode removes: a test file that nothing
/// imports and a dev dependency that nothing uses.
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"production-env","private":true,"main":"src/index.ts","devDependencies":{"unused-dev-tool":"1.0.0"}}"#,
    )
    .expect("package");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("index");
    std::fs::write(
        root.join("src/orphan.test.ts"),
        "export const helper = 2;\n",
    )
    .expect("test file");
}

/// The paths of the unused files and the names of the unused dev
/// dependencies that one `analyze` call reports.
fn production_findings(
    server: &mut McpServer,
    root: &Path,
    extra: &serde_json::Value,
) -> (Vec<String>, Vec<String>) {
    let mut arguments = serde_json::json!({"root": root.display().to_string(), "no_cache": true});
    for (key, value) in extra.as_object().expect("object") {
        arguments[key] = value.clone();
    }
    let result = server.call_raw("analyze", &arguments);
    assert_ne!(result["isError"], true, "analyze must succeed: {result}");
    let text = result["content"][0]["text"].as_str().expect("text content");
    let envelope: serde_json::Value = serde_json::from_str(text).expect("analyze payload");
    let names = |key: &str, field: &str| -> Vec<String> {
        envelope[key]
            .as_array()
            .expect("finding array")
            .iter()
            .filter_map(|finding| finding[field].as_str().map(str::to_owned))
            .collect()
    };
    (
        names("unused_files", "path"),
        names("unused_dev_dependencies", "package_name"),
    )
}

/// The same call on the typed route and on the CLI route. A baseline save
/// routes `analyze` to the CLI and does not change the findings.
fn both_routes(env: &[(&str, &str)], extra: &serde_json::Value) -> [(Vec<String>, Vec<String>); 2] {
    let project = tempfile::tempdir().expect("project dir");
    write_project(project.path());
    let scratch = tempfile::tempdir().expect("scratch dir");
    let baseline = scratch.path().join("baseline.json");
    let mut cli_extra = extra.clone();
    cli_extra["save_baseline"] = serde_json::json!(baseline.display().to_string());

    let mut server = McpServer::start_with_env(env);
    let typed = production_findings(&mut server, project.path(), extra);
    let cli = production_findings(&mut server, project.path(), &cli_extra);
    assert!(
        baseline.is_file(),
        "the second call must take the CLI route"
    );
    [typed, cli]
}

const NO_FINDINGS: (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());

#[test]
fn analyze_routes_read_fallow_production() {
    let [typed, cli] = both_routes(&[("FALLOW_PRODUCTION", "1")], &serde_json::json!({}));
    assert_eq!(cli, NO_FINDINGS, "the CLI route runs in production mode");
    assert_eq!(
        typed, cli,
        "the typed route must read FALLOW_PRODUCTION too"
    );
}

#[test]
fn analyze_routes_read_the_variable_of_the_analysis_before_the_global_one() {
    let [typed, cli] = both_routes(
        &[
            ("FALLOW_PRODUCTION", "0"),
            ("FALLOW_PRODUCTION_DEAD_CODE", "true"),
        ],
        &serde_json::json!({}),
    );
    assert_eq!(
        cli, NO_FINDINGS,
        "the dead-code variable wins on the CLI route"
    );
    assert_eq!(typed, cli, "the typed route gives the same precedence");
}

/// An ordering guard, not a regression test: a typed route that reads no
/// variable also passes. It fails when a route puts the variable before the
/// tool parameter.
#[test]
fn analyze_routes_let_the_tool_parameter_win_over_the_environment() {
    let [typed, cli] = both_routes(
        &[("FALLOW_PRODUCTION", "1")],
        &serde_json::json!({"production": false}),
    );
    assert_eq!(
        cli,
        (
            vec!["src/orphan.test.ts".to_owned()],
            vec!["unused-dev-tool".to_owned()]
        ),
        "production: false wins over the variable on the CLI route"
    );
    assert_eq!(typed, cli, "the typed route gives the same precedence");
}

/// `files_analyzed` of one `check_health` call. A test file holds the only
/// function, so production mode analyzes one file less.
fn health_files_analyzed(server: &mut McpServer, root: &Path, baseline: Option<&Path>) -> u64 {
    let mut arguments = serde_json::json!({"root": root.display().to_string(), "no_cache": true});
    if let Some(baseline) = baseline {
        arguments["save_baseline"] = serde_json::json!(baseline.display().to_string());
    }
    let result = server.call_raw("check_health", &arguments);
    let text = result["content"][0]["text"].as_str().expect("text content");
    let envelope: serde_json::Value = serde_json::from_str(text).expect("health payload");
    envelope["summary"]["files_analyzed"]
        .as_u64()
        .unwrap_or_else(|| panic!("files_analyzed: {envelope}"))
}

#[test]
fn check_health_routes_read_fallow_production_health() {
    let project = tempfile::tempdir().expect("project dir");
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"production-health-env","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("index");
    std::fs::write(
        root.join("src/branchy.test.ts"),
        "export function branchy(a: number) { if (a > 1) { return 1; } return 2; }\n",
    )
    .expect("test file");
    let scratch = tempfile::tempdir().expect("scratch dir");
    let baseline = scratch.path().join("health-baseline.json");

    let mut server = McpServer::start_with_env(&[("FALLOW_PRODUCTION_HEALTH", "on")]);
    let typed = health_files_analyzed(&mut server, root, None);
    let cli = health_files_analyzed(&mut server, root, Some(&baseline));
    assert!(
        baseline.is_file(),
        "the second call must take the CLI route"
    );
    assert_eq!(cli, 1, "the CLI route leaves the test file out");
    assert_eq!(
        typed, cli,
        "the typed route must read FALLOW_PRODUCTION_HEALTH too"
    );
}
