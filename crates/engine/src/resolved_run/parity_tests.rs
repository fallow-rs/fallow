//! Self-tests of the harness. The translator here is a model of a typed MCP
//! tool: it reads the tool name and the `production` parameter. The cases
//! have only an `mcp` input, so the self-tests prove that the cases catch a
//! wrong command, a wrong surface and a dropped `production` parameter of
//! each tool that the cases name.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin};
use serde_json::Value;

use super::{
    Case, Deviation, Expected, RowFailure, RowResult, assert_rows, load_cases,
    normalize_mcp_cli_argv,
};
use crate::resolved_run::resolve::resolve_run;

const ROW: &str = "mcp";

fn tool_command(tool: &str) -> (CommandKind, SelectionInput) {
    let single = SelectionInput::Single(PlannedAnalysis::DeadCode);
    match tool {
        "analyze" => (CommandKind::DeadCode, single),
        "trace_import_path" => (CommandKind::TracePath, single),
        "audit" => (
            CommandKind::Audit,
            SelectionInput::Options {
                dead_code: None,
                duplication: None,
                health: None,
                origin: Origin::ToolParam("audit"),
            },
        ),
        other => panic!("the model translator has no tool `{other}`"),
    }
}

/// The model translator, with a hook that changes the decided command and
/// surface.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(CommandKind, Surface) -> (CommandKind, Surface),
) -> RowResult {
    translate_params(case, root, env, change, |_| true)
}

/// The model translator, with hooks that change the decided command and
/// surface, and that drop the `production` parameter of a tool.
fn translate_params(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(CommandKind, Surface) -> (CommandKind, Surface),
    reads_production: impl Fn(&str) -> bool,
) -> RowResult {
    let input = &case.inputs[ROW];
    let tool = input["tool"].as_str().unwrap_or_default();
    let (command, selection) = tool_command(tool);
    let (command, surface) = change(command, Surface::Mcp);
    let mut inputs = RunInputs::new(surface, env, command, selection);
    inputs.root = Some(Given {
        value: root.to_path_buf(),
        origin: Origin::ToolParam("root"),
    });
    inputs.threads = Some(Given {
        value: 1,
        origin: Origin::ToolParam("threads"),
    });
    inputs.production = input["params"]["production"]
        .as_bool()
        .filter(|_| reads_production(tool))
        .map(|value| Given {
            value,
            origin: Origin::ToolParam("production"),
        });
    match resolve_run(&inputs) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message()),
    }
}

fn translate(case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    translate_with(case, root, env, |command, surface| (command, surface))
}

fn failed_cases(failures: &[RowFailure]) -> Vec<&str> {
    let mut cases: Vec<&str> = failures
        .iter()
        .map(|failure| failure.case.as_str())
        .collect();
    cases.dedup();
    cases
}

#[test]
fn the_model_translator_passes_every_case() {
    let cases = load_cases();
    assert!(cases.len() >= 2, "the matrix has cases");
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn a_translator_that_maps_every_command_to_dead_code_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |_, surface| {
            (CommandKind::DeadCode, surface)
        })
    });
    let failed = failed_cases(&failures);
    assert!(
        failed.contains(&"trace-import-path-production-env"),
        "{failures:?}"
    );
    assert!(!failed.contains(&"analyze-production-env"), "{failures:?}");
}

#[test]
fn a_translator_that_maps_audit_to_combined_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |command, surface| {
            let command = if command == CommandKind::Audit {
                CommandKind::Combined
            } else {
                command
            };
            (command, surface)
        })
    });
    assert!(
        failures
            .iter()
            .any(|failure| failure.case == "per-analysis-split-env" && failure.key == "command"),
        "{failures:?}"
    );
}

#[test]
fn a_translator_that_drops_the_production_parameter_fails() {
    let cases = load_cases();
    for tool in ["analyze", "trace_import_path"] {
        let failures = assert_rows(ROW, &cases, |case, root, env| {
            translate_params(case, root, env, |c, s| (c, s), |name| name != tool)
        });
        assert!(
            failures
                .iter()
                .any(|failure| failure.key == "analyses.dead_code.production"),
            "{tool}: {failures:?}"
        );
    }
}

#[test]
fn a_translator_with_the_wrong_surface_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |command, _| (command, Surface::Api))
    });
    assert!(
        failures
            .iter()
            .any(|failure| failure.key == "surface" && failure.case == "analyze-production-env"),
        "{failures:?}"
    );
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
        assert_rows(ROW, &[inline_case("MCP model row")], translate),
        Vec::new()
    );
    let failures = assert_rows(ROW, &[inline_case(" ")], translate);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].key, "deviations");
}

#[test]
fn a_case_without_the_row_input_fails() {
    let failures = assert_rows("lsp", &[inline_case("MCP model row")], translate);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].key, "input");
}

#[test]
fn a_not_expressible_row_needs_a_reason() {
    let mut case = inline_case("MCP model row");
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
