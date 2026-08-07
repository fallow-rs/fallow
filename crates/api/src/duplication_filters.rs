//! Duplication post-processing helpers for programmatic API runs.

use std::path::{Path, PathBuf};

use fallow_output::DiffIndex;
use fallow_types::duplicates::{CloneInstance, DuplicationReport};

pub fn filter_by_diff(report: &mut DuplicationReport, diff_index: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = diff_index.key_for(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff_index.range_overlaps_added(&rel, start, end)
    };
    report
        .clone_groups
        .retain(|group| group.instances.iter().any(instance_overlaps));
    rebuild_duplication_derived_fields(report, root);
}

pub fn filter_by_workspaces(
    report: &mut DuplicationReport,
    workspace_roots: &[PathBuf],
    root: &Path,
) {
    for group in &mut report.clone_groups {
        let before = group.instances.len();
        group.instances.retain(|instance| {
            workspace_roots
                .iter()
                .any(|workspace_root| instance.file.starts_with(workspace_root))
        });
        if group.instances.len() >= 2 && group.instances.len() != before {
            fallow_engine::duplicates::refresh_clone_group_metrics(group);
        }
    }
    report
        .clone_groups
        .retain(|group| group.instances.len() >= 2);
    rebuild_duplication_derived_fields(report, root);
}

pub fn apply_top(report: &mut DuplicationReport, n: usize, root: &Path) {
    report.sort();
    report.clone_groups.truncate(n);
    fallow_engine::duplicates::refresh_clone_families(report, root);
    report.stats.clone_groups = report.clone_groups.len();
    report.stats.clone_instances = report
        .clone_groups
        .iter()
        .map(|group| group.instances.len())
        .sum();
    report.sort();
}

fn rebuild_duplication_derived_fields(report: &mut DuplicationReport, root: &Path) {
    fallow_engine::duplicates::refresh_clone_families(report, root);
    report.stats = fallow_engine::duplicates::recompute_stats(report);
    report.sort();
}

#[cfg(test)]
mod tests {
    use fallow_types::duplicates::{CloneGroup, DuplicationStats};

    use super::*;

    fn instance(file: &str) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(files: &[&str], token_count: usize) -> CloneGroup {
        CloneGroup {
            instances: files.iter().map(|file| instance(file)).collect(),
            token_count,
            line_count: 10,
            similarity: None,
        }
    }

    #[test]
    fn top_uses_spread_ranking_and_preserves_scoped_metrics() {
        let local = group(&["/repo/src/a.ts", "/repo/src/b.ts"], 100);
        let distant = group(&["/repo/packages/a/a.ts", "/repo/packages/b/b.ts"], 100);
        let stats = DuplicationStats {
            total_files: 4,
            files_with_clones: 4,
            total_lines: 1_000,
            duplicated_lines: 400,
            total_tokens: 5_000,
            duplicated_tokens: 400,
            clone_groups: 2,
            clone_instances: 4,
            duplication_percentage: 40.0,
            clone_groups_below_min_occurrences: 1,
            clone_groups_ignored: 2,
            near_candidates_skipped: 3,
        };
        let mut report = DuplicationReport {
            clone_groups: vec![local, distant],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats,
        };

        apply_top(&mut report, 1, Path::new("/repo"));

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.clone_groups[0].spread(), 2);
        assert_eq!(report.stats.clone_groups, 1);
        assert_eq!(report.stats.clone_instances, 2);
        assert_eq!(report.stats.duplicated_lines, 400);
        assert_eq!(report.stats.duplicated_tokens, 400);
        assert!((report.stats.duplication_percentage - 40.0).abs() < f64::EPSILON);
        assert_eq!(report.stats.clone_groups_below_min_occurrences, 1);
        assert_eq!(report.stats.clone_groups_ignored, 2);
        assert_eq!(report.stats.near_candidates_skipped, 3);
    }

    #[test]
    fn workspace_filter_refreshes_surviving_near_similarity() {
        let source = "function same(value) { const adjusted = value + 2; return adjusted * 2; }";
        let mut near = group(&["/repo/a/one.ts", "/repo/a/two.ts", "/repo/b/three.ts"], 1);
        near.similarity = Some(0.8);
        for instance in &mut near.instances {
            instance.fragment = source.to_string();
        }
        let mut report = DuplicationReport {
            clone_groups: vec![near],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats {
                total_files: 3,
                total_lines: 100,
                total_tokens: 1_000,
                ..DuplicationStats::default()
            },
        };

        filter_by_workspaces(&mut report, &[PathBuf::from("/repo/a")], Path::new("/repo"));

        assert_eq!(report.clone_groups[0].instances.len(), 2);
        assert_eq!(report.clone_groups[0].similarity, Some(1.0));
        assert!(report.clone_groups[0].token_count > 1);
    }
}
