#![allow(
    clippy::print_stderr,
    reason = "human stderr notes for slow churn and file-score failures are preserved from the health pipeline"
)]

use std::process::ExitCode;
use std::time::Instant;

use colored::Colorize;
use fallow_config::ResolvedConfig;
use fallow_output::FileHealthScore;
use fallow_types::output_format::OutputFormat;

use crate::error::emit_error;

use super::HealthExecutionOptions;
use super::filters::filter_coverage_gaps;
use super::hotspots;
use super::scoring::{self, compute_file_scores};

type FileScoreResult = (Option<scoring::FileScoreOutput>, Option<usize>, Option<f64>);
type FileScoresAndChurn = (FileScoreResult, f64, Option<hotspots::ChurnFetchResult>);

#[derive(Clone, Copy)]
pub struct FileScoresAndChurnInput<'a> {
    pub(crate) opts: &'a HealthExecutionOptions<'a>,
    pub(crate) config: &'a ResolvedConfig,
    pub(crate) modules: &'a [crate::source::ModuleInfo],
    pub(crate) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(crate) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(crate) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(crate) ignore_set: &'a globset::GlobSet,
    pub(crate) istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pub(crate) needs_file_scores: bool,
}

pub fn compute_file_scores_and_churn(
    input: FileScoresAndChurnInput<'_>,
    precomputed_for_scores: Option<crate::DeadCodeAnalysisArtifacts>,
) -> Result<FileScoresAndChurn, ExitCode> {
    let needs_churn = input.opts.hotspots || input.opts.targets;
    if input.needs_file_scores && needs_churn {
        return std::thread::scope(|s| {
            let churn_handle =
                s.spawn(|| hotspots::fetch_churn_data(input.opts, &input.config.cache_dir));
            let t = Instant::now();
            let score_result = compute_filtered_file_scores(FileScoreInput {
                config: input.config,
                modules: input.modules,
                file_paths: input.file_paths,
                changed_files: input.changed_files,
                ws_roots: input.ws_roots,
                ignore_set: input.ignore_set,
                output: input.opts.output,
                istanbul_coverage: input.istanbul_coverage,
                pre_computed: precomputed_for_scores,
            })?;
            let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
            let churn = churn_handle
                .join()
                .map_err(|_| emit_error("churn thread panicked", 2, input.opts.output))?;
            Ok((score_result, fs_ms, churn))
        });
    }

    let t = Instant::now();
    let score_result = if input.needs_file_scores {
        compute_filtered_file_scores(FileScoreInput {
            config: input.config,
            modules: input.modules,
            file_paths: input.file_paths,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            ignore_set: input.ignore_set,
            output: input.opts.output,
            istanbul_coverage: input.istanbul_coverage,
            pre_computed: precomputed_for_scores,
        })?
    } else {
        (None, None, None)
    };
    let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
    let churn = if needs_churn {
        hotspots::fetch_churn_data(input.opts, &input.config.cache_dir)
    } else {
        None
    };
    Ok((score_result, fs_ms, churn))
}

pub fn print_slow_churn_note(
    opts: &HealthExecutionOptions<'_>,
    churn_fetch: Option<&hotspots::ChurnFetchResult>,
) {
    if let Some(cf) = churn_fetch
        && !cf.cache_hit
        && !opts.no_cache
        && !opts.quiet
        && cf.git_log_ms > 500.0
    {
        eprintln!(
            "{}",
            format!(
                "  note: git churn analysis took {:.1}s (cached for next run at same HEAD)",
                cf.git_log_ms / 1000.0
            )
            .dimmed()
        );
    }
}

pub fn health_file_scores_slice(
    score_output: Option<&scoring::FileScoreOutput>,
) -> &[FileHealthScore] {
    score_output.map_or(&[] as &[_], |output| output.scores.as_slice())
}

/// Compute file scores, applying workspace and ignore filters.
struct FileScoreInput<'a> {
    config: &'a ResolvedConfig,
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    ignore_set: &'a globset::GlobSet,
    output: OutputFormat,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pre_computed: Option<crate::DeadCodeAnalysisArtifacts>,
}

fn compute_filtered_file_scores(input: FileScoreInput<'_>) -> Result<FileScoreResult, ExitCode> {
    let analysis_output = if let Some(pre) = input.pre_computed {
        pre
    } else {
        crate::dead_code::analyze_with_parse_result(input.config, input.modules)
            .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, input.output))?
    };
    match compute_file_scores(
        input.modules,
        input.file_paths,
        input.changed_files,
        analysis_output,
        input.istanbul_coverage,
        &input.config.root,
    ) {
        Ok(mut output) => {
            if let Some(ws) = input.ws_roots {
                output
                    .scores
                    .retain(|s| ws.iter().any(|r| s.path.starts_with(r)));
            }
            if !input.ignore_set.is_empty() {
                output.scores.retain(|s| {
                    let relative = s.path.strip_prefix(&input.config.root).unwrap_or(&s.path);
                    !input.ignore_set.is_match(relative)
                });
            }
            filter_coverage_gaps(
                &mut output.coverage.report,
                &mut output.coverage.runtime_paths,
                input.config,
                input.changed_files,
                input.ws_roots,
                input.ignore_set,
            );
            let total_scored = output.scores.len();
            let avg = if total_scored > 0 {
                let sum: f64 = output.scores.iter().map(|s| s.maintainability_index).sum();
                Some((sum / total_scored as f64 * 10.0).round() / 10.0)
            } else {
                None
            };
            Ok((Some(output), Some(total_scored), avg))
        }
        Err(e) => {
            eprintln!("Warning: failed to compute file scores: {e}");
            Ok((None, Some(0), None))
        }
    }
}
