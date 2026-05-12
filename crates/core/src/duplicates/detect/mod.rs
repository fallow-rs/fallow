//! Suffix Array + LCP based clone detection engine.
//!
//! Uses an O(N log N) prefix-doubling suffix array construction (with radix
//! sort) followed by an O(N) LCP scan. This avoids quadratic pairwise
//! comparisons and naturally finds all maximal clones in a single linear pass.

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
        self.detect_inner(file_data, None)
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
        self.detect_inner(file_data, Some(focus_files))
    }

    fn detect_inner(
        &self,
        file_data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)>,
        focus_files: Option<&FxHashSet<PathBuf>>,
    ) -> DuplicationReport {
        let _span = tracing::info_span!("clone_detect").entered();

        if file_data.is_empty() || self.min_tokens == 0 {
            return empty_report(0);
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

        // Compute total stats.
        let total_files = files.len();
        let total_lines: usize = files.iter().map(|f| f.file_tokens.line_count).sum();
        let total_tokens: usize = files.iter().map(|f| f.hashed_tokens.len()).sum();
        let focus_file_ids = focus_files.map(|focus| {
            files
                .iter()
                .map(|file| focus.contains(&file.path))
                .collect::<Vec<_>>()
        });

        tracing::debug!(
            total_files,
            total_tokens,
            total_lines,
            focused_files = focus_file_ids
                .as_ref()
                .map_or(0, |ids| ids.iter().filter(|&&is_focus| is_focus).count()),
            "clone detection input"
        );

        // Step 1: Rank reduction — map u64 hashes to consecutive u32 ranks.
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

        // Step 2: Concatenate with sentinels.
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
            return empty_report(total_files);
        }

        // Step 3: Build suffix array.
        let t0 = std::time::Instant::now();
        let sa = suffix_array::build_suffix_array(&text);
        let sa_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = sa_time.as_micros(),
            n = text.len(),
            "step3_suffix_array"
        );

        // Step 4: Build LCP array (Kasai's algorithm, sentinel-aware).
        let t0 = std::time::Instant::now();
        let lcp_arr = lcp::build_lcp(&text, &sa);
        let lcp_time = t0.elapsed();
        tracing::debug!(elapsed_us = lcp_time.as_micros(), "step4_lcp_array");

        // Step 5: Extract clone groups from LCP intervals.
        let t0 = std::time::Instant::now();
        let raw_groups = extraction::extract_clone_groups(
            &sa,
            &lcp_arr,
            &file_of,
            &file_offsets,
            self.min_tokens,
            &files,
            focus_file_ids.as_deref(),
        );
        let extract_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = extract_time.as_micros(),
            raw_groups = raw_groups.len(),
            "step5_extract_groups"
        );

        // Step 6: Build CloneGroup structs with line info, apply filters.
        let t0 = std::time::Instant::now();
        let clone_groups =
            filtering::build_groups(raw_groups, &files, self.min_lines, self.skip_local);
        let build_time = t0.elapsed();
        tracing::debug!(
            elapsed_us = build_time.as_micros(),
            final_groups = clone_groups.len(),
            "step6_build_groups"
        );

        // Step 7: Compute stats.
        let t0 = std::time::Instant::now();
        let stats =
            statistics::compute_stats(&clone_groups, total_files, total_lines, total_tokens);
        let stats_time = t0.elapsed();
        tracing::debug!(elapsed_us = stats_time.as_micros(), "step7_compute_stats");

        tracing::info!(
            total_us = (rank_time
                + concat_time
                + sa_time
                + lcp_time
                + extract_time
                + build_time
                + stats_time)
                .as_micros(),
            rank_us = rank_time.as_micros(),
            sa_us = sa_time.as_micros(),
            lcp_us = lcp_time.as_micros(),
            extract_us = extract_time.as_micros(),
            build_us = build_time.as_micros(),
            stats_us = stats_time.as_micros(),
            total_tokens,
            clone_groups = clone_groups.len(),
            "clone detection complete"
        );

        DuplicationReport {
            clone_groups,
            clone_families: vec![], // Populated by the caller after suppression filtering
            mirrored_directories: vec![],
            stats,
        }
    }
}

/// Create an empty report when there are no files to analyze.
const fn empty_report(total_files: usize) -> DuplicationReport {
    DuplicationReport {
        clone_groups: Vec::new(),
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            total_files,
            files_with_clones: 0,
            total_lines: 0,
            duplicated_lines: 0,
            total_tokens: 0,
            duplicated_tokens: 0,
            clone_groups: 0,
            clone_instances: 0,
            duplication_percentage: 0.0,
            clone_groups_below_min_occurrences: 0,
        },
    }
}
