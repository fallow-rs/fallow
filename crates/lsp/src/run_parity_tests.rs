//! The language server row of the run parity matrix. The translator is the
//! path of the server: [`parse_initialization_options`] on the
//! `initializationOptions` of a case, then [`run_inputs`] for the case root,
//! then [`fallow_api::run::resolve_run`].

use std::path::Path;

use fallow_api::run::parity::{Case, RowFailure, RowResult, assert_rows, load_cases};
use fallow_config::EnvSnapshot;

use super::run_inputs;
use crate::initialization::{LspInitializationOptions, parse_initialization_options};

const ROW: &str = "lsp";

/// The translator of the row, with hooks that change the parsed options and
/// the workspace root before the translation.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(&mut LspInitializationOptions),
    workspace_root: impl Fn(&Path, &EnvSnapshot) -> std::path::PathBuf,
) -> RowResult {
    let input = &case.inputs[ROW];
    if input.get("not_expressible").is_some() {
        return RowResult::NotExpressible;
    }
    let Some(options) = input.get("initializationOptions") else {
        return RowResult::Failed("the lsp input has no initializationOptions".to_owned());
    };
    let mut parsed = parse_initialization_options(Some(options));
    change(&mut parsed);
    let workspace_root = workspace_root(root, &env);
    let inputs = run_inputs(&parsed, &workspace_root, env);
    match fallow_api::run::resolve_run(&inputs) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message()),
    }
}

fn translate(case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    translate_with(case, root, env, |_| {}, |root, _| root.to_path_buf())
}

fn failed_cases(failures: &[RowFailure]) -> Vec<&str> {
    failures
        .iter()
        .map(|failure| failure.case.as_str())
        .collect()
}

#[test]
fn the_lsp_row_passes_every_case() {
    let cases = load_cases();
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn a_translator_that_drops_the_production_setting_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(
            case,
            root,
            env,
            |options| options.production = None,
            |root, _| root.to_path_buf(),
        )
    });
    assert!(
        failed_cases(&failures).contains(&"given-production-flag"),
        "{failures:?}"
    );
}

#[test]
fn a_translator_that_resolves_from_the_process_cwd_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(
            case,
            root,
            env,
            |_| {},
            |root, env| env.cwd().unwrap_or(root).to_path_buf(),
        )
    });
    assert!(
        failed_cases(&failures).contains(&"editor-relative-config-path"),
        "{failures:?}"
    );
}
