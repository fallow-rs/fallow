//! Step 6: Result filtering and deduplication.
//!
//! Converts raw clone groups into `CloneGroup` structs with line info,
//! applies token-level and line-level subset removal, min_lines filter,
//! and skip_local filter.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::FileData;
use super::extraction::RawGroup;
use super::utils::build_clone_instance_fast;
use crate::duplicates::types::{CloneGroup, CloneInstance};

/// Sorted interval lists indexed by a numeric slot (file id or path index).
struct IntervalIndex {
    slots: Vec<Vec<(usize, usize)>>,
}

impl IntervalIndex {
    fn new(num_slots: usize) -> Self {
        Self {
            slots: vec![Vec::new(); num_slots],
        }
    }

    /// Return `true` if `[start, start+len)` is fully contained within an
    /// existing interval in `slot`.
    fn is_covered(&self, slot: usize, start: usize, len: usize) -> bool {
        let intervals = &self.slots[slot];
        let idx = intervals.partition_point(|&(s, _)| s <= start);
        if idx == 0 {
            return false;
        }
        let (s, e) = intervals[idx - 1];
        start >= s && start + len <= e
    }

    /// Insert `[start, end)` into `slot`, coalescing every existing interval
    /// that touches or overlaps the new range into a single merged interval.
    ///
    /// Robust to out-of-order inserts. Both `remove_token_subsets` and
    /// `remove_line_subsets` process groups in length-/token-desc order, so
    /// per-slot inserts arrive in arbitrary offset order; coalescing on every
    /// insert keeps the slot's interval list non-overlapping and accurate so
    /// `is_covered` does not produce false negatives across fragmented gaps.
    fn insert(&mut self, slot: usize, start: usize, end: usize) {
        let intervals = &mut self.slots[slot];
        let lo = intervals.partition_point(|&(_, e)| e < start);
        let hi = intervals.partition_point(|&(s, _)| s <= end);

        if lo == hi {
            intervals.insert(lo, (start, end));
        } else {
            let merged_start = intervals[lo].0.min(start);
            let merged_end = intervals[hi - 1].1.max(end);
            intervals.drain(lo..hi);
            intervals.insert(lo, (merged_start, merged_end));
        }
    }
}

/// Remove raw groups whose token spans are fully contained within a larger
/// group's spans. Groups must arrive sorted by length descending.
fn remove_token_subsets(mut raw_groups: Vec<RawGroup>, num_files: usize) -> Vec<RawGroup> {
    let raw_count = raw_groups.len();
    raw_groups.sort_by_key(|b| std::cmp::Reverse(b.length));

    let mut covered = IntervalIndex::new(num_files);
    let mut surviving = Vec::new();

    for rg in raw_groups {
        let len = rg.length;
        let all_covered = rg
            .instances
            .iter()
            .all(|&(fid, offset)| covered.is_covered(fid, offset, len));

        if all_covered {
            continue;
        }

        for &(fid, offset) in &rg.instances {
            covered.insert(fid, offset, offset + len);
        }
        surviving.push(rg);
    }

    tracing::trace!(
        raw = raw_count,
        surviving = surviving.len(),
        "token-level subset removal"
    );

    surviving
}

/// Build a sorted vec of newline byte positions per file for O(log L) lookup.
fn build_line_tables(files: &[FileData]) -> Vec<Vec<usize>> {
    files
        .iter()
        .map(|f| {
            let src = f.file_tokens.source.as_bytes();
            let mut lines = Vec::new();
            let mut pos = 0;
            while pos < src.len() {
                if let Some(offset) = src[pos..].iter().position(|&b| b == b'\n') {
                    lines.push(pos + offset);
                    pos += offset + 1;
                } else {
                    break;
                }
            }
            lines
        })
        .collect()
}

fn token_span_for_raw_instance(
    file: &FileData,
    offset: usize,
    length: usize,
) -> Option<(u32, u32)> {
    if offset + length > file.hashed_tokens.len() {
        return None;
    }
    let first_hashed = &file.hashed_tokens[offset];
    let last_hashed = &file.hashed_tokens[offset + length - 1];
    let first_source = file.file_tokens.tokens.get(first_hashed.original_index)?;
    let last_source = file.file_tokens.tokens.get(last_hashed.original_index)?;
    (first_source.span.start <= last_source.span.end)
        .then_some((first_source.span.start, last_source.span.end))
}

