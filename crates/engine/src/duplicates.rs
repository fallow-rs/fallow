//! Duplication result types exposed through the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::DuplicatesConfig;
use fallow_types::discover::DiscoveredFile;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{core_backend, results::DuplicationAnalysis};

pub const FINGERPRINT_PREFIX: &str = "dup:";

pub type CloneGroup = fallow_types::duplicates::CloneGroup;
pub type CloneInstance = fallow_types::duplicates::CloneInstance;
pub type DefaultIgnoreSkips = fallow_types::duplicates::DefaultIgnoreSkips;
pub type DuplicationReport = fallow_types::duplicates::DuplicationReport;
pub type DuplicationStats = fallow_types::duplicates::DuplicationStats;

/// Report-scoped clone fingerprint assignment exposed through the engine boundary.
#[derive(Debug, Clone)]
pub struct CloneFingerprintSet {
    inner: core_backend::BackendCloneFingerprintSet,
}

impl CloneFingerprintSet {
    /// Assign collision-free fingerprints for the report's clone groups.
    #[must_use]
    pub fn from_groups(groups: &[CloneGroup]) -> Self {
        Self {
            inner: core_backend::BackendCloneFingerprintSet::from_groups(groups),
        }
    }

    /// Return the assigned fingerprint for a clone group.
    #[must_use]
    pub fn fingerprint_for_group(&self, group: &CloneGroup) -> String {
        self.inner.fingerprint_for_group(group)
    }

    /// Return the assigned fingerprint for clone-group parts.
    #[must_use]
    pub fn fingerprint_for_parts(
        &self,
        instances: &[CloneInstance],
        token_count: usize,
        line_count: usize,
    ) -> String {
        self.inner
            .fingerprint_for_parts(instances, token_count, line_count)
    }

    /// Find the group addressed by an assigned fingerprint.
    #[must_use]
    pub fn find_group<'a>(
        &self,
        groups: &'a [CloneGroup],
        fingerprint: &str,
    ) -> Option<&'a CloneGroup> {
        self.inner.find_group(groups, fingerprint)
    }
}

/// Compute the stable fingerprint for a clone group.
#[must_use]
pub fn clone_fingerprint(instances: &[CloneInstance]) -> String {
    core_backend::clone_fingerprint(instances)
}

/// Compute a clone fingerprint directly from a representative source fragment.
#[must_use]
pub fn fingerprint_for_fragment(fragment: &str) -> String {
    core_backend::fingerprint_for_fragment(fragment)
}

/// Return the best-effort dominant identifier for a clone group.
#[must_use]
pub fn dominant_identifier(group: &CloneGroup) -> Option<String> {
    core_backend::dominant_identifier(group)
}

/// Refresh clone-family and mirrored-directory fields after clone groups change.
pub fn refresh_clone_families(report: &mut DuplicationReport, root: &Path) {
    core_backend::refresh_clone_families(report, root);
}

/// Recompute duplication statistics after clone groups have been filtered.
///
/// Uses per-file line deduplication, matching the detector's stats model, so
/// overlapping clone instances do not inflate the duplicated line count.
#[must_use]
pub fn recompute_stats(report: &DuplicationReport) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut file_dup_lines: FxHashMap<&Path, FxHashSet<usize>> = FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in &report.clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        duplicated_tokens += group.token_count * group.instances.len();
    }

    let duplicated_lines: usize = file_dup_lines.values().map(FxHashSet::len).sum();

    DuplicationStats {
        total_files: report.stats.total_files,
        files_with_clones: files_with_clones.len(),
        total_lines: report.stats.total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens,
        clone_groups: report.clone_groups.len(),
        clone_instances,
        duplication_percentage: if report.stats.total_lines > 0 {
            (duplicated_lines as f64 / report.stats.total_lines as f64) * 100.0
        } else {
            0.0
        },
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
    }
}

