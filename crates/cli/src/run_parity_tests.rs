//! The CLI row of the run parity matrix. The translator is the path of the
//! CLI: `Cli::try_parse_from` and the `review` alias rewrite on the `argv` of
//! a case with `--root` set to the case root, then [`command_run_inputs`], then
//! [`fallow_engine::resolved_run::resolve_run`].

use std::path::Path;

use clap::Parser as _;
use fallow_config::{CommandKind, EnvSnapshot, RunInputs};
use fallow_engine::resolved_run::parity::{Case, RowFailure, RowResult, assert_rows, load_cases};

use super::command_run_inputs;
use crate::cli_args::Cli;

const ROW: &str = "cli";

/// Parse `args` as `main` parses the process arguments.
fn parse_argv(args: &[String]) -> Result<Cli, clap::Error> {
    let mut cli = Cli::try_parse_from(args)?;
    crate::cli_startup::apply_review_alias(&mut cli, args.iter().cloned());
    Ok(cli)
}

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
    let cli = match parse_argv(&args) {
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

#[test]
fn the_row_reads_the_review_alias_as_the_brief() {
    let argv = ["fallow", "review"].map(str::to_owned);
    let cli = parse_argv(&argv).expect("argv parses");
    let inputs = command_run_inputs(&cli, EnvSnapshot::none()).expect("review loads a config");
    let brief = parse_argv(&["fallow", "audit", "--brief"].map(str::to_owned)).expect("parses");
    let expected = command_run_inputs(&brief, EnvSnapshot::none()).expect("audit loads a config");
    assert_eq!(inputs.command, CommandKind::Audit);
    assert_eq!(inputs.selection, expected.selection);
}

#[test]
fn a_zero_thread_flag_keeps_the_default_thread_count() {
    let cli = Cli::try_parse_from(["fallow", "dead-code", "--threads", "0"]).expect("argv parses");
    let inputs = command_run_inputs(&cli, EnvSnapshot::none()).expect("dead-code loads a config");
    assert_eq!(inputs.threads, None);
    let cli = Cli::try_parse_from(["fallow", "dead-code", "--threads", "3"]).expect("argv parses");
    let inputs = command_run_inputs(&cli, EnvSnapshot::none()).expect("dead-code loads a config");
    assert_eq!(inputs.threads.map(|given| given.value), Some(3));
}
