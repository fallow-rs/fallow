//! `fallow-ignore-start` / `fallow-ignore-end` ranges for clone detection.
//!
//! The tokens inside a range get unique hash values before detection. No
//! repeat can hold a unique value, so a clone inside a range is gone, and a
//! clone that reaches into a range stops at its border. A cut part below
//! `minTokens` or `minLines` then fails the normal thresholds.

use std::path::PathBuf;

use fallow_extract::suppress::ParsedSuppressions;
use fallow_types::duplicates::RangeMarkerFinding;
use rustc_hash::FxHashMap;

use super::TokenizedFile;
use super::tokenize::{FileTokens, TokenKind};
use super::types::DuplicationReport;
use crate::suppress::Suppression;

/// Drop the markers whose text is inside a string or template literal
/// token, then pair the range markers that are left.
///
/// The source scan does not know string context, so a marker in a template
/// literal would otherwise suppress the file, a line, or open or close a
/// range.
pub(super) fn drop_markers_in_strings(parsed: &mut ParsedSuppressions, tokens: &FileTokens) {
    if !parsed.has_markers() {
        return;
    }
    let literal_spans: Vec<(u32, u32)> = tokens
        .tokens
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                TokenKind::StringLiteral(_) | TokenKind::TemplateLiteral(_)
            )
        })
        .map(|token| (token.span.start, token.span.end))
        .collect();
    parsed.drop_markers_in_literals(&literal_spans);
    parsed.pair_ranges();
}

/// The `[first_line, end_line)` line ranges of the range suppressions.
fn line_ranges(suppressions: &[Suppression]) -> Vec<(usize, usize)> {
    suppressions
        .iter()
        .filter_map(|suppression| {
            suppression
                .range_end_line()
                .map(|end| (suppression.line as usize, end as usize))
        })
        .filter(|(first, end)| first < end)
        .collect()
}

/// 1-based line of each byte offset, by binary search over line starts.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(index, _)| index + 1))
            .collect();
        Self { starts }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.starts.partition_point(|&start| start <= offset)
    }
}

/// Give each hashed token inside a range a unique hash value.
///
/// Masked values count down from `u64::MAX`, one per token over the whole
/// corpus, so two masked tokens never match each other.
pub(super) fn mask_range_tokens(files: &mut [TokenizedFile]) {
    let mut next_mask = u64::MAX;
    for file in files {
        let ranges = line_ranges(&file.suppressions);
        if ranges.is_empty() {
            continue;
        }
        let index = LineIndex::new(&file.file_tokens.source);
        for token in &mut file.hashed_tokens {
            let Some(source) = file.file_tokens.tokens.get(token.original_index) else {
                continue;
            };
            let line = index.line_of(source.span.start as usize);
            if ranges
                .iter()
                .any(|&(first, end)| line >= first && line < end)
            {
                token.hash = next_mask;
                next_mask -= 1;
            }
        }
    }
}

/// The line ranges per file, for the files that have one.
pub(super) fn ranges_by_file(files: &[TokenizedFile]) -> FxHashMap<PathBuf, Vec<(usize, usize)>> {
    files
        .iter()
        .filter_map(|file| {
            let ranges = line_ranges(&file.suppressions);
            (!ranges.is_empty()).then(|| (file.path.clone(), ranges))
        })
        .collect()
}

/// Drop each clone instance that overlaps a range, and each group that has
/// fewer than two instances left.
///
/// Exact clones never reach into a range after [`mask_range_tokens`]. Near
/// clones come from their own token stream, so this filter applies the
/// range to them.
pub(super) fn drop_instances_in_ranges(
    report: &mut DuplicationReport,
    ranges: &FxHashMap<PathBuf, Vec<(usize, usize)>>,
) {
    if ranges.is_empty() {
        return;
    }
    report.clone_groups.retain_mut(|group| {
        let before = group.instances.len();
        group.instances.retain(|instance| {
            ranges.get(&instance.file).is_none_or(|file_ranges| {
                !file_ranges
                    .iter()
                    .any(|&(first, end)| instance.start_line < end && instance.end_line >= first)
            })
        });
        if group.instances.len() < 2 {
            return false;
        }
        if group.instances.len() != before {
            super::near::refresh_near_group_metrics(group);
        }
        true
    });
}

/// Move the range marker problems of each file into the report.
pub(super) fn collect_problems(report: &mut DuplicationReport, files: &[TokenizedFile]) {
    report.range_marker_problems = files
        .iter()
        .flat_map(|file| {
            file.range_problems
                .iter()
                .map(|problem| RangeMarkerFinding {
                    path: file.path.clone(),
                    problem: problem.clone(),
                })
        })
        .collect();
    report
        .range_marker_problems
        .sort_by(|a, b| (&a.path, a.problem.line).cmp(&(&b.path, b.problem.line)));
}
