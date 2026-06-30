//! Scope and diff filters for health output sections.

use std::path::{Path, PathBuf};

use fallow_config::ResolvedConfig;
use fallow_output::{
    ComplexityViolation, CoverageGaps, HotspotEntry, LargeFunctionEntry, RefactoringTarget,
};
use fallow_types::discover::DiscoveredFile;
use rustc_hash::FxHashSet;

use super::runtime_filter::relative_to_root;
use super::scoring;

/// Drop complexity findings whose function body span does NOT overlap any
/// added line in the supplied diff. The function spans
/// `[line..=line + line_count - 1]`: a hotspot that starts before the
/// diff but extends into a touched line counts as overlap. `line_count`
/// of zero collapses to `[line..=line]` so older fixture rows without
/// extents do not silently match every diff.
///
/// Paths that cannot be expressed relative to `root` are retained rather than
/// silently dropped: surfacing an unfilterable path is better than hiding it.
pub(super) fn filter_complexity_findings_by_diff(
    findings: &mut Vec<ComplexityViolation>,
    diff_index: &fallow_output::DiffIndex,
    root: &Path,
) {
    findings.retain(|finding| {
        let Some(rel) = relative_to_root(&finding.path, root) else {
            return true;
        };
        diff_index.range_overlaps_added(
            &rel,
            u64::from(finding.line),
            finding_body_end_line(finding.line, finding.line_count),
        )
    });
}

/// Drop hotspot entries whose file is not touched by the supplied diff.
pub(super) fn filter_hotspots_by_diff(
    hotspots: &mut Vec<HotspotEntry>,
    diff_index: &fallow_output::DiffIndex,
    root: &Path,
) {
    hotspots.retain(|hotspot| match relative_to_root(&hotspot.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop refactoring targets whose file is not touched by the diff.
pub(super) fn filter_refactoring_targets_by_diff(
    targets: &mut Vec<RefactoringTarget>,
    diff_index: &fallow_output::DiffIndex,
    root: &Path,
) {
    targets.retain(|target| match relative_to_root(&target.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop large-function entries whose body span does NOT overlap any added line
/// in the supplied diff.
pub(super) fn filter_large_functions_by_diff(
    entries: &mut Vec<LargeFunctionEntry>,
    diff_index: &fallow_output::DiffIndex,
    root: &Path,
) {
    entries.retain(|entry| {
        let Some(rel) = relative_to_root(&entry.path, root) else {
            return true;
        };
        diff_index.range_overlaps_added(
            &rel,
            u64::from(entry.line),
            finding_body_end_line(entry.line, entry.line_count),
        )
    });
}

pub(super) fn collect_candidate_paths(
    files: &[DiscoveredFile],
    config: &ResolvedConfig,
    changed_files: Option<&FxHashSet<PathBuf>>,
    ws_roots: Option<&[PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> FxHashSet<PathBuf> {
    files
        .iter()
        .filter(|file| {
            path_in_health_scope(&file.path, config, changed_files, ws_roots, ignore_set)
        })
        .map(|file| file.path.clone())
        .collect()
}

pub(super) fn filter_files_to_paths(
    files: &[DiscoveredFile],
    candidate_paths: &FxHashSet<PathBuf>,
) -> Vec<DiscoveredFile> {
    files
        .iter()
        .filter(|file| candidate_paths.contains(&file.path))
        .cloned()
        .collect()
}

fn path_in_health_scope(
    path: &Path,
    config: &ResolvedConfig,
    changed_files: Option<&FxHashSet<PathBuf>>,
    ws_roots: Option<&[PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> bool {
    if let Some(changed) = changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = ws_roots
        && !ws.iter().any(|root| path.starts_with(root))
    {
        return false;
    }
    if !ignore_set.is_empty() {
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            return false;
        }
    }
    true
}

pub(super) fn filter_coverage_gaps(
    coverage_gaps: &mut CoverageGaps,
    runtime_paths: &mut Vec<PathBuf>,
    config: &ResolvedConfig,
    changed_files: Option<&FxHashSet<PathBuf>>,
    ws_roots: Option<&[PathBuf]>,
    ignore_set: &globset::GlobSet,
) {
    runtime_paths
        .retain(|path| path_in_health_scope(path, config, changed_files, ws_roots, ignore_set));
    coverage_gaps.files.retain(|item| {
        path_in_health_scope(&item.file.path, config, changed_files, ws_roots, ignore_set)
    });
    coverage_gaps.exports.retain(|item| {
        path_in_health_scope(
            &item.export.path,
            config,
            changed_files,
            ws_roots,
            ignore_set,
        )
    });

    runtime_paths.sort();
    runtime_paths.dedup();

    let runtime_files = runtime_paths.len();
    let untested_files = coverage_gaps.files.len();
    let covered_files = runtime_files.saturating_sub(untested_files);
    coverage_gaps.summary = scoring::build_coverage_summary(
        runtime_files,
        covered_files,
        untested_files,
        coverage_gaps.exports.len(),
    );
}

const fn finding_body_end_line(line: u32, line_count: u32) -> u64 {
    let start = line as u64;
    if line_count == 0 {
        start
    } else {
        start + line_count as u64 - 1
    }
}
