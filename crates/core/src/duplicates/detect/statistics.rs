//! Step 7: Aggregation and reporting — compute duplication statistics.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::duplicates::types::{CloneGroup, DuplicationStats};

/// Compute aggregate duplication statistics.
pub(super) fn compute_stats(
    clone_groups: &[CloneGroup],
    total_files: usize,
    total_lines: usize,
    total_tokens: usize,
) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    // Group duplicated lines by file to avoid cloning PathBuf per line.
    let mut file_dup_lines: FxHashMap<&Path, FxHashSet<usize>> = FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        // Each instance contributes token_count duplicated tokens,
        // but only count duplicates (all instances beyond the first).
        if group.instances.len() > 1 {
            duplicated_tokens += group.token_count * (group.instances.len() - 1);
        }
    }

    let dup_line_count: usize = file_dup_lines.values().map(FxHashSet::len).sum();
    let duplication_percentage = if total_lines > 0 {
        (dup_line_count as f64 / total_lines as f64) * 100.0
    } else {
        0.0
    };

    // Cap duplicated_tokens to total_tokens to avoid impossible values
    // when overlapping clone groups double-count the same token positions.
    let duplicated_tokens = duplicated_tokens.min(total_tokens);

    DuplicationStats {
        total_files,
        files_with_clones: files_with_clones.len(),
        total_lines,
        duplicated_lines: dup_line_count,
        total_tokens,
        duplicated_tokens,
        clone_groups: clone_groups.len(),
        clone_instances,
        duplication_percentage,
        clone_groups_below_min_occurrences: 0,
    }
}
