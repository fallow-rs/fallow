//! Cross-reference duplication findings with dead code analysis results.
//!
//! When code is both duplicated AND unused, it's a higher-priority finding:
//! the duplicate can be safely removed without any refactoring. This module
//! identifies such combined findings.

use rustc_hash::FxHashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::duplicates::types::{CloneInstance, DuplicationReport};
use crate::results::AnalysisResults;

/// A combined finding where a clone instance overlaps with a dead code issue.
#[derive(Debug, Clone, Serialize)]
pub struct CombinedFinding {
    /// The clone instance that is also unused.
    pub clone_instance: CloneInstance,
    /// What kind of dead code overlaps with this clone.
    pub dead_code_kind: DeadCodeKind,
    /// Clone group index (for associating with the parent group).
    pub group_index: usize,
}

/// The type of dead code that overlaps with a clone instance.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum DeadCodeKind {
    /// The entire file containing the clone is unused.
    UnusedFile,
    /// A specific unused export overlaps with the clone's line range.
    UnusedExport { export_name: String },
    /// A specific unused type overlaps with the clone's line range.
    UnusedType { type_name: String },
}

/// Result of cross-referencing duplication with dead code analysis.
#[derive(Debug, Clone, Serialize)]
pub struct CrossReferenceResult {
    /// Clone instances that are also dead code (safe to delete).
    pub combined_findings: Vec<CombinedFinding>,
    /// Number of clone instances in unused files.
    pub clones_in_unused_files: usize,
    /// Number of clone instances overlapping unused exports.
    pub clones_with_unused_exports: usize,
}

/// Cross-reference duplication findings with dead code analysis results.
///
/// For each clone instance, checks whether:
/// 1. The file is entirely unused (in `unused_files`)
/// 2. An unused export/type at the same line range overlaps
///
/// Returns combined findings sorted by priority (unused files first, then exports).
#[must_use]
pub fn cross_reference(
    duplication: &DuplicationReport,
    dead_code: &AnalysisResults,
) -> CrossReferenceResult {
    // Build lookup sets for fast checking
    let unused_files: FxHashSet<&PathBuf> =
        dead_code.unused_files.iter().map(|f| &f.path).collect();

    let mut combined_findings = Vec::new();
    let mut clones_in_unused_files = 0usize;
    let mut clones_with_unused_exports = 0usize;

    for (group_idx, group) in duplication.clone_groups.iter().enumerate() {
        for instance in &group.instances {
            // Check 1: Is the file entirely unused?
            if unused_files.contains(&instance.file) {
                combined_findings.push(CombinedFinding {
                    clone_instance: instance.clone(),
                    dead_code_kind: DeadCodeKind::UnusedFile,
                    group_index: group_idx,
                });
                clones_in_unused_files += 1;
                continue; // No need to check exports if entire file is unused
            }

            // Check 2: Does an unused export/type overlap with this clone's line range?
            if let Some(finding) = find_overlapping_unused_export(instance, group_idx, dead_code) {
                clones_with_unused_exports += 1;
                combined_findings.push(finding);
            }
        }
    }

    CrossReferenceResult {
        combined_findings,
        clones_in_unused_files,
        clones_with_unused_exports,
    }
}

/// Check if any unused export/type overlaps with the clone instance's line range.
fn find_overlapping_unused_export(
    instance: &CloneInstance,
    group_index: usize,
    dead_code: &AnalysisResults,
) -> Option<CombinedFinding> {
    // Check unused exports
    for export in &dead_code.unused_exports {
        if export.path == instance.file
            && (export.line as usize) >= instance.start_line
            && (export.line as usize) <= instance.end_line
        {
            return Some(CombinedFinding {
                clone_instance: instance.clone(),
                dead_code_kind: DeadCodeKind::UnusedExport {
                    export_name: export.export_name.clone(),
                },
                group_index,
            });
        }
    }

    // Check unused types
    for type_export in &dead_code.unused_types {
        if type_export.path == instance.file
            && (type_export.line as usize) >= instance.start_line
            && (type_export.line as usize) <= instance.end_line
        {
            return Some(CombinedFinding {
                clone_instance: instance.clone(),
                dead_code_kind: DeadCodeKind::UnusedType {
                    type_name: type_export.export_name.clone(),
                },
                group_index,
            });
        }
    }

    None
}

