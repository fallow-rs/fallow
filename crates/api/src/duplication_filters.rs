//! Duplication post-processing helpers for programmatic API runs.

use std::path::{Path, PathBuf};

use fallow_output::{DiffIndex, relative_to_diff_path};
use fallow_types::duplicates::{CloneInstance, DuplicationReport};

pub fn filter_by_diff(report: &mut DuplicationReport, diff_index: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = relative_to_diff_path(&instance.file, root) else {
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
    report.clone_groups.retain(|group| {
        group.instances.iter().any(|instance| {
            workspace_roots
                .iter()
                .any(|workspace_root| instance.file.starts_with(workspace_root))
        })
    });
    rebuild_duplication_derived_fields(report, root);
}

pub fn apply_top(report: &mut DuplicationReport, n: usize, root: &Path) {
    report.clone_groups.sort_by(|a, b| {
        b.instances
            .len()
            .cmp(&a.instances.len())
            .then(b.line_count.cmp(&a.line_count))
            .then_with(|| match (a.instances.first(), b.instances.first()) {
                (Some(ai), Some(bi)) => ai
                    .file
                    .cmp(&bi.file)
                    .then(ai.start_line.cmp(&bi.start_line)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    report.clone_groups.truncate(n);
    rebuild_duplication_derived_fields(report, root);
    report.sort();
}

fn rebuild_duplication_derived_fields(report: &mut DuplicationReport, root: &Path) {
    fallow_engine::refresh_clone_families(report, root);
    report.stats = fallow_engine::recompute_stats(report);
}
