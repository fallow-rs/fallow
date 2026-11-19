//! The CLI row of the run parity matrix. The translator is the path of the
//! CLI: `Cli::try_parse_from` on the `argv` of a case with `--root` set to
//! the case root, then [`command_run_inputs`], then
//! [`fallow_engine::resolved_run::resolve_run`].

use std::path::Path;

use clap::Parser as _;
use fallow_config::{CommandKind, EnvSnapshot, RunInputs};
use fallow_engine::resolved_run::parity::{Case, RowFailure, RowResult, assert_rows, load_cases};

use super::command_run_inputs;
use crate::cli_args::Cli;

const ROW: &str = "cli";

/// The translator of the row, with a hook that changes the run inputs before
/// the resolve.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(&mut RunInputs),
) -> RowResult {
    let input = &case.inputs[ROW];
    if input.get("not_expressible").is_some() {
        return RowResult::NotExpressible;
    }
    let Some(argv) = input.get("argv").and_then(serde_json::Value::as_array) else {
        return RowResult::Failed("the cli input has no argv".to_owned());
    };
    let mut args: Vec<String> = argv
        .iter()
        .filter_map(|arg| arg.as_str().map(str::to_owned))
        .collect();
    args.push("--root".to_owned());
    args.push(root.display().to_string());
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(err) => return RowResult::Failed(format!("argv {args:?} does not parse: {err}")),
    };
    let Some(mut inputs) = command_run_inputs(&cli, env) else {
        return RowResult::Failed(format!("argv {args:?} loads no config"));
    };
    change(&mut inputs);
    match fallow_engine::resolved_run::resolve_run(&inputs) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message()),
    }
}

fn translate(case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    translate_with(case, root, env, |_| {})
}

fn failed_cases(failures: &[RowFailure]) -> Vec<&str> {
    failures
        .iter()
        .map(|failure| failure.case.as_str())
        .collect()
}

#[test]
fn the_cli_row_passes_every_case() {
    let cases = load_cases();
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn cli_row_fails_on_a_wrong_command_kind() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |inputs| {
            inputs.command = CommandKind::DeadCode;
        })
    });
    assert!(
        failed_cases(&failures).contains(&"trace-import-path-production-env"),
        "{failures:?}"
    );
}

#[test]
fn a_translator_that_drops_the_production_flag_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |inputs| inputs.production = None)
    });
    assert!(
        failed_cases(&failures).contains(&"given-production-flag"),
        "{failures:?}"
    );
}
