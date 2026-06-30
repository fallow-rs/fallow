//! Health analysis data preparation.

use std::process::ExitCode;

use fallow_config::ResolvedConfig;

use crate::error::emit_error;

use super::framework_health::FrameworkHealthFacts;
use super::{FileScoresAndChurnInput, HealthOptions, HealthSeams, RuntimeCoverageSeamInput};
use super::{compute_file_scores_and_churn, hotspots, print_slow_churn_note, scoring};

pub(super) struct HealthAnalysisData {
    pub(super) runtime_coverage: Option<fallow_output::RuntimeCoverageReport>,
    pub(super) score_output: Option<scoring::FileScoreOutput>,
    pub(super) files_scored: Option<usize>,
    pub(super) average_maintainability: Option<f64>,
    pub(super) framework_health_facts: Option<FrameworkHealthFacts>,
    pub(super) file_scores_ms: f64,
    pub(super) git_churn_ms: f64,
    pub(super) git_churn_cache_hit: bool,
    pub(super) churn_fetch: Option<hotspots::ChurnFetchResult>,
}

pub(super) struct HealthAnalysisDataInput<'a> {
    pub(super) opts: &'a HealthOptions<'a>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pub(super) pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    pub(super) needs_file_scores: bool,
    pub(super) seams: &'a HealthSeams<'a>,
}

pub(super) fn prepare_health_analysis_data(
    input: HealthAnalysisDataInput<'_>,
) -> Result<HealthAnalysisData, ExitCode> {
    let mut input = input;
    let needs_analysis_output = input.needs_file_scores || input.opts.runtime_coverage.is_some();
    let seams = input.seams;
    let mut shared_analysis =
        prepare_shared_health_analysis(&mut input, needs_analysis_output, seams)?;

    let runtime_coverage = analyze_runtime_coverage(
        RuntimeCoverageAnalysisScope {
            opts: input.opts,
            config: input.config,
            modules: input.modules,
            shared_analysis_output: shared_analysis.output.as_ref(),
            istanbul_coverage: input.istanbul_coverage,
            file_paths: input.file_paths,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
        },
        seams,
    )?;

    let precomputed_for_scores = shared_analysis.take_for_file_scores(input.needs_file_scores);

    let (file_score_result, file_scores_ms, churn_fetch) = compute_file_scores_and_churn(
        FileScoresAndChurnInput {
            opts: input.opts,
            config: input.config,
            modules: input.modules,
            file_paths: input.file_paths,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            ignore_set: input.ignore_set,
            istanbul_coverage: input.istanbul_coverage,
            needs_file_scores: input.needs_file_scores,
        },
        precomputed_for_scores,
    )?;
    let (git_churn_ms, git_churn_cache_hit) = churn_fetch
        .as_ref()
        .map_or((0.0, false), |cf| (cf.git_log_ms, cf.cache_hit));
    let (score_output, files_scored, average_maintainability) = file_score_result;

    print_slow_churn_note(input.opts, churn_fetch.as_ref());

    Ok(HealthAnalysisData {
        runtime_coverage,
        score_output,
        files_scored,
        average_maintainability,
        framework_health_facts: shared_analysis.framework_health_facts,
        file_scores_ms,
        git_churn_ms,
        git_churn_cache_hit,
        churn_fetch,
    })
}

fn prepare_shared_analysis_output(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    modules: &[crate::source::ModuleInfo],
    pre_computed: Option<crate::DeadCodeAnalysisArtifacts>,
    needed: bool,
) -> Result<Option<crate::DeadCodeAnalysisArtifacts>, ExitCode> {
    if !needed {
        return Ok(None);
    }
    if let Some(pre) = pre_computed {
        return Ok(Some(pre));
    }
    crate::dead_code::analyze_with_parse_result(config, modules)
        .map(Some)
        .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))
}

#[derive(Clone, Copy)]
struct RuntimeCoverageAnalysisScope<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    shared_analysis_output: Option<&'a crate::DeadCodeAnalysisArtifacts>,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn analyze_runtime_coverage(
    input: RuntimeCoverageAnalysisScope<'_>,
    seams: &HealthSeams<'_>,
) -> Result<Option<fallow_output::RuntimeCoverageReport>, ExitCode> {
    let Some(production_options) = input.opts.runtime_coverage.as_ref() else {
        return Ok(None);
    };
    let Some(analysis_output) = input.shared_analysis_output else {
        return Err(emit_error(
            "runtime coverage requires analysis output",
            2,
            input.opts.output,
        ));
    };
    (seams.runtime_coverage_analyzer)(
        production_options,
        RuntimeCoverageSeamInput {
            root: &input.config.root,
            modules: input.modules,
            analysis_output,
            istanbul_coverage: input.istanbul_coverage,
            file_paths: input.file_paths,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            top: input.opts.top,
            codeowners_path: input.config.codeowners.as_deref(),
            quiet: input.opts.quiet,
            output: input.opts.output,
        },
    )
    .map(Some)
}

struct PreparedSharedHealthAnalysis {
    output: Option<crate::DeadCodeAnalysisArtifacts>,
    framework_health_facts: Option<FrameworkHealthFacts>,
}

impl PreparedSharedHealthAnalysis {
    fn take_for_file_scores(
        &mut self,
        needs_file_scores: bool,
    ) -> Option<crate::DeadCodeAnalysisArtifacts> {
        if needs_file_scores {
            self.output.take()
        } else {
            None
        }
    }
}

fn prepare_shared_health_analysis(
    input: &mut HealthAnalysisDataInput<'_>,
    needs_analysis_output: bool,
    seams: &HealthSeams<'_>,
) -> Result<PreparedSharedHealthAnalysis, ExitCode> {
    let output = prepare_shared_analysis_output(
        input.opts,
        input.config,
        input.modules,
        input.pre_computed_analysis.take(),
        needs_analysis_output,
    )?;
    let framework_health_facts = output.as_ref().map(|output| FrameworkHealthFacts {
        unused_load_data_keys_global_abstain: output.results.unused_load_data_keys_global_abstain,
    });
    if let Some(graph) = output.as_ref().and_then(|output| output.graph.as_ref()) {
        (seams.note_graph_structure)(graph.module_count(), graph.edge_count());
    }

    Ok(PreparedSharedHealthAnalysis {
        output,
        framework_health_facts,
    })
}
