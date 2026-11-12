//! The unused code of the base side under the config of the base commit.

use std::path::{Path, PathBuf};

use fallow_config::{ProductionAnalysis, Severity};
use fallow_engine::project_config::ProjectConfigOptions;
use fallow_types::output_format::OutputFormat;
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
/// only evidence.
pub(super) fn base_own_config_unused_code(
    input: &BaseOwnConfigInput<'_>,
) -> Option<FxHashSet<String>> {
    let focus = input.focus?;
    let relative = input.head_config_path?.strip_prefix(input.head_root).ok()?;
    if !focus.contains(&input.base_root.join(relative)) {
        return None;
    }
    let base_config_path = fallow_config::FallowConfig::find_config_path(input.base_root);
    if !base_rules_report_unused_code(input, base_config_path.as_deref())? {
        return None;
    }
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
    let results = &dead_code.output.results;
    let mut keys = crate::audit_keys::collection_keys(&results.unused_files, &dead_code.root);
    keys.extend(crate::audit_keys::collection_keys(
        &results.unused_exports,
        &dead_code.root,
    ));
    Some(keys.into_iter().collect())
}

/// Whether the base config loads and keeps both unused-code rules on.
/// `None` when the base config does not load.
fn base_rules_report_unused_code(
    input: &BaseOwnConfigInput<'_>,
    base_config_path: Option<&Path>,
) -> Option<bool> {
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
    Some(rules.unused_files != Severity::Off && rules.unused_exports != Severity::Off)
}
