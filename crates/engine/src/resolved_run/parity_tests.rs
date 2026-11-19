//! Self-tests of the harness. The surface rows test their own translators
//! against the cases. These tests use one fixed translator: a dead-code run
//! on the MCP surface with the case root as the given root.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin};
use serde_json::Value;

use super::{
    Case, Deviation, Expected, RowResult, assert_rows, load_cases, normalize_mcp_cli_argv,
};
use crate::resolved_run::resolve::resolve_run;

const ROW: &str = "mcp";

/// A dead-code run of the case root on the MCP surface. It reads no input of
/// the case.
fn translate(_case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    let mut inputs = RunInputs::new(
        Surface::Mcp,
        env,
        CommandKind::DeadCode,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    inputs.root = Some(Given {
        value: root.to_path_buf(),
        origin: Origin::ToolParam("root"),
    });
    match resolve_run(&inputs) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message()),
    }
}

#[test]
fn every_case_file_parses() {
    let cases = load_cases();
    assert!(cases.len() >= 2, "the matrix has cases");
}

fn inline_case(deviation_reason: &str) -> Case {
    let mut case: Case = serde_json::from_value(serde_json::json!({
        "name": "inline",
        "files": { "package.json": "{}" },
        "inputs": { "mcp": { "tool": "analyze", "params": {} } },
        "expected": { "surface": "<row>" },
    }))
    .expect("inline case");
    case.deviations.insert(
        ROW.to_owned(),
        Deviation {
            reason: deviation_reason.to_owned(),
            expected: BTreeMap::from([(
                "analyses.dead_code.production".to_owned(),
                Expected::Decision {
                    value: Value::Bool(false),
                    source: "default:production-off".to_owned(),
                },
            )]),
        },
    );
    case
}

#[test]
fn a_deviation_needs_a_reason() {
    assert_eq!(
        assert_rows(ROW, &[inline_case("fixed row")], translate),
        Vec::new()
    );
    let failures = assert_rows(ROW, &[inline_case(" ")], translate);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].key, "deviations");
}

#[test]
fn a_wrong_surface_fails_the_row_placeholder() {
    let mut case = inline_case("fixed row");
    case.inputs.insert(
        "api".to_owned(),
        serde_json::json!({ "entry": "dead-code" }),
    );
    let failures = assert_rows("api", &[case], translate);
    assert!(
        failures.iter().any(|failure| failure.key == "surface"),
        "{failures:?}"
    );
}

#[test]
fn a_case_without_the_row_input_fails() {
    let failures = assert_rows("lsp", &[inline_case("fixed row")], translate);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].key, "input");
}

#[test]
fn a_not_expressible_row_needs_a_reason() {
    let mut case = inline_case("fixed row");
    case.inputs.insert(
        "node".to_owned(),
        serde_json::json!({ "not_expressible": "Node has no trace entry point" }),
    );
    case.inputs.insert(
        "vscode".to_owned(),
        serde_json::json!({ "not_expressible": "" }),
    );
    let skip = |_: &Case, _: &Path, _: EnvSnapshot| RowResult::NotExpressible;
    assert_eq!(
        assert_rows("node", std::slice::from_ref(&case), skip),
        Vec::new()
    );
    assert_eq!(assert_rows("vscode", &[case], skip).len(), 1);
}

#[test]
fn the_snapshot_cwd_is_not_the_case_root() {
    let cases = load_cases();
    let _ = assert_rows(ROW, &cases[..1], |_, root, env| {
        assert_ne!(env.cwd(), Some(root));
        assert!(env.cwd().is_some());
        RowResult::Failed("probe".to_owned())
    });
}

#[test]
fn mcp_cli_argv_drops_the_arguments_of_the_server() {
    let argv: Vec<String> = [
        "fallow",
        "dead-code",
        "--format",
        "json",
        "--quiet",
        "--explain",
        "--production",
        "--format=json",
    ]
    .map(str::to_owned)
    .to_vec();
    assert_eq!(
        normalize_mcp_cli_argv(&argv),
        vec!["fallow", "dead-code", "--production"]
    );
    let human: Vec<String> = ["fallow", "--format", "human"].map(str::to_owned).to_vec();
    assert_eq!(normalize_mcp_cli_argv(&human), human);
}
