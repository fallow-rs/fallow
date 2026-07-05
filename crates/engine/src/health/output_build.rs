//! Health output context, grouping, timings, and report assembly.

use std::time::Instant;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_output::{
    ComplexityViolation, FileHealthScore, HealthGrouping, HealthTimings, HotspotEntry,
    HotspotSummary, RefactoringTarget,
};

use super::actions::build_health_action_context;
use super::analysis_data::HealthAnalysisData;
use super::assembly::{HealthReportAssembly, assemble_health_report};
use super::findings_pipeline::HealthFindingsData;
use super::framework_health::build_framework_health_diagnostics;
use super::pipeline::{HealthPipelineTimings, HealthScope, HealthTimingBaseInput};
use super::result::HealthOutputParts;
use super::timings::{HealthTimingInput, build_health_timings};
use super::{
    HealthDerivedSections, HealthOptions, HealthVitalData, grouping, health_file_scores_slice,
    scoring,
};

pub(super) struct HealthOutputContextInput<'a, R> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) scope: &'a HealthScope<'a, R>,
    pub(super) needs_file_scores: bool,
    pub(super) report_coverage_gaps: bool,
    pub(super) has_istanbul_coverage: bool,
    pub(super) findings_data: HealthFindingsData,
    pub(super) analysis_data: HealthAnalysisData,
    pub(super) derived_sections: HealthDerivedSections,
    pub(super) vital_data: HealthVitalData,
    pub(super) timings: HealthPipelineTimings,
    pub(super) workspaces: &'a [WorkspaceInfo],
    pub(super) start: &'a Instant,
}

pub(super) struct HealthOutputContext<'a, R> {
    pub(super) build: HealthOutputBuildInput<'a, R>,
    pub(super) sections: HealthOutputSectionInput,
}

pub(super) struct HealthOutputBuildInput<'a, R> {
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    group_resolver: Option<&'a R>,
    needs_file_scores: bool,
    report_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    workspaces: &'a [WorkspaceInfo],
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
    timing_base: HealthTimingBaseInput,
    start: &'a Instant,
}

pub(super) struct HealthOutputSectionInput {
    analysis_data: HealthAnalysisData,
    derived_sections: HealthDerivedSections,
    vital_data: HealthVitalData,
    findings: Vec<ComplexityViolation>,
}

struct HealthOutputSupportingParts {
    grouping: Option<fallow_output::HealthGrouping>,
    timings: Option<fallow_output::HealthTimings>,
}

pub(super) fn prepare_health_output_context<R>(
    input: HealthOutputContextInput<'_, R>,
) -> HealthOutputContext<'_, R> {
    let HealthFindingsData {
        findings,
        threshold_overrides,
        files_analyzed,
        total_functions,
        complexity_ms,
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline: _,
    } = input.findings_data;

    HealthOutputContext {
        build: HealthOutputBuildInput {
            config: input.config,
            modules: input.modules,
            file_paths: &input.scope.file_paths,
            group_resolver: input.scope.group_resolver.as_ref(),
            needs_file_scores: input.needs_file_scores,
            report_coverage_gaps: input.report_coverage_gaps,
            has_istanbul_coverage: input.has_istanbul_coverage,
            threshold_overrides,
            max_cyclomatic: input.scope.max_cyclomatic,
            max_cognitive: input.scope.max_cognitive,
            max_crap: input.scope.max_crap,
            workspaces: input.workspaces,
            files_analyzed,
            total_functions,
            total_above_threshold,
            sev_critical,
            sev_high,
            sev_moderate,
            timing_base: input.timings.into_base_input(complexity_ms),
            start: input.start,
        },
        sections: HealthOutputSectionInput {
            analysis_data: input.analysis_data,
            derived_sections: input.derived_sections,
            vital_data: input.vital_data,
            findings,
        },
    }
}

pub(super) fn build_health_output_parts<R: super::HealthGroupResolver>(
    opts: &HealthOptions<'_>,
    build: &HealthOutputBuildInput<'_, R>,
    sections: HealthOutputSectionInput,
) -> HealthOutputParts {
    let HealthOutputSectionInput {
        analysis_data,
        derived_sections,
        vital_data,
        findings,
    } = sections;
    let coverage_gaps_has_findings =
        health_coverage_gaps_has_findings(analysis_data.score_output.as_ref());
    let action_ctx = build_health_action_context(
        opts,
        build.config,
        build.max_cyclomatic,
        build.max_cognitive,
        build.max_crap,
    );

    let HealthOutputSupportingParts { grouping, timings } =
        build_health_supporting_parts(HealthSupportingPartsInput {
            opts,
            build,
            analysis_data: &analysis_data,
            derived_sections: &derived_sections,
            vital_data: &vital_data,
            findings: &findings,
            action_ctx: &action_ctx,
        });

    let framework_health = build_framework_health_diagnostics(
        build.config,
        build.workspaces,
        analysis_data.framework_health_facts,
    );

    let report = build_health_report_from_pipeline(
        opts,
        &action_ctx,
        build_health_report_pipeline_input(
            build,
            analysis_data,
            vital_data,
            derived_sections,
            findings,
            framework_health,
        ),
    );

    HealthOutputParts {
        report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    }
}

