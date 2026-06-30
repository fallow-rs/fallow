//! Health runtime section assembly.

use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use fallow_output::{ComplexityViolation, FileHealthScore, RefactoringTarget};

use crate::baseline::HealthBaselineData;

use super::analysis_data::HealthAnalysisData;
use super::findings_pipeline::save_health_baseline_if_requested;
use super::runtime_filter::{RuntimeCoverageFilterContext, apply_runtime_coverage_filters};
use super::{
    HealthDerivedSectionInput, HealthDerivedSections, HealthOptions, HealthVitalData,
    HealthVitalDataInput, health_file_scores_slice, prepare_health_derived_sections,
    prepare_health_vital_data,
};

pub(super) struct HealthRuntimeSectionsInput<'a> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) diff_index: Option<&'a fallow_output::DiffIndex>,
    pub(super) loaded_baseline: Option<&'a HealthBaselineData>,
    pub(super) findings: &'a [ComplexityViolation],
    pub(super) analysis_data: HealthAnalysisData,
    pub(super) has_istanbul_coverage: bool,
    pub(super) needs_file_scores: bool,
}

pub(super) struct HealthRuntimeSections {
    pub(super) analysis_data: HealthAnalysisData,
    pub(super) derived_sections: HealthDerivedSections,
    pub(super) vital_data: HealthVitalData,
}

pub(super) fn prepare_health_runtime_sections(
    opts: &HealthOptions<'_>,
    mut input: HealthRuntimeSectionsInput<'_>,
) -> Result<HealthRuntimeSections, ExitCode> {
    let file_scores_slice = health_file_scores_slice(input.analysis_data.score_output.as_ref());
    let derived_sections = prepare_health_derived_sections(
        opts,
        HealthDerivedSectionInput {
            config: input.config,
            files: input.files,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            file_scores: file_scores_slice,
            churn_fetch: input.analysis_data.churn_fetch.take(),
            diff_index: input.diff_index,
            score_output: input.analysis_data.score_output.as_ref(),
            loaded_baseline: input.loaded_baseline,
        },
    );

    finalize_health_runtime_outputs(
        opts,
        HealthRuntimeFinalizeInput {
            config: input.config,
            runtime_coverage: &mut input.analysis_data.runtime_coverage,
            findings: input.findings,
            targets: &derived_sections.targets,
            loaded_baseline: input.loaded_baseline,
            changed_files: input.changed_files,
            diff_index: input.diff_index,
        },
    )?;

    let vital_data = prepare_health_vital_data_from_sections(
        opts,
        &input,
        &derived_sections,
        file_scores_slice,
    )?;

    Ok(HealthRuntimeSections {
        analysis_data: input.analysis_data,
        derived_sections,
        vital_data,
    })
}

fn prepare_health_vital_data_from_sections(
    opts: &HealthOptions<'_>,
    input: &HealthRuntimeSectionsInput<'_>,
    derived_sections: &HealthDerivedSections,
    file_scores_slice: &[FileHealthScore],
) -> Result<HealthVitalData, ExitCode> {
    prepare_health_vital_data(&HealthVitalDataInput {
        opts,
        modules: input.modules,
        file_paths: input.file_paths,
        score_output: input.analysis_data.score_output.as_ref(),
        file_scores_slice,
        hotspots: &derived_sections.hotspots,
        dupes_report: derived_sections.dupes_report.as_ref(),
        candidate_paths: &derived_sections.candidate_paths,
        total_files: input.files.len(),
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        diff_index: input.diff_index,
        hotspot_summary: derived_sections.hotspot_summary.as_ref(),
        has_istanbul_coverage: input.has_istanbul_coverage,
        needs_file_scores: input.needs_file_scores,
    })
}

fn filter_runtime_coverage_report(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    report: Option<&mut fallow_output::RuntimeCoverageReport>,
    loaded_baseline: Option<&HealthBaselineData>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&fallow_output::DiffIndex>,
) {
    if let Some(report) = report {
        let ctx = RuntimeCoverageFilterContext::new(&config.root)
            .with_baseline(loaded_baseline)
            .with_top(opts.top)
            .with_changed_files(changed_files)
            .with_diff_index(diff_index);
        apply_runtime_coverage_filters(report, &ctx);
    }
}

struct HealthRuntimeFinalizeInput<'a> {
    config: &'a ResolvedConfig,
    runtime_coverage: &'a mut Option<fallow_output::RuntimeCoverageReport>,
    findings: &'a [ComplexityViolation],
    targets: &'a [RefactoringTarget],
    loaded_baseline: Option<&'a HealthBaselineData>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn finalize_health_runtime_outputs(
    opts: &HealthOptions<'_>,
    input: HealthRuntimeFinalizeInput<'_>,
) -> Result<(), ExitCode> {
    let HealthRuntimeFinalizeInput {
        config,
        runtime_coverage,
        findings,
        targets,
        loaded_baseline,
        changed_files,
        diff_index,
    } = input;

    filter_runtime_coverage_report(
        opts,
        config,
        runtime_coverage.as_mut(),
        loaded_baseline,
        changed_files,
        diff_index,
    );
    save_health_baseline_if_requested(opts, config, findings, runtime_coverage.as_ref(), targets)
}
