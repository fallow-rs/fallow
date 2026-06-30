//! Health core pipeline preparation.

use std::process::ExitCode;

use fallow_config::ResolvedConfig;

use super::analysis_data::{
    HealthAnalysisData, HealthAnalysisDataInput, prepare_health_analysis_data,
};
use super::coverage_settings::{HealthCoverageSettings, prepare_health_coverage_settings};
use super::findings_pipeline::{HealthFindingsData, HealthFindingsInput, prepare_health_findings};
use super::pipeline::HealthScope;
use super::runtime_sections::{
    HealthRuntimeSections, HealthRuntimeSectionsInput, prepare_health_runtime_sections,
};
use super::{HealthDerivedSections, HealthOptions, HealthSeams, HealthVitalData, scoring};

pub(super) struct HealthCoreSectionsInput<'a, R> {
    pub(super) opts: &'a HealthOptions<'a>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) scope: &'a HealthScope<'a, R>,
    pub(super) pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    pub(super) seams: &'a HealthSeams<'a>,
}

struct HealthAnalysisPreludeInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    seams: &'a HealthSeams<'a>,
}

struct HealthScopedFindingsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    score_output: Option<&'a scoring::FileScoreOutput>,
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
) -> Result<HealthPreparedCore, ExitCode> {
    let HealthCoreSectionsInput {
        opts,
        config,
        files,
        modules,
        scope,
        pre_computed_analysis,
        seams,
    } = input;

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
    })?;

    let findings_data = prepare_health_scoped_findings(&HealthScopedFindingsInput {
        opts,
        config,
        modules,
        scope,
        score_output: analysis_data.score_output.as_ref(),
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
            has_istanbul_coverage,
            needs_file_scores,
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
) -> Result<HealthAnalysisPrelude, ExitCode> {
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
) -> Result<HealthFindingsData, ExitCode> {
    prepare_health_findings(HealthFindingsInput {
        opts: input.opts,
        config: input.config,
        modules: input.modules,
        file_paths: &input.scope.file_paths,
        ignore_set: &input.scope.ignore_set,
        changed_files: input.scope.changed_files.as_ref(),
        ws_roots: input.scope.ws_roots.as_deref(),
        diff_index: input.scope.diff_index,
        max_cyclomatic: input.scope.max_cyclomatic,
        max_cognitive: input.scope.max_cognitive,
        max_crap: input.scope.max_crap,
        enforce_crap: input.scope.enforce_crap,
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
