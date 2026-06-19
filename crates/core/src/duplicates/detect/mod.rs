//! Suffix Array + LCP based clone detection engine.
//!
//! Uses a linear-time SA-IS (suffix array by induced sorting) construction
//! followed by an O(N) LCP scan. This avoids quadratic pairwise comparisons
//! and naturally finds all maximal clones in a single linear pass.

mod concatenation;
mod extraction;
mod filtering;
mod lcp;
mod ranking;
mod statistics;
mod suffix_array;
mod utils;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use oxc_span::Span;
use rustc_hash::FxHashSet;

use super::normalize::HashedToken;
use super::tokenize::FileTokens;
use super::types::{DuplicationReport, DuplicationStats};

/// Data for a single file being analyzed.
struct FileData {
    path: PathBuf,
    hashed_tokens: Vec<HashedToken>,
    file_tokens: FileTokens,
    atomic_invocation_spans: Vec<Span>,
}

#[derive(Clone, Copy)]
pub(super) struct CorpusTotals {
    pub(super) files: usize,
    pub(super) lines: usize,
    pub(super) tokens: usize,
}

/// Suffix Array + LCP based clone detection engine.
///
/// Concatenates all files' token sequences (separated by unique sentinels),
/// builds a suffix array and LCP array, then extracts maximal clone groups
/// from contiguous LCP intervals.
pub struct CloneDetector {
    /// Minimum clone size in tokens.
    min_tokens: usize,
    /// Minimum clone size in lines.
    min_lines: usize,
    /// Only report cross-directory duplicates.
    skip_local: bool,
}

impl CloneDetector {
    /// Create a new detector with the given thresholds.
    #[must_use]
    pub const fn new(min_tokens: usize, min_lines: usize, skip_local: bool) -> Self {
        Self {
            min_tokens,
            min_lines,
            skip_local,
        }
    }

    /// Run clone detection across all files.
    ///
    /// `file_data` is a list of `(path, hashed_tokens, file_tokens)` tuples,
    /// one per analyzed file.
    pub fn detect(
        &self,
        file_data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)>,
    ) -> DuplicationReport {
        self.detect_inner(file_data, None, None)
    }

    pub(super) fn detect_with_totals(
        &self,
        file_data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)>,
        totals: CorpusTotals,
    ) -> DuplicationReport {
        self.detect_inner(file_data, None, Some(totals))
    }

    /// Run clone detection while only materializing groups that touch one of the
    /// given files.
    ///
    /// All files still participate in matching, so focused files can be reported
    /// as duplicated against unchanged files. Groups that only involve
    /// non-focused files are dropped before expensive result building.
    pub fn detect_touching_files(
        &self,
        file_data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)>,
        focus_files: &FxHashSet<PathBuf>,
    ) -> DuplicationReport {
        self.detect_inner(file_data, Some(focus_files), None)
    }

    fn detect_inner(
        &self,
        file_data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)>,
        focus_files: Option<&FxHashSet<PathBuf>>,
        corpus_totals: Option<CorpusTotals>,
    ) -> DuplicationReport {
        let _span = tracing::info_span!("clone_detect").entered();

        if file_data.is_empty() || self.min_tokens == 0 {
            return empty_report(corpus_totals.unwrap_or(CorpusTotals {
                files: 0,
                lines: 0,
                tokens: 0,
            }));
        }

        let files: Vec<FileData> = file_data
            .into_iter()
            .map(|(path, hashed_tokens, file_tokens)| FileData {
                atomic_invocation_spans: file_tokens.atomic_invocation_spans.clone(),
                path,
                hashed_tokens,
                file_tokens,
            })
            .collect();

        let totals = corpus_totals.unwrap_or_else(|| CorpusTotals {
            files: files.len(),
            lines: files.iter().map(|f| f.file_tokens.line_count).sum(),
            tokens: files.iter().map(|f| f.hashed_tokens.len()).sum(),
        });
        let focus_file_ids = focus_files.map(|focus| build_focus_file_ids(&files, focus));
        trace_clone_detection_input(
            totals.files,
            totals.tokens,
            totals.lines,
            focus_file_ids.as_deref(),
        );

        let t0 = std::time::Instant::now();
        let ranked_files = ranking::rank_reduce(&files);
        let rank_time = t0.elapsed();
        let unique_ranks: usize = ranked_files
            .iter()
            .flat_map(|f| f.iter())
            .copied()
            .max()
            .map_or(0, |m| m as usize + 1);
        tracing::debug!(
            elapsed_us = rank_time.as_micros(),
            unique_ranks,
            "step1_rank_reduce"
        );

        let t0 = std::time::Instant::now();
        let (text, file_of, file_offsets) =
            concatenation::concatenate_with_sentinels(&ranked_files);
        let concat_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = concat_time.as_micros(),
            concat_len = text.len(),
            "step2_concatenate"
        );

        if text.is_empty() {
            return empty_report(totals);
        }

        let t0 = std::time::Instant::now();
        let sa = suffix_array::build_suffix_array(&text);
        let sa_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = sa_time.as_micros(),
            n = text.len(),
            "step3_suffix_array"
        );

        let t0 = std::time::Instant::now();
        let lcp_arr = lcp::build_lcp(&text, &sa);
        let lcp_time = t0.elapsed();
        tracing::debug!(elapsed_us = lcp_time.as_micros(), "step4_lcp_array");

        let t0 = std::time::Instant::now();
        let raw_groups = extraction::extract_clone_groups(&extraction::CloneGroupExtractionInput {
            sa: &sa,
            lcp: &lcp_arr,
            file_of: &file_of,
            file_offsets: &file_offsets,
            min_tokens: self.min_tokens,
            files: &files,
            focus_file_ids: focus_file_ids.as_deref(),
        });
        let extract_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = extract_time.as_micros(),
            raw_groups = raw_groups.len(),
            "step5_extract_groups"
        );

        let t0 = std::time::Instant::now();
        let clone_groups =
            filtering::build_groups(raw_groups, &files, self.min_lines, self.skip_local);
        let build_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = build_time.as_micros(),
            final_groups = clone_groups.len(),
            "step6_build_groups"
        );

        let t0 = std::time::Instant::now();
        let stats =
            statistics::compute_stats(&clone_groups, totals.files, totals.lines, totals.tokens);
        let stats_time = t0.elapsed();
        tracing::debug!(elapsed_us = stats_time.as_micros(), "step7_compute_stats");

        trace_clone_detection_complete(
            &CloneDetectionTimings {
                rank: rank_time,
                concat: concat_time,
                suffix_array: sa_time,
                lcp: lcp_time,
                extract: extract_time,
                build: build_time,
                stats: stats_time,
            },
            totals.tokens,
            clone_groups.len(),
        );

        DuplicationReport {
            clone_groups,
            clone_families: vec![], // Populated by the caller after suppression filtering
            mirrored_directories: vec![],
            stats,
        }
    }
}

