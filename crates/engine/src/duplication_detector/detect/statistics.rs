//! Step 7: Aggregation and reporting, compute duplication statistics.

use std::path::Path;

use rustc_hash::FxHashMap;

use crate::duplicates::types::{CloneGroup, DuplicationStats};

/// Compute aggregate duplication statistics.
pub(super) fn compute_stats(
    clone_groups: &[CloneGroup],
    total_files: usize,
    total_lines: usize,
    total_tokens: usize,
) -> DuplicationStats {
    let mut file_dup_ranges: FxHashMap<&Path, Vec<(usize, usize)>> = FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in clone_groups {
        for instance in &group.instances {
            clone_instances += 1;
            let ranges = file_dup_ranges.entry(&instance.file).or_default();
            if instance.start_line <= instance.end_line {
                ranges.push((instance.start_line, instance.end_line));
            }
        }
        if group.instances.len() > 1 {
            duplicated_tokens += group.token_count * (group.instances.len() - 1);
        }
    }

    let dup_line_count = file_dup_ranges
        .values_mut()
        .map(|ranges| count_covered_lines(ranges))
        .sum();
    let duplication_percentage = if total_lines > 0 {
        (dup_line_count as f64 / total_lines as f64) * 100.0
    } else {
        0.0
    };

    let duplicated_tokens = duplicated_tokens.min(total_tokens);

    DuplicationStats {
        total_files,
        files_with_clones: file_dup_ranges.len(),
        total_lines,
        duplicated_lines: dup_line_count,
        total_tokens,
        duplicated_tokens,
        clone_groups: clone_groups.len(),
        clone_instances,
        duplication_percentage,
        clone_groups_below_min_occurrences: 0,
        clone_groups_ignored: 0,
        near_candidates_skipped: 0,
    }
}

fn count_covered_lines(ranges: &mut [(usize, usize)]) -> usize {
    ranges.sort_unstable();
    let Some(&(mut current_start, mut current_end)) = ranges.first() else {
        return 0;
    };

    let mut covered = 0usize;
    for &(start, end) in &ranges[1..] {
        if start <= current_end.saturating_add(1) {
            current_end = current_end.max(end);
            continue;
        }

        covered += current_end - current_start + 1;
        (current_start, current_end) = (start, end);
    }

    covered + current_end - current_start + 1
}