fn span_is_atomic_invocation(file: &FileData, start: u32, end: u32) -> bool {
    file.atomic_invocation_spans
        .iter()
        .any(|span| span.start <= start && end <= span.end)
}

/// Return true when the entire clone group is just repeated invocation syntax,
/// optionally wrapped by a return/await/expression statement. Those findings
/// tend to be non-actionable: the shared abstraction is already the callee.
fn is_atomic_invocation_group(rg: &RawGroup, files: &[FileData]) -> bool {
    let mut seen: FxHashSet<(usize, usize)> = FxHashSet::default();
    let mut checked = 0;

    for &(file_id, offset) in &rg.instances {
        if !seen.insert((file_id, offset)) {
            continue;
        }
        let Some(file) = files.get(file_id) else {
            return false;
        };
        let Some((start, end)) = token_span_for_raw_instance(file, offset, rg.length) else {
            return false;
        };
        if !span_is_atomic_invocation(file, start, end) {
            return false;
        }
        checked += 1;
    }

    checked >= 2
}

/// Convert a single `RawGroup` into a `CloneGroup`, returning `None` when
/// the group should be filtered out (too few instances, below min_lines,
/// or same-directory when skip_local is set).
fn build_clone_group(
    rg: &RawGroup,
    files: &[FileData],
    line_tables: &[Vec<usize>],
    min_lines: usize,
    skip_local: bool,
) -> Option<CloneGroup> {
    let mut instances = collect_clone_instances(rg, files, line_tables);

    if skip_local && instances.len() >= 2 {
        let dirs: FxHashSet<_> = instances
            .iter()
            .filter_map(|inst| inst.file.parent().map(Path::to_path_buf))
            .collect();
        if dirs.len() < 2 {
            return None;
        }
    }

    if instances.len() < 2 {
        return None;
    }

    let line_count = instances
        .iter()
        .map(|inst| inst.end_line.saturating_sub(inst.start_line) + 1)
        .max()
        .unwrap_or(0);

    if line_count < min_lines {
        return None;
    }

    instances.sort_by(|a, b| a.file.cmp(&b.file).then(a.start_line.cmp(&b.start_line)));
    merge_overlapping_instances(&mut instances);

    if instances.len() < 2 {
        return None;
    }

    Some(CloneGroup {
        instances,
        token_count: rg.length,
        line_count,
    })
}

/// Materialize each unique `(file_id, offset)` instance of a raw group into a
/// [`CloneInstance`], skipping exact-position duplicates.
fn collect_clone_instances(
    rg: &RawGroup,
    files: &[FileData],
    line_tables: &[Vec<usize>],
) -> Vec<CloneInstance> {
    let mut seen: FxHashSet<(usize, usize)> = FxHashSet::default();
    let mut instances: Vec<CloneInstance> = Vec::new();

    for &(file_id, offset) in &rg.instances {
        if !seen.insert((file_id, offset)) {
            continue;
        }
        let file = &files[file_id];
        if let Some(inst) =
            build_clone_instance_fast(file, offset, rg.length, &line_tables[file_id])
        {
            instances.push(inst);
        }
    }
    instances
}

/// Coalesce instances in the same file whose line ranges overlap or abut,
/// extending the surviving entry's end position. `instances` must be sorted by
/// `(file, start_line)`.
fn merge_overlapping_instances(instances: &mut Vec<CloneInstance>) {
    instances.dedup_by(|b, a| {
        if a.file != b.file {
            return false;
        }
        if b.start_line <= a.end_line {
            if b.end_line > a.end_line {
                a.end_line = b.end_line;
                a.end_col = b.end_col;
            }
            true
        } else {
            false
        }
    });
}

