//! The unused code of the base side under the config of the base commit.

use std::path::{Path, PathBuf};

use fallow_config::{ProductionAnalysis, ResolvedConfig, Severity};
use fallow_engine::project_config::ProjectConfigOptions;
use fallow_types::output_format::OutputFormat;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::{AnalysisOptions, AuditOptions, TypeAwareOptions};

/// The inputs of one base pass under the config of the base commit.
pub(super) struct BaseOwnConfigInput<'a> {
    pub(super) options: &'a AuditOptions,
    /// The analysis options of the base pass with the head config.
    pub(super) base_analysis: &'a AnalysisOptions,
    pub(super) head_root: &'a Path,
    /// The config file of the head run.
    pub(super) head_config_path: Option<&'a Path>,
    pub(super) base_root: &'a Path,
    /// The changed files, spelled under `base_root`.
    pub(super) focus: Option<&'a FxHashSet<PathBuf>>,
    /// The dead-code results of the base pass with the head config.
    pub(super) base_results: &'a AnalysisResults,
}

/// The keys of the unused files and unused exports of the base under the
/// config of the base commit (see
/// [`crate::audit_run::AuditKeySnapshot::own_config_unused_code`]).
///
/// This second dead-code pass runs only when the change touched the config
/// file of the head run, as in `fallow audit`. It returns `None` when the
/// change did not touch it, when the base config does not load or the pass
/// fails, and when the base config turns the `unused-files` or
/// `unused-exports` rule off. Then the base pass with the head config is the
/// only evidence. A finding of that pass in code that the base config ignores
/// also counts as unused under the base config (see
/// [`crate::audit_run::own_config_unused_code_keys`]).
pub(super) fn base_own_config_unused_code(
    input: &BaseOwnConfigInput<'_>,
) -> Option<FxHashSet<String>> {
    let focus = input.focus?;
    let relative =
        crate::audit_run::config_path_under_root(input.head_config_path?, input.head_root)?;
    if !focus.contains(&input.base_root.join(relative)) {
        return None;
    }
    let base_config_path = fallow_config::FallowConfig::find_config_path(input.base_root);
    let own_config = base_config_reporting_unused_code(input, base_config_path.as_deref())?;
    let analysis = AnalysisOptions {
        config_path: base_config_path,
        // A syntactic pass reports at least the unused code of a type-aware
        // pass, so it can only make fewer head findings look introduced.
        type_aware: TypeAwareOptions {
            enabled: Some(false),
            ..TypeAwareOptions::default()
        },
        ..input.base_analysis.clone()
    };
    let options = AuditOptions {
        analysis: analysis.clone(),
        dead_code: Some(true),
        health: Some(false),
        duplication: Some(false),
        coverage: None,
        coverage_root: None,
        ..input.options.clone()
    };
    let subanalyses = super::run_audit_subanalyses(&options, &analysis, Some(focus), true).ok()?;
    let dead_code = subanalyses.dead_code?;
    Some(crate::audit_run::own_config_unused_code_keys(
        &dead_code.output.results,
        &own_config,
        input.base_results,
        &dead_code.root,
    ))
}

/// The base config, when it loads and keeps both unused-code rules on.
fn base_config_reporting_unused_code(
    input: &BaseOwnConfigInput<'_>,
    base_config_path: Option<&Path>,
) -> Option<ResolvedConfig> {
    let analysis = input.base_analysis;
    let project = fallow_engine::project_config::config_for_project_analysis(
        input.base_root,
        base_config_path,
        ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: analysis.no_cache,
            threads: analysis.threads.unwrap_or(1),
            production_override: analysis.production_override,
            quiet: true,
            analysis: ProductionAnalysis::DeadCode,
            allow_remote_extends: analysis.allow_remote_extends,
        },
    )
    .ok()?;
    let rules = &project.config.rules;
    (rules.unused_files != Severity::Off && rules.unused_exports != Severity::Off)
        .then_some(project.config)
}