/// Compare two JS/TS sources by duplicate-token kind sequence.
///
/// This keeps CLI audit's non-behavioral change check from depending on the
/// tokenizer module shape.
#[must_use]
pub fn source_token_kinds_equivalent(
    path: &Path,
    current: &str,
    base: &str,
    cross_language: bool,
) -> bool {
    core_backend::source_token_kinds_equivalent(path, current, base, cross_language)
}

/// Run duplication detection on a discovered file set.
#[must_use]
pub fn find_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> DuplicationReport {
    core_backend::find_duplicates(root, files, config)
}

/// Run cached duplication detection inside the engine boundary.
#[must_use]
pub fn find_duplicates_cached(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_dir: &Path,
) -> DuplicationReport {
    core_backend::find_duplicates_cached(root, files, config, cache_dir)
}

/// Run duplication detection and include metadata about built-in ignored files.
#[must_use]
pub fn find_duplicates_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    core_backend::find_duplicates_with_defaults(root, files, config, cache_dir)
}

/// Run focused duplication detection and include metadata about built-in ignored files.
#[must_use]
pub fn find_duplicates_touching_files_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    changed_files: &[PathBuf],
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    core_backend::find_duplicates_touching_files_with_defaults(
        root,
        files,
        config,
        changed_files,
        cache_dir,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn instance(file: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn report(clone_groups: Vec<CloneGroup>) -> DuplicationReport {
        DuplicationReport {
            clone_groups,
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats {
                total_files: 3,
                total_lines: 100,
                total_tokens: 1_000,
                clone_groups_below_min_occurrences: 4,
                ..DuplicationStats::default()
            },
        }
    }

    #[test]
    fn recompute_stats_deduplicates_overlapping_lines_per_file() {
        let report = report(vec![
            CloneGroup {
                instances: vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 20, 24)],
                token_count: 30,
                line_count: 10,
            },
            CloneGroup {
                instances: vec![instance("src/a.ts", 5, 12), instance("src/c.ts", 40, 44)],
                token_count: 20,
                line_count: 8,
            },
        ]);

        let stats = recompute_stats(&report);

        assert_eq!(stats.total_files, 3);
        assert_eq!(stats.files_with_clones, 3);
        assert_eq!(stats.total_lines, 100);
        assert_eq!(stats.duplicated_lines, 22);
        assert_eq!(stats.total_tokens, 1_000);
        assert_eq!(stats.duplicated_tokens, 100);
        assert_eq!(stats.clone_groups, 2);
        assert_eq!(stats.clone_instances, 4);
        assert!((stats.duplication_percentage - 22.0).abs() < f64::EPSILON);
        assert_eq!(stats.clone_groups_below_min_occurrences, 4);
    }

    #[test]
    fn recompute_stats_handles_zero_total_lines() {
        let mut report = report(vec![CloneGroup {
            instances: vec![instance("src/a.ts", 1, 1)],
            token_count: 5,
            line_count: 1,
        }]);
        report.stats.total_lines = 0;

        let stats = recompute_stats(&report);

        assert_eq!(stats.duplicated_lines, 1);
        assert!(stats.duplication_percentage.abs() < f64::EPSILON);
    }

    #[test]
    fn clone_fingerprint_set_delegates_without_leaking_core_type() {
        let groups = vec![CloneGroup {
            instances: vec![
                CloneInstance {
                    fragment: "const value = 1;".to_string(),
                    ..instance("src/a.ts", 1, 1)
                },
                CloneInstance {
                    fragment: "const value = 1;".to_string(),
                    ..instance("src/b.ts", 2, 2)
                },
            ],
            token_count: 5,
            line_count: 1,
        }];
        let fingerprints = CloneFingerprintSet::from_groups(&groups);
        let fingerprint = fingerprints.fingerprint_for_group(&groups[0]);

        assert!(fingerprint.starts_with(FINGERPRINT_PREFIX));
        assert!(fingerprints.find_group(&groups, &fingerprint).is_some());
    }
}
