//! Health core pipeline preparation.

use fallow_config::ResolvedConfig;

use crate::{duplicates::DuplicationReport, results::DeadCodeAnalysisArtifacts};

use super::analysis_data::{
    HealthAnalysisData, HealthAnalysisDataInput, prepare_health_analysis_data,
};
use super::coverage_settings::{HealthCoverageSettings, prepare_health_coverage_settings};
use super::findings_pipeline::{HealthFindingsData, HealthFindingsInput, prepare_health_findings};
use super::pipeline::HealthScope;
use super::runtime_sections::{
    HealthRuntimeSections, HealthRuntimeSectionsInput, prepare_health_runtime_sections,
};
use super::threshold_overrides::{GlobalHealthThresholds, ThresholdOverrideResolver};
use super::{
    HealthDerivedSections, HealthError, HealthOptions, HealthSeams, HealthVitalData, scoring,
};

pub(super) struct HealthCoreSectionsInput<'a, R> {
    pub(super) opts: &'a HealthOptions<'a>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) scope: &'a HealthScope<'a, R>,
    pub(super) pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pub(super) pre_computed_duplication: Option<DuplicationReport>,
    pub(super) seams: &'a HealthSeams<'a>,
}

struct HealthAnalysisPreludeInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    seams: &'a HealthSeams<'a>,
    threshold_resolver: &'a ThresholdOverrideResolver,
}

struct HealthScopedFindingsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    threshold_resolver: &'a ThresholdOverrideResolver,
}

struct HealthAnalysisPrelude {
    analysis_data: HealthAnalysisData,
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    needs_file_scores: bool,
}

pub(super) struct HealthPreparedCore {
    pub(super) findings_data: HealthFindingsData,
    pub(super) analysis_data: HealthAnalysisData,
    pub(super) derived_sections: HealthDerivedSections,
    pub(super) vital_data: HealthVitalData,
    pub(super) report_coverage_gaps: bool,
    pub(super) enforce_coverage_gaps: bool,
    pub(super) has_istanbul_coverage: bool,
    pub(super) needs_file_scores: bool,
}

pub(super) fn prepare_health_core_sections<R>(
    input: HealthCoreSectionsInput<'_, R>,
) -> Result<HealthPreparedCore, HealthError> {
    let HealthCoreSectionsInput {
        opts,
        config,
        files,
        modules,
        scope,
        pre_computed_analysis,
        pre_computed_duplication,
        seams,
    } = input;

    // Constructed once for the whole run so file scoring, findings, and the
    // large-function list resolve the same effective ceilings from the same
    // flag-resolved globals (issue #2228).
    let threshold_resolver = ThresholdOverrideResolver::new(
        &config.health.threshold_overrides,
        GlobalHealthThresholds {
            cyclomatic: scope.max_cyclomatic,
            cognitive: scope.max_cognitive,
            crap: scope.max_crap,
            unit_size: config.health.max_unit_size,
        },
    );

    let HealthAnalysisPrelude {
        analysis_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    } = prepare_health_analysis_prelude(HealthAnalysisPreludeInput {
        opts,
        config,
        modules,
        scope,
        pre_computed_analysis,
        seams,
        threshold_resolver: &threshold_resolver,
    })?;

    let findings_data = prepare_health_scoped_findings(&HealthScopedFindingsInput {
        opts,
        config,
        modules,
        scope,
        score_output: analysis_data.score_output.as_ref(),
        threshold_resolver: &threshold_resolver,
    })?;

    let HealthRuntimeSections {
        analysis_data,
        derived_sections,
        vital_data,
    } = prepare_health_runtime_sections(
        opts,
        HealthRuntimeSectionsInput {
            config,
            files,
            modules,
            file_paths: &scope.file_paths,
            ignore_set: &scope.ignore_set,
            changed_files: scope.changed_files.as_ref(),
            ws_roots: scope.ws_roots.as_deref(),
            diff_index: scope.diff_index,
            loaded_baseline: findings_data.loaded_baseline.as_ref(),
            findings: &findings_data.findings,
            analysis_data,
            pre_computed_duplication,
            has_istanbul_coverage,
            needs_file_scores,
            max_crap: scope.max_crap,
            threshold_resolver: &threshold_resolver,
        },
    )?;

    Ok(HealthPreparedCore {
        findings_data,
        analysis_data,
        derived_sections,
        vital_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    })
}

fn prepare_health_analysis_prelude<R>(
    input: HealthAnalysisPreludeInput<'_, R>,
) -> Result<HealthAnalysisPrelude, HealthError> {
    let HealthCoverageSettings {
        report_coverage_gaps,
        enforce_coverage_gaps,
        istanbul_coverage,
    } = prepare_health_coverage_settings(input.opts, input.config)?;

    let needs_file_scores = needs_health_file_scores(
        input.opts,
        report_coverage_gaps,
        enforce_coverage_gaps,
        input.scope.enforce_crap,
    );
    let analysis_data = prepare_health_analysis_data(HealthAnalysisDataInput {
        opts: input.opts,
        config: input.config,
        modules: input.modules,
        file_paths: &input.scope.file_paths,
        ignore_set: &input.scope.ignore_set,
        changed_files: input.scope.changed_files.as_ref(),
        ws_roots: input.scope.ws_roots.as_deref(),
        istanbul_coverage: istanbul_coverage.as_ref(),
        pre_computed_analysis: input.pre_computed_analysis,
        needs_file_scores,
        seams: input.seams,
        threshold_resolver: input.threshold_resolver,
        enforce_crap: input.scope.enforce_crap,
    })?;

    Ok(HealthAnalysisPrelude {
        analysis_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage: istanbul_coverage.is_some(),
        needs_file_scores,
    })
}

fn prepare_health_scoped_findings<R>(
    input: &HealthScopedFindingsInput<'_, R>,
) -> Result<HealthFindingsData, HealthError> {
    prepare_health_findings(HealthFindingsInput {
        opts: input.opts,
        config: input.config,
        modules: input.modules,
        file_paths: &input.scope.file_paths,
        ignore_set: &input.scope.ignore_set,
        changed_files: input.scope.changed_files.as_ref(),
        ws_roots: input.scope.ws_roots.as_deref(),
        diff_index: input.scope.diff_index,
        enforce_crap: input.scope.enforce_crap,
        threshold_resolver: input.threshold_resolver,
        score_output: input.score_output,
    })
}

fn needs_health_file_scores(
    opts: &HealthOptions<'_>,
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    enforce_crap: bool,
) -> bool {
    opts.file_scores
        || report_coverage_gaps
        || enforce_coverage_gaps
        || opts.hotspots
        || opts.targets
        || opts.force_full
        || enforce_crap
}
