//! The Rust API row of the run parity matrix. The translator is
//! [`resolve_programmatic_run`] on the options of a case, read through a
//! test-local mirror of [`AnalysisOptions`] and [`ProgrammaticEntry`]. An
//! audit case resolves through the steps of `run_audit`.

use std::path::Path;

use fallow_engine::resolved_run::parity::{Case, RowFailure, RowResult, assert_rows, load_cases};
use serde::Deserialize;

use super::*;

const ROW: &str = "api";

/// The `api` input of a case.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiInput {
    entry: String,
    #[serde(default)]
    options: ApiOptions,
}

/// The options of an `api` input: the fields of [`AnalysisOptions`] that
/// change a decision, and the fields of the combined and audit entries.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiOptions {
    #[serde(default)]
    production: bool,
    production_override: Option<bool>,
    #[serde(default)]
    no_package_baselines: bool,
    #[serde(default)]
    no_cache: bool,
    threads: Option<usize>,
    dead_code: Option<bool>,
    duplication: Option<bool>,
    health: Option<bool>,
    #[serde(default)]
    audit_production: bool,
    production_dead_code: Option<bool>,
    production_health: Option<bool>,
    production_dupes: Option<bool>,
    type_aware: Option<bool>,
    base: Option<String>,
}

fn entry(name: &str, options: &ApiOptions) -> Option<ProgrammaticEntry> {
    let entry = match name {
        "dead-code" => ProgrammaticEntry::DeadCode,
        "duplication" => ProgrammaticEntry::Duplication,
        "health" => ProgrammaticEntry::Health,
        "feature-flags" => ProgrammaticEntry::FeatureFlags,
        "trace-path" => ProgrammaticEntry::TracePath,
        "trace-chain" => ProgrammaticEntry::TraceChain,
        "trace-error" => ProgrammaticEntry::TraceError,
        "trace-dependency" => ProgrammaticEntry::TraceDependency,
        "mutation" => ProgrammaticEntry::Mutation,
        "similar-code" => ProgrammaticEntry::SimilarCode,
        "list" => ProgrammaticEntry::List,
        "combined" => ProgrammaticEntry::Combined {
            dead_code: options.dead_code.unwrap_or(true),
            duplication: options.duplication.unwrap_or(true),
            health: options.health.unwrap_or(true),
            production_dead_code: options.production_dead_code,
            production_health: options.production_health,
            production_dupes: options.production_dupes,
        },
        _ => return None,
    };
    Some(entry)
}

/// The audit options of an `audit` input.
fn audit_options(analysis: AnalysisOptions, options: &ApiOptions) -> crate::AuditOptions {
    crate::AuditOptions {
        analysis,
        dead_code: options.dead_code,
        duplication: options.duplication,
        health: options.health,
        production: options.audit_production,
        production_dead_code: options.production_dead_code,
        production_health: options.production_health,
        production_dupes: options.production_dupes,
        base: options.base.clone(),
        ..crate::AuditOptions::default()
    }
}

/// The run of `analysis` for the entry `name`. An audit resolves through the
/// steps of `run_audit`, which fold the audit options into the analysis
/// options first.
fn resolve(
    name: &str,
    analysis: AnalysisOptions,
    options: &ApiOptions,
) -> Result<ResolvedRun, String> {
    let result = if name == "audit" {
        let audit = audit_options(analysis, options);
        let folded = crate::runtime::analysis_options_for_audit(&audit, "HEAD");
        crate::runtime::resolve_audit_run(&audit, &folded)
    } else {
        let Some(entry) = entry(name, options) else {
            return Err(format!("the row has no entry `{name}`"));
        };
        resolve_programmatic_run(&analysis, &entry)
    };
    result.map_err(|err| err.message)
}

/// The translator of the row, with a hook that changes the options before
/// the resolve.
fn translate_with(
    case: &Case,
    root: &Path,
    env: EnvSnapshot,
    change: impl Fn(&mut AnalysisOptions),
) -> RowResult {
    let input = &case.inputs[ROW];
    if input.get("not_expressible").is_some() {
        return RowResult::NotExpressible;
    }
    let input: ApiInput = match serde_json::from_value(input.clone()) {
        Ok(input) => input,
        Err(err) => return RowResult::Failed(format!("the api input does not parse: {err}")),
    };
    let mut options = AnalysisOptions {
        root: Some(root.to_path_buf()),
        production: input.options.production,
        production_override: input.options.production_override,
        no_package_baselines: input.options.no_package_baselines,
        no_cache: input.options.no_cache,
        threads: input.options.threads.or(Some(1)),
        type_aware: crate::TypeAwareOptions {
            enabled: input.options.type_aware,
            ..crate::TypeAwareOptions::default()
        },
        host: Some(HostInputs {
            surface: Surface::Api,
            env,
        }),
        ..AnalysisOptions::default()
    };
    change(&mut options);
    match resolve(&input.entry, options, &input.options) {
        Ok(run) => RowResult::Run(Box::new(run)),
        Err(message) => RowResult::Failed(message),
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
fn the_api_row_passes_every_case() {
    let cases = load_cases();
    assert_eq!(assert_rows(ROW, &cases, translate), Vec::new());
}

#[test]
fn a_translator_with_the_node_surface_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |options| {
            if let Some(host) = options.host.as_mut() {
                host.surface = Surface::Node;
            }
        })
    });
    assert!(
        failed_keys(&failures).contains(&("analyze-production-env", "surface")),
        "{failures:?}"
    );
}

#[test]
fn a_translator_that_reads_the_legacy_production_option_for_audit_fails() {
    let cases = load_cases();
    let failures = assert_rows(ROW, &cases, |case, root, env| {
        translate_with(case, root, env, |options| {
            if options.production {
                options.production_override = Some(true);
            }
        })
    });
    assert!(
        failed_keys(&failures).contains(&(
            "audit-ignores-the-legacy-production-option",
            "analyses.dead_code.production"
        )),
        "{failures:?}"
    );
}