fn build_health_report_pipeline_input<R>(
    build: &HealthOutputBuildInput<'_, R>,
    analysis_data: HealthAnalysisData,
    vital_data: HealthVitalData,
    derived_sections: HealthDerivedSections,
    findings: Vec<ComplexityViolation>,
    framework_health: Option<fallow_output::FrameworkHealthDiagnostics>,
) -> HealthReportPipelineInput {
    HealthReportPipelineInput {
        report_coverage_gaps: build.report_coverage_gaps,
        findings,
        threshold_overrides: build.threshold_overrides.clone(),
        files_analyzed: build.files_analyzed,
        total_functions: build.total_functions,
        total_above_threshold: build.total_above_threshold,
        max_cyclomatic: build.max_cyclomatic,
        max_cognitive: build.max_cognitive,
        max_crap: build.max_crap,
        max_unit_size: build.config.health.max_unit_size,
        analysis_data,
        vital_data,
        hotspots: derived_sections.hotspots,
        hotspot_summary: derived_sections.hotspot_summary,
        targets: derived_sections.targets,
        target_thresholds: derived_sections.target_thresholds,
        has_istanbul_coverage: build.has_istanbul_coverage,
        framework_health,
        sev_critical: build.sev_critical,
        sev_high: build.sev_high,
        sev_moderate: build.sev_moderate,
    }
}

#[derive(Clone, Copy)]
struct HealthSupportingPartsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    build: &'a HealthOutputBuildInput<'a, R>,
    analysis_data: &'a HealthAnalysisData,
    derived_sections: &'a HealthDerivedSections,
    vital_data: &'a HealthVitalData,
    findings: &'a [ComplexityViolation],
    action_ctx: &'a fallow_output::HealthActionContext,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "input is a Copy struct; by-value matches the original CLI signature"
)]
fn build_health_supporting_parts<R: super::HealthGroupResolver>(
    input: HealthSupportingPartsInput<'_, R>,
) -> HealthOutputSupportingParts {
    let grouping = build_health_output_grouping(&input);
    let timings = build_health_timings_from_pipeline(
        input.opts,
        input.build.start,
        input.analysis_data,
        input.derived_sections,
        &input.build.timing_base,
    );

    HealthOutputSupportingParts { grouping, timings }
}

fn build_health_output_grouping<R: super::HealthGroupResolver>(
    input: &HealthSupportingPartsInput<'_, R>,
) -> Option<fallow_output::HealthGrouping> {
    let file_scores = health_file_scores_slice(input.analysis_data.score_output.as_ref());
    build_health_grouping_from_context(HealthGroupingContextInput {
        opts: input.opts,
        config: input.build.config,
        group_resolver: input.build.group_resolver,
        candidate_paths: &input.derived_sections.candidate_paths,
        modules: input.build.modules,
        file_paths: input.build.file_paths,
        score_output: input.analysis_data.score_output.as_ref(),
        file_scores,
        findings: input.findings,
        hotspots: &input.derived_sections.hotspots,
        vital_data: input.vital_data,
        targets: &input.derived_sections.targets,
        dupes_report: input.derived_sections.dupes_report.as_ref(),
        needs_file_scores: input.build.needs_file_scores,
        action_ctx: input.action_ctx,
    })
}

struct HealthReportPipelineInput {
    report_coverage_gaps: bool,
    findings: Vec<ComplexityViolation>,
    threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    max_unit_size: u32,
    analysis_data: HealthAnalysisData,
    vital_data: HealthVitalData,
    hotspots: Vec<HotspotEntry>,
    hotspot_summary: Option<HotspotSummary>,
    targets: Vec<RefactoringTarget>,
    target_thresholds: Option<fallow_output::TargetThresholds>,
    has_istanbul_coverage: bool,
    framework_health: Option<fallow_output::FrameworkHealthDiagnostics>,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
}