/// Remove groups whose line ranges are fully contained within a larger
/// group's line ranges. Groups must arrive sorted by token count descending.
///
/// Uses a per-file interval index to avoid O(g^2 x m x n): iterate from
/// largest to smallest, registering kept groups' spans and checking smaller
/// groups against the index in O(instances x log(intervals)).
fn remove_line_subsets(clone_groups: Vec<CloneGroup>) -> Vec<CloneGroup> {
    let mut path_to_idx: FxHashMap<PathBuf, usize> = FxHashMap::default();
    for group in &clone_groups {
        for inst in &group.instances {
            let next = path_to_idx.len();
            path_to_idx.entry(inst.file.clone()).or_insert(next);
        }
    }

    let mut index = IntervalIndex::new(path_to_idx.len());
    let mut kept = Vec::new();

    for group in clone_groups {
        let all_contained = group.instances.iter().all(|inst| {
            let Some(&fidx) = path_to_idx.get(&inst.file) else {
                tracing::error!(
                    file = %inst.file.display(),
                    "remove_line_subsets: instance file missing from path_to_idx; please report"
                );
                return false;
            };
            let intervals = &index.slots[fidx];
            let idx = intervals.partition_point(|&(s, _)| s <= inst.start_line);
            idx > 0 && {
                let (s, e) = intervals[idx - 1];
                inst.start_line >= s && inst.end_line <= e
            }
        });

        if all_contained {
            continue;
        }

        for inst in &group.instances {
            let Some(&fidx) = path_to_idx.get(&inst.file) else {
                continue;
            };
            index.insert(fidx, inst.start_line, inst.end_line);
        }
        kept.push(group);
    }

    kept
}