struct CloneDetectionTimings {
    rank: std::time::Duration,
    concat: std::time::Duration,
    suffix_array: std::time::Duration,
    lcp: std::time::Duration,
    extract: std::time::Duration,
    build: std::time::Duration,
    stats: std::time::Duration,
}

fn build_focus_file_ids(files: &[FileData], focus_files: &FxHashSet<PathBuf>) -> Vec<bool> {
    let normalized: rustc_hash::FxHashSet<std::path::PathBuf> = focus_files
        .iter()
        .map(|p| dunce::simplified(p).to_path_buf())
        .collect();
    files
        .iter()
        .map(|file| normalized.contains(dunce::simplified(&file.path)))
        .collect()
}

fn trace_clone_detection_input(
    total_files: usize,
    total_tokens: usize,
    total_lines: usize,
    focus_file_ids: Option<&[bool]>,
) {
    tracing::debug!(
        total_files,
        total_tokens,
        total_lines,
        focused_files =
            focus_file_ids.map_or(0, |ids| ids.iter().filter(|&&is_focus| is_focus).count()),
        "clone detection input"
    );
}

fn trace_clone_detection_complete(
    timings: &CloneDetectionTimings,
    total_tokens: usize,
    clone_groups: usize,
) {
    tracing::info!(
        total_us = (timings.rank
            + timings.concat
            + timings.suffix_array
            + timings.lcp
            + timings.extract
            + timings.build
            + timings.stats)
            .as_micros(),
        rank_us = timings.rank.as_micros(),
        sa_us = timings.suffix_array.as_micros(),
        lcp_us = timings.lcp.as_micros(),
        extract_us = timings.extract.as_micros(),
        build_us = timings.build.as_micros(),
        stats_us = timings.stats.as_micros(),
        total_tokens,
        clone_groups,
        "clone detection complete"
    );
}

/// Create an empty report when there are no files to analyze.
const fn empty_report(totals: CorpusTotals) -> DuplicationReport {
    DuplicationReport {
        clone_groups: Vec::new(),
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            total_files: totals.files,
            files_with_clones: 0,
            total_lines: totals.lines,
            duplicated_lines: 0,
            total_tokens: totals.tokens,
            duplicated_tokens: 0,
            clone_groups: 0,
            clone_instances: 0,
            duplication_percentage: 0.0,
            clone_groups_below_min_occurrences: 0,
        },
    }
}
