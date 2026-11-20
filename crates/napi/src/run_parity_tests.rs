//! The Node row of the run parity matrix. The translator is the option
//! mapping of the exported functions, [`ToApiOptions::to_api`] over
//! `map_common_options`, then [`api::run::resolve_programmatic_run`] with the
//! entry of the Rust API runner that the function calls.

use std::path::Path;

use fallow_api::run::parity::{Case, RowFailure, RowResult, assert_rows, load_cases};
use fallow_api::run::{EnvSnapshot, ProgrammaticEntry, Surface, resolve_programmatic_run};
use serde_json::Value;

use super::*;

const ROW: &str = "node";

/// The options of a `node` input that change a decision, as camelCase keys.
struct NodeInput<'a> {
    root: String,
    options: &'a Value,
}

impl NodeInput<'_> {
    fn bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(Value::as_bool)
    }
}

/// The analysis options and the runner entry of one exported function.
fn analysis_of(
    function: &str,
    input: &NodeInput<'_>,
    env: EnvSnapshot,
) -> Option<napi::Result<(api::AnalysisOptions, ProgrammaticEntry)>> {
    let root = Some(input.root.clone());
    let threads = Some(1);
    let result = match function {
        "detectDeadCode" => DeadCodeOptions {
            root,
            threads,
            production: input.bool("production"),
            no_package_baselines: input.bool("noPackageBaselines"),
            type_aware: input
                .options
                .get("typeAware")
                .map(|type_aware| TypeAwareOptions {
                    enabled: type_aware.get("enabled").and_then(Value::as_bool),
                    ..TypeAwareOptions::default()
                }),
            ..DeadCodeOptions::default()
        }
        .to_api(env)
        .map(|options| (options.analysis, ProgrammaticEntry::DeadCode)),
        "detectDuplication" => DuplicationOptions {
            root,
            threads,
            production: input.bool("production"),
            no_package_baselines: input.bool("noPackageBaselines"),
            ..DuplicationOptions::default()
        }
        .to_api(env)
        .map(|options| (options.analysis, ProgrammaticEntry::Duplication)),
        "computeHealth" => ComplexityOptions {
            root,
            threads,
            production: input.bool("production"),
            ..ComplexityOptions::default()
        }
        .to_api(env)
        .map(|options| (options.analysis, ProgrammaticEntry::Health)),
        "detectFeatureFlags" => FeatureFlagsOptions {
            root,
            threads,
            production: input.bool("production"),
            ..FeatureFlagsOptions::default()
        }
        .to_api(env)
        .map(|options| (options.analysis, ProgrammaticEntry::FeatureFlags)),
        _ => return None,
    };
    Some(result)
}

/// The translator of the row, with a hook that changes the API options
/// before the resolve.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(&mut api::AnalysisOptions),
) -> RowResult {
    let input = &case.inputs[ROW];
    if input.get("not_expressible").is_some() {
        return RowResult::NotExpressible;
    }
    let function = input["function"].as_str().unwrap_or_default();
    let node = NodeInput {
        root: root.display().to_string(),
        options: input.get("options").unwrap_or(&Value::Null),
    };
    let Some(mapped) = analysis_of(function, &node, env) else {
        return RowResult::Failed(format!("the row has no function `{function}`"));
    };
    let (mut analysis, entry) = match mapped {
        Ok(mapped) => mapped,
        Err(err) => return RowResult::Failed(err.reason),
    };
    change(&mut analysis);
    match resolve_programmatic_run(&analysis, &entry) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(err) => RowResult::Failed(err.message),
    }
}

fn translate(case: &Case, root: &Path, env: EnvSnapshot) -> RowResult {
    translate_with(case, root, env, |_| {})
}

fn failed_keys(failures: &[RowFailure]) -> Vec<(&str, &str)> {
    failures
        .iter()
        .map(|failure| (failure.case.as_str(), failure.key.as_str()))
        .collect()
}

#[test]
fn the_node_row_passes_every_case() {
    let cases = load_cases();
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn a_translator_with_the_api_surface_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |analysis| {
            if let Some(host) = analysis.host.as_mut() {
                host.surface = Surface::Api;
            }
        })
    });
    let failed = failed_keys(&failures);
    assert!(
        failed.contains(&("analyze-production-env", "surface")),
        "{failures:?}"
    );
    assert!(
        failed.contains(&("given-production-flag", "analyses.dead_code.production")),
        "{failures:?}"
    );
}

#[test]
fn a_translator_that_drops_an_explicit_production_off_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |analysis| {
            if analysis.production_override == Some(false) {
                analysis.production_override = None;
            }
        })
    });
    assert!(
        failed_keys(&failures)
            .contains(&("explicit-production-off", "analyses.dead_code.production")),
        "{failures:?}"
    );
}
