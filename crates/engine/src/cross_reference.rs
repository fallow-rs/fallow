//! Cross-reference helpers exposed through the engine boundary.

use std::path::PathBuf;

use rustc_hash::FxHashSet;
use serde::Serialize;

use crate::duplicates::{CloneInstance, DuplicationReport};
use crate::results::AnalysisResults;

/// A combined finding where a clone instance overlaps with a dead-code issue.
#[derive(Debug, Clone, Serialize)]
pub struct CombinedFinding {
    /// The clone instance that is also unused.
    pub clone_instance: CloneInstance,
    /// What kind of dead code overlaps with this clone.
    pub dead_code_kind: DeadCodeKind,
    /// Clone group index for associating with the parent group.
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

/// Result of cross-referencing duplication with dead-code analysis.
#[derive(Debug, Clone, Serialize)]
pub struct CrossReferenceResult {
    /// Clone instances that are also dead code.
    pub combined_findings: Vec<CombinedFinding>,
    /// Number of clone instances in unused files.
    pub clones_in_unused_files: usize,
    /// Number of clone instances overlapping unused exports.
    pub clones_with_unused_exports: usize,
}

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

    /// Get clone groups that have at least one combined finding.
    #[must_use]
    pub fn affected_group_indices(&self) -> FxHashSet<usize> {
        self.combined_findings
            .iter()
            .map(|finding| finding.group_index)
            .collect()
    }
}

/// Cross-reference duplication findings with dead-code analysis results.
#[must_use]
pub fn cross_reference(
    duplication: &DuplicationReport,
    dead_code: &AnalysisResults,
) -> CrossReferenceResult {
    let unused_files: FxHashSet<&PathBuf> = dead_code
        .unused_files
        .iter()
        .map(|finding| &finding.file.path)
        .collect();

    let mut combined_findings = Vec::new();
    let mut clones_in_unused_files = 0usize;
    let mut clones_with_unused_exports = 0usize;

    for (group_index, group) in duplication.clone_groups.iter().enumerate() {
        for instance in &group.instances {
            if unused_files.contains(&instance.file) {
                combined_findings.push(CombinedFinding {
                    clone_instance: instance.clone(),
                    dead_code_kind: DeadCodeKind::UnusedFile,
                    group_index,
                });
                clones_in_unused_files += 1;
                continue;
            }

            if let Some(finding) = find_overlapping_unused_export(instance, group_index, dead_code)
            {
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

fn find_overlapping_unused_export(
    instance: &CloneInstance,
    group_index: usize,
    dead_code: &AnalysisResults,
) -> Option<CombinedFinding> {
    for export in &dead_code.unused_exports {
        if export.export.path == instance.file
            && (export.export.line as usize) >= instance.start_line
            && (export.export.line as usize) <= instance.end_line
        {
            return Some(CombinedFinding {
                clone_instance: instance.clone(),
                dead_code_kind: DeadCodeKind::UnusedExport {
                    export_name: export.export.export_name.clone(),
                },
                group_index,
            });
        }
    }

    for type_export in &dead_code.unused_types {
        if type_export.export.path == instance.file
            && (type_export.export.line as usize) >= instance.start_line
            && (type_export.export.line as usize) <= instance.end_line
        {
            return Some(CombinedFinding {
                clone_instance: instance.clone(),
                dead_code_kind: DeadCodeKind::UnusedType {
                    type_name: type_export.export.export_name.clone(),
                },
                group_index,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::duplicates::{CloneGroup, DuplicationStats};
    use fallow_types::{
        output_dead_code::{UnusedExportFinding, UnusedFileFinding, UnusedTypeFinding},
        results::{UnusedExport, UnusedFile},
    };

    fn clone_instance(file: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn duplicate_report(instances: Vec<CloneInstance>) -> DuplicationReport {
        DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances,
                token_count: 50,
                line_count: 10,
            }],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        }
    }

    #[test]
    fn cross_reference_result_methods_use_engine_owned_findings() {
        let result = CrossReferenceResult {
            combined_findings: vec![
                CombinedFinding {
                    clone_instance: clone_instance("src/a.ts", 1, 3),
                    dead_code_kind: DeadCodeKind::UnusedFile,
                    group_index: 2,
                },
                CombinedFinding {
                    clone_instance: clone_instance("src/b.ts", 4, 8),
                    dead_code_kind: DeadCodeKind::UnusedExport {
                        export_name: "unused".to_string(),
                    },
                    group_index: 4,
                },
            ],
            clones_in_unused_files: 1,
            clones_with_unused_exports: 1,
        };

        assert_eq!(result.total(), 2);
        assert!(result.has_findings());
        assert!(result.affected_group_indices().contains(&2));
        assert!(result.affected_group_indices().contains(&4));
    }

    #[test]
    fn cross_reference_prioritizes_unused_file_overlap() {
        let duplication = duplicate_report(vec![
            clone_instance("src/a.ts", 1, 3),
            clone_instance("src/b.ts", 4, 8),
        ]);
        let mut dead_code = AnalysisResults::default();
        dead_code
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src/a.ts"),
            }));

        let result = cross_reference(&duplication, &dead_code);

        assert_eq!(result.clones_in_unused_files, 1);
        assert_eq!(result.clones_with_unused_exports, 0);
        assert!(matches!(
            result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedFile
        ));
    }

    #[test]
    fn cross_reference_detects_unused_export_and_type_overlap() {
        let duplication = duplicate_report(vec![
            clone_instance("src/a.ts", 10, 20),
            clone_instance("src/b.ts", 30, 40),
        ]);
        let mut dead_code = AnalysisResults::default();
        dead_code
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/a.ts"),
                export_name: "deadValue".to_string(),
                line: 12,
                col: 0,
                span_start: 0,
                is_re_export: false,
                is_type_only: false,
            }));
        dead_code
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/b.ts"),
                export_name: "DeadType".to_string(),
                line: 35,
                col: 0,
                span_start: 0,
                is_re_export: false,
                is_type_only: true,
            }));

        let result = cross_reference(&duplication, &dead_code);

        assert_eq!(result.clones_in_unused_files, 0);
        assert_eq!(result.clones_with_unused_exports, 2);
        assert!(matches!(
            result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedExport { ref export_name } if export_name == "deadValue"
        ));
        assert!(matches!(
            result.combined_findings[1].dead_code_kind,
            DeadCodeKind::UnusedType { ref type_name } if type_name == "DeadType"
        ));
    }
}