fn build_health_report_from_pipeline(
    opts: &HealthOptions<'_>,
    action_ctx: &fallow_output::HealthActionContext,
    input: HealthReportPipelineInput,
) -> fallow_output::HealthReport {
    assemble_health_report(
        opts,
        action_ctx,
        HealthReportAssembly {
            report_coverage_gaps: input.report_coverage_gaps,
            findings: input.findings,
            threshold_overrides: input.threshold_overrides,
            files_analyzed: input.files_analyzed,
            total_functions: input.total_functions,
            total_above_threshold: input.total_above_threshold,
            max_cyclomatic: input.max_cyclomatic,
            max_cognitive: input.max_cognitive,
            max_crap: input.max_crap,
            max_unit_size: input.max_unit_size,
            files_scored: input.analysis_data.files_scored,
            average_maintainability: input.analysis_data.average_maintainability,
            vital_signs: input.vital_data.vital_signs,
            health_score: input.vital_data.health_score,
            score_output: input.analysis_data.score_output,
            hotspots: input.hotspots,
            hotspot_summary: input.hotspot_summary,
            targets: input.targets,
            target_thresholds: input.target_thresholds,
            health_trend: input.vital_data.health_trend,
            has_istanbul_coverage: input.has_istanbul_coverage,
            runtime_coverage: input.analysis_data.runtime_coverage,
            framework_health: input.framework_health,
            large_functions: input.vital_data.large_functions,
            sev_critical: input.sev_critical,
            sev_high: input.sev_high,
            sev_moderate: input.sev_moderate,
        },
    )
}

#[derive(Clone, Copy)]
struct HealthGroupingContextInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    group_resolver: Option<&'a R>,
    candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores: &'a [FileHealthScore],
    findings: &'a [ComplexityViolation],
    hotspots: &'a [HotspotEntry],
    vital_data: &'a HealthVitalData,
    targets: &'a [RefactoringTarget],
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
    needs_file_scores: bool,
    action_ctx: &'a fallow_output::HealthActionContext,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "input is a Copy struct; by-value matches the original CLI signature"
)]
fn build_health_grouping_from_context<R: super::HealthGroupResolver>(
    input: HealthGroupingContextInput<'_, R>,
) -> Option<fallow_output::HealthGrouping> {
    build_optional_health_grouping_opt(
        input.group_resolver,
        &input.config.root,
        input.candidate_paths,
        &grouping::HealthGroupingInput {
            modules: input.modules,
            file_paths: input.file_paths,
            score_output: input.score_output,
            file_scores: input.file_scores,
            findings: input.findings,
            hotspots: input.hotspots,
            large_functions: &input.vital_data.large_functions,
            targets: input.targets,
            score_requested: input.opts.score,
            dupes_report: input.opts.score.then_some(input.dupes_report).flatten(),
            needs_file_scores: input.needs_file_scores,
            needs_hotspots: input.opts.hotspots || input.opts.targets,
            show_vital_signs: !input.opts.score_only_output,
            action_ctx: input.action_ctx,
        },
    )
}

fn health_coverage_gaps_has_findings(score_output: Option<&scoring::FileScoreOutput>) -> bool {
    score_output.is_some_and(|output| !output.coverage.report.is_empty())
}

fn build_health_timings_from_pipeline(
    opts: &HealthOptions<'_>,
    start: &Instant,
    analysis_data: &HealthAnalysisData,
    sections: &HealthDerivedSections,
    input: &HealthTimingBaseInput,
) -> Option<HealthTimings> {
    build_health_timings(
        opts,
        start,
        &HealthTimingInput {
            config_ms: input.config_ms,
            discover_ms: input.discover_ms,
            parse_ms: input.parse_ms,
            parse_cpu_ms: input.parse_cpu_ms,
            complexity_ms: input.complexity_ms,
            file_scores_ms: analysis_data.file_scores_ms,
            git_churn_ms: analysis_data.git_churn_ms,
            git_churn_cache_hit: analysis_data.git_churn_cache_hit,
            hotspots_ms: sections.hotspots_ms,
            duplication_ms: sections.duplication_ms,
            targets_ms: sections.targets_ms,
            shared_parse: input.shared_parse,
        },
    )
}

fn build_optional_health_grouping_opt<R: super::HealthGroupResolver>(
    resolver: Option<&R>,
    project_root: &std::path::Path,
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
    input: &grouping::HealthGroupingInput<'_>,
) -> Option<HealthGrouping> {
    let resolver = resolver?;
    Some(grouping::build_health_grouping(
        resolver as &dyn super::HealthGroupResolver,
        project_root,
        candidate_paths,
        input,
    ))
}