/// Convert raw groups into `CloneGroup` structs, applying `min_lines` and
/// `skip_local` filters, deduplication, and subset removal.
pub(super) fn build_groups(
    raw_groups: Vec<RawGroup>,
    files: &[FileData],
    min_lines: usize,
    skip_local: bool,
) -> Vec<CloneGroup> {
    if raw_groups.is_empty() {
        return Vec::new();
    }

    let raw_count = raw_groups.len();

    let t0 = std::time::Instant::now();
    let surviving = remove_token_subsets(raw_groups, files.len());
    let token_subset_us = t0.elapsed().as_micros();

    let t0 = std::time::Instant::now();
    let line_tables = build_line_tables(files);
    let line_tables_us = t0.elapsed().as_micros();

    let t0 = std::time::Instant::now();
    let mut clone_groups: Vec<CloneGroup> = surviving
        .iter()
        .filter(|rg| !is_atomic_invocation_group(rg, files))
        .filter_map(|rg| build_clone_group(rg, files, &line_tables, min_lines, skip_local))
        .collect();
    let build_clone_us = t0.elapsed().as_micros();

    let t0 = std::time::Instant::now();
    clone_groups.sort_by(|a, b| {
        b.token_count
            .cmp(&a.token_count)
            .then(b.instances.len().cmp(&a.instances.len()))
    });
    let sort_us = t0.elapsed().as_micros();

    let t0 = std::time::Instant::now();
    let kept = remove_line_subsets(clone_groups);
    let line_subset_us = t0.elapsed().as_micros();

    tracing::debug!(
        raw_count,
        surviving_count = surviving.len(),
        kept_count = kept.len(),
        token_subset_us,
        line_tables_us,
        build_clone_us,
        sort_us,
        line_subset_us,
        "build_groups breakdown"
    );

    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_covered_empty_index_returns_false() {
        let index = IntervalIndex::new(1);
        assert!(!index.is_covered(0, 5, 3));
    }

    #[test]
    fn is_covered_single_interval_contained() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 10);
        assert!(index.is_covered(0, 2, 3));
    }

    #[test]
    fn is_covered_single_interval_not_contained() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        assert!(!index.is_covered(0, 3, 5));
    }

    #[test]
    fn is_covered_exact_boundary() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 10);
        assert!(index.is_covered(0, 0, 10));
    }

    #[test]
    fn is_covered_at_interval_start() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 5, 15);
        assert!(index.is_covered(0, 5, 5));
    }

    #[test]
    fn is_covered_gap_between_intervals() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 10, 20);
        assert!(!index.is_covered(0, 6, 3));
    }

    #[test]
    fn is_covered_adjacent_intervals_not_merged() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 6, 10);
        assert!(!index.is_covered(0, 4, 3));
    }

    #[test]
    fn is_covered_before_any_interval() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 10, 20);
        assert!(!index.is_covered(0, 0, 5));
    }

    #[test]
    fn is_covered_different_slots_independent() {
        let mut index = IntervalIndex::new(2);
        index.insert(0, 0, 10);
        assert!(!index.is_covered(1, 0, 5));
        assert!(index.is_covered(0, 0, 5));
    }

    #[test]
    fn insert_non_overlapping() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 10, 15);
        assert_eq!(index.slots[0], vec![(0, 5), (10, 15)]);
    }

    #[test]
    fn insert_overlapping_extends_end() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 10);
        index.insert(0, 5, 15); // overlaps [0, 10), should extend to 15
        assert_eq!(index.slots[0], vec![(0, 15)]);
    }

    #[test]
    fn insert_fully_contained_no_change() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 20);
        index.insert(0, 5, 10); // fully within [0, 20), no change
        assert_eq!(index.slots[0], vec![(0, 20)]);
    }

    #[test]
    fn insert_adjacent_merges() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 5, 10); // adjacent at boundary, prev.1 (5) >= start (5)
        assert_eq!(index.slots[0], vec![(0, 10)]);
    }

    #[test]
    fn insert_maintains_sorted_order() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 10, 20);
        index.insert(0, 0, 5); // inserted before existing
        assert_eq!(index.slots[0], vec![(0, 5), (10, 20)]);
    }

    #[test]
    fn insert_overlap_with_left_neighbor_only() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 10, 15);
        index.insert(0, 3, 8); // overlaps [0,5), does not reach [10,15)
        assert_eq!(index.slots[0], vec![(0, 8), (10, 15)]);
    }

    #[test]
    fn insert_out_of_order_overlapping_merges() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 100, 150);
        index.insert(0, 50, 110); // overlaps [100, 150) on the right
        assert_eq!(index.slots[0], vec![(50, 150)]);
        assert!(index.is_covered(0, 90, 40));
    }

    #[test]
    fn insert_spans_three_existing_intervals() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 10, 15);
        index.insert(0, 20, 25);
        index.insert(0, 3, 22);
        assert_eq!(index.slots[0], vec![(0, 25)]);
    }

    #[test]
    fn insert_multiple_merges_coalesce_overlapping_neighbors() {
        let mut index = IntervalIndex::new(1);
        index.insert(0, 0, 5);
        index.insert(0, 10, 15);
        index.insert(0, 3, 10);
        assert_eq!(index.slots[0], vec![(0, 15)]);
    }

    #[test]
    fn remove_token_subsets_empty_input() {
        let result = remove_token_subsets(vec![], 0);
        assert!(result.is_empty());
    }

    #[test]
    fn remove_token_subsets_single_group_survives() {
        let groups = vec![RawGroup {
            instances: vec![(0, 0), (1, 0)],
            length: 10,
        }];
        let result = remove_token_subsets(groups, 2);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].length, 10);
    }

    #[test]
    fn remove_token_subsets_no_subsets_both_survive() {
        let groups = vec![
            RawGroup {
                instances: vec![(0, 0), (1, 0)],
                length: 5,
            },
            RawGroup {
                instances: vec![(0, 20), (1, 20)],
                length: 5,
            },
        ];
        let result = remove_token_subsets(groups, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn remove_token_subsets_strict_subset_removed() {
        let groups = vec![
            RawGroup {
                instances: vec![(0, 0), (1, 0)],
                length: 10,
            },
            RawGroup {
                instances: vec![(0, 2), (1, 2)],
                length: 3,
            },
        ];
        let result = remove_token_subsets(groups, 2);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].length, 10);
    }

    #[test]
    fn remove_token_subsets_partial_overlap_survives() {
        let groups = vec![
            RawGroup {
                instances: vec![(0, 0), (1, 0)],
                length: 10,
            },
            RawGroup {
                instances: vec![(0, 5), (1, 5)],
                length: 7,
            },
        ];
        let result = remove_token_subsets(groups, 2);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn remove_token_subsets_subset_in_one_file_but_not_other() {
        let groups = vec![
            RawGroup {
                instances: vec![(0, 0), (1, 0)],
                length: 10,
            },
            RawGroup {
                instances: vec![(0, 2), (1, 50)],
                length: 3,
            },
        ];
        let result = remove_token_subsets(groups, 2);
        assert_eq!(result.len(), 2);
    }

    fn make_file_data(source: &str) -> FileData {
        use crate::duplicates::tokenize::FileTokens;
        FileData {
            path: PathBuf::from("test.ts"),
            hashed_tokens: vec![],
            file_tokens: FileTokens {
                tokens: vec![],
                atomic_invocation_spans: Vec::new(),
                source: source.to_string(),
                line_count: source.lines().count().max(1),
            },
            atomic_invocation_spans: Vec::new(),
        }
    }

    #[test]
    fn build_line_tables_empty_file() {
        let files = vec![make_file_data("")];
        let tables = build_line_tables(&files);
        assert_eq!(tables.len(), 1);
        assert!(tables[0].is_empty()); // No newlines in empty string
    }

    #[test]
    fn build_line_tables_single_line_no_newline() {
        let files = vec![make_file_data("hello world")];
        let tables = build_line_tables(&files);
        assert!(tables[0].is_empty()); // No newlines
    }

    #[test]
    fn build_line_tables_multiple_lines() {
        let files = vec![make_file_data("abc\ndef\nghi")];
        let tables = build_line_tables(&files);
        assert_eq!(tables[0], vec![3, 7]);
    }

    #[test]
    fn build_line_tables_trailing_newline() {
        let files = vec![make_file_data("abc\ndef\n")];
        let tables = build_line_tables(&files);
        assert_eq!(tables[0], vec![3, 7]);
    }

    #[test]
    fn build_line_tables_multiple_files() {
        let files = vec![make_file_data("a\nb"), make_file_data("x\ny\nz")];
        let tables = build_line_tables(&files);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0], vec![1]);
        assert_eq!(tables[1], vec![1, 3]);
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test span values are trivially small"
    )]
    fn make_test_file_data(path: &str, source: &str, num_tokens: usize) -> FileData {
        use crate::duplicates::normalize::HashedToken;
        use crate::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};
        use oxc_span::Span;

        let tokens: Vec<SourceToken> = (0..num_tokens)
            .map(|i| SourceToken {
                kind: TokenKind::Identifier(format!("t{i}")),
                span: Span::new((i * 3) as u32, (i * 3 + 2) as u32),
            })
            .collect();

        let hashed: Vec<HashedToken> = (0..num_tokens)
            .map(|i| HashedToken {
                hash: i as u64,
                original_index: i,
            })
            .collect();

        FileData {
            path: PathBuf::from(path),
            hashed_tokens: hashed,
            file_tokens: FileTokens {
                tokens,
                atomic_invocation_spans: Vec::new(),
                source: source.to_string(),
                line_count: source.lines().count().max(1),
            },
            atomic_invocation_spans: Vec::new(),
        }
    }

    #[test]
    fn build_clone_group_returns_none_for_single_instance() {
        let files = vec![
            make_test_file_data("a.ts", "aa\nbb\ncc\ndd\nee", 5),
            make_test_file_data("b.ts", "aa\nbb\ncc\ndd\nee", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0)], // only one instance
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 1, false);
        assert!(result.is_none());
    }

    #[test]
    fn build_clone_group_returns_none_below_min_lines() {
        let files = vec![
            make_test_file_data("a.ts", "aabbccddeeff", 5), // single line, no newlines
            make_test_file_data("b.ts", "aabbccddeeff", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0), (1, 0)],
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 5, false);
        assert!(result.is_none());
    }

    #[test]
    fn build_clone_group_skip_local_filters_same_dir() {
        let files = vec![
            make_test_file_data("src/a.ts", "aa\nbb\ncc\ndd\nee", 5),
            make_test_file_data("src/b.ts", "aa\nbb\ncc\ndd\nee", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0), (1, 0)],
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 1, true);
        assert!(result.is_none());
    }

    #[test]
    fn build_clone_group_skip_local_keeps_cross_dir() {
        let files = vec![
            make_test_file_data("src/a.ts", "aa\nbb\ncc\ndd\nee", 5),
            make_test_file_data("lib/b.ts", "aa\nbb\ncc\ndd\nee", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0), (1, 0)],
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 1, false);
        assert!(result.is_some());
        let group = result.unwrap();
        assert_eq!(group.instances.len(), 2);
        assert_eq!(group.token_count, 3);
    }

    #[test]
    fn build_clone_group_valid_group_construction() {
        let files = vec![
            make_test_file_data("a.ts", "aa\nbb\ncc\ndd\nee", 5),
            make_test_file_data("b.ts", "aa\nbb\ncc\ndd\nee", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0), (1, 0)],
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 1, false);
        assert!(result.is_some());
        let group = result.unwrap();
        assert_eq!(group.instances.len(), 2);
        assert_eq!(group.token_count, 3);
        assert!(group.instances[0].file <= group.instances[1].file);
    }

    #[test]
    fn build_clone_group_deduplicates_same_offset() {
        let files = vec![
            make_test_file_data("a.ts", "aa\nbb\ncc\ndd\nee", 5),
            make_test_file_data("b.ts", "aa\nbb\ncc\ndd\nee", 5),
        ];
        let line_tables = build_line_tables(&files);
        let rg = RawGroup {
            instances: vec![(0, 0), (0, 0), (1, 0)],
            length: 3,
        };
        let result = build_clone_group(&rg, &files, &line_tables, 1, false);
        assert!(result.is_some());
        let group = result.unwrap();
        assert_eq!(group.instances.len(), 2);
    }

    fn make_clone_group(instances: Vec<(&str, usize, usize)>, token_count: usize) -> CloneGroup {
        CloneGroup {
            instances: instances
                .into_iter()
                .map(|(file, start, end)| CloneInstance {
                    file: PathBuf::from(file),
                    start_line: start,
                    end_line: end,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                })
                .collect(),
            token_count,
            line_count: 0, // not used by remove_line_subsets
        }
    }

    #[test]
    fn remove_line_subsets_empty_input() {
        let result = remove_line_subsets(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn remove_line_subsets_single_group_survives() {
        let groups = vec![make_clone_group(vec![("a.ts", 1, 10), ("b.ts", 1, 10)], 20)];
        let result = remove_line_subsets(groups);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn remove_line_subsets_no_subsets_all_survive() {
        let groups = vec![
            make_clone_group(vec![("a.ts", 1, 10), ("b.ts", 1, 10)], 20),
            make_clone_group(vec![("a.ts", 50, 60), ("b.ts", 50, 60)], 15),
        ];
        let result = remove_line_subsets(groups);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn remove_line_subsets_nested_clone_removed() {
        let groups = vec![
            make_clone_group(vec![("a.ts", 1, 20), ("b.ts", 1, 20)], 50),
            make_clone_group(vec![("a.ts", 5, 10), ("b.ts", 5, 10)], 15),
        ];
        let result = remove_line_subsets(groups);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].token_count, 50); // Only the larger group survives
    }

    #[test]
    fn remove_line_subsets_partial_overlap_survives() {
        let groups = vec![
            make_clone_group(vec![("a.ts", 1, 20), ("b.ts", 1, 20)], 50),
            make_clone_group(vec![("a.ts", 5, 10), ("b.ts", 50, 60)], 15),
        ];
        let result = remove_line_subsets(groups);
        assert_eq!(result.len(), 2); // B survives because b.ts instance is not contained
    }

    #[test]
    fn remove_line_subsets_different_files_not_subset() {
        let groups = vec![
            make_clone_group(vec![("a.ts", 1, 20), ("b.ts", 1, 20)], 50),
            make_clone_group(vec![("c.ts", 1, 10), ("d.ts", 1, 10)], 15),
        ];
        let result = remove_line_subsets(groups);
        assert_eq!(result.len(), 2);
    }
}