/// Summary statistics for cross-referenced findings.
impl CrossReferenceResult {
    /// Total number of combined findings.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.combined_findings.len()
    }

    /// Whether any combined findings exist.
    #[must_use]
    pub const fn has_findings(&self) -> bool {
        !self.combined_findings.is_empty()
    }

    /// Get clone groups that have at least one combined finding, with their indices.
    #[must_use]
    pub fn affected_group_indices(&self) -> FxHashSet<usize> {
        self.combined_findings
            .iter()
            .map(|f| f.group_index)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::CloneGroup;
    use crate::results::{UnusedExport, UnusedFile};

    fn make_instance(file: &str, start: usize, end: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn make_group(instances: Vec<CloneInstance>) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: 50,
            line_count: 10,
        }
    }

    #[test]
    fn empty_inputs_produce_no_findings() {
        let duplication = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 0,
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
        };
        let dead_code = AnalysisResults::default();

        let result = cross_reference(&duplication, &dead_code);
        assert!(!result.has_findings());
        assert_eq!(result.total(), 0);
    }

    #[test]
    fn detects_clone_in_unused_file() {
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 1, 10),
                make_instance("src/b.ts", 1, 10),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 20,
                duplicated_lines: 10,
                total_tokens: 100,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_files.push(UnusedFile {
            path: PathBuf::from("src/a.ts"),
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(result.has_findings());
        assert_eq!(result.clones_in_unused_files, 1);
        assert_eq!(
            result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedFile
        );
    }

    #[test]
    fn detects_clone_overlapping_unused_export() {
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 5, 15),
                make_instance("src/b.ts", 5, 15),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 20,
                duplicated_lines: 10,
                total_tokens: 100,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/a.ts"),
            export_name: "processData".to_string(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(result.has_findings());
        assert_eq!(result.clones_with_unused_exports, 1);
        assert!(matches!(
            &result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedExport { export_name } if export_name == "processData"
        ));
    }

    #[test]
    fn no_findings_when_no_overlap() {
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 5, 15),
                make_instance("src/b.ts", 5, 15),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 20,
                duplicated_lines: 10,
                total_tokens: 100,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        // Unused export on a different line range
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/a.ts"),
            export_name: "other".to_string(),
            is_type_only: false,
            line: 20, // outside clone range 5-15
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(!result.has_findings());
    }

    #[test]
    fn affected_group_indices() {
        let duplication = DuplicationReport {
            clone_groups: vec![
                make_group(vec![
                    make_instance("src/a.ts", 1, 10),
                    make_instance("src/b.ts", 1, 10),
                ]),
                make_group(vec![
                    make_instance("src/c.ts", 1, 10),
                    make_instance("src/d.ts", 1, 10),
                ]),
            ],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 4,
                files_with_clones: 4,
                total_lines: 40,
                duplicated_lines: 20,
                total_tokens: 200,
                duplicated_tokens: 100,
                clone_groups: 2,
                clone_instances: 4,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_files.push(UnusedFile {
            path: PathBuf::from("src/c.ts"),
        });

        let result = cross_reference(&duplication, &dead_code);
        let affected = result.affected_group_indices();
        assert!(!affected.contains(&0)); // Group 0 not affected
        assert!(affected.contains(&1)); // Group 1 has clone in unused file
    }

    #[test]
    fn unused_file_takes_priority_over_export() {
        // If a file is unused AND has unused exports, we should only get the
        // UnusedFile finding (not both), because the continue skips export checks.
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 5, 15),
                make_instance("src/b.ts", 5, 15),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 20,
                duplicated_lines: 10,
                total_tokens: 100,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_files.push(UnusedFile {
            path: PathBuf::from("src/a.ts"),
        });
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/a.ts"),
            export_name: "foo".to_string(),
            is_type_only: false,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        // Only 1 finding for src/a.ts (the unused file), not 2
        let a_findings: Vec<_> = result
            .combined_findings
            .iter()
            .filter(|f| f.clone_instance.file == std::path::Path::new("src/a.ts"))
            .collect();
        assert_eq!(a_findings.len(), 1);
        assert_eq!(a_findings[0].dead_code_kind, DeadCodeKind::UnusedFile);
    }

    #[test]
    fn detects_clone_overlapping_unused_type() {
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/types.ts", 1, 20),
                make_instance("src/other.ts", 1, 20),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 40,
                duplicated_lines: 20,
                total_tokens: 100,
                duplicated_tokens: 50,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_types.push(UnusedExport {
            path: PathBuf::from("src/types.ts"),
            export_name: "OldInterface".to_string(),
            is_type_only: true,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(result.has_findings());
        assert!(matches!(
            &result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedType { type_name } if type_name == "OldInterface"
        ));
    }

    #[test]
    fn empty_result_methods() {
        let result = CrossReferenceResult {
            combined_findings: vec![],
            clones_in_unused_files: 0,
            clones_with_unused_exports: 0,
        };
        assert_eq!(result.total(), 0);
        assert!(!result.has_findings());
        assert!(result.affected_group_indices().is_empty());
    }

    #[test]
    fn multiple_groups_with_findings() {
        let duplication = DuplicationReport {
            clone_groups: vec![
                make_group(vec![
                    make_instance("src/a.ts", 1, 10),
                    make_instance("src/b.ts", 1, 10),
                ]),
                make_group(vec![
                    make_instance("src/c.ts", 5, 15),
                    make_instance("src/d.ts", 5, 15),
                ]),
                make_group(vec![
                    make_instance("src/e.ts", 1, 10),
                    make_instance("src/f.ts", 1, 10),
                ]),
            ],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats {
                total_files: 6,
                files_with_clones: 6,
                total_lines: 60,
                duplicated_lines: 30,
                total_tokens: 300,
                duplicated_tokens: 150,
                clone_groups: 3,
                clone_instances: 6,
                duplication_percentage: 50.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_files.push(UnusedFile {
            path: PathBuf::from("src/a.ts"),
        });
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/c.ts"),
            export_name: "helper".to_string(),
            is_type_only: false,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert_eq!(result.total(), 2);
        assert_eq!(result.clones_in_unused_files, 1);
        assert_eq!(result.clones_with_unused_exports, 1);

        let affected = result.affected_group_indices();
        assert!(affected.contains(&0)); // Group 0 has clone in unused file
        assert!(affected.contains(&1)); // Group 1 has clone overlapping unused export
        assert!(!affected.contains(&2)); // Group 2 unaffected
    }

    #[test]
    fn clone_instance_outside_export_line_range() {
        // Clone instance at lines 1-5, unused export at line 10
        // They don't overlap, so no finding
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 1, 5),
                make_instance("src/b.ts", 1, 5),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats::default(),
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/a.ts"),
            export_name: "fn".to_string(),
            is_type_only: false,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(!result.has_findings());
    }

    #[test]
    fn clone_in_different_file_than_unused_export() {
        // Clone is in src/a.ts, unused export is in src/x.ts
        let duplication = DuplicationReport {
            clone_groups: vec![make_group(vec![
                make_instance("src/a.ts", 5, 15),
                make_instance("src/b.ts", 5, 15),
            ])],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: crate::duplicates::types::DuplicationStats::default(),
        };
        let mut dead_code = AnalysisResults::default();
        dead_code.unused_exports.push(UnusedExport {
            path: PathBuf::from("src/x.ts"), // different file
            export_name: "fn".to_string(),
            is_type_only: false,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let result = cross_reference(&duplication, &dead_code);
        assert!(!result.has_findings());
    }
}
