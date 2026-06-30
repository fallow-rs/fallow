//! Cross-reference helpers exposed through the engine boundary.

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

impl From<fallow_core::cross_reference::CombinedFinding> for CombinedFinding {
    fn from(finding: fallow_core::cross_reference::CombinedFinding) -> Self {
        Self {
            clone_instance: finding.clone_instance,
            dead_code_kind: finding.dead_code_kind.into(),
            group_index: finding.group_index,
        }
    }
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

impl From<fallow_core::cross_reference::DeadCodeKind> for DeadCodeKind {
    fn from(kind: fallow_core::cross_reference::DeadCodeKind) -> Self {
        match kind {
            fallow_core::cross_reference::DeadCodeKind::UnusedFile => Self::UnusedFile,
            fallow_core::cross_reference::DeadCodeKind::UnusedExport { export_name } => {
                Self::UnusedExport { export_name }
            }
            fallow_core::cross_reference::DeadCodeKind::UnusedType { type_name } => {
                Self::UnusedType { type_name }
            }
        }
    }
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

impl From<fallow_core::cross_reference::CrossReferenceResult> for CrossReferenceResult {
    fn from(result: fallow_core::cross_reference::CrossReferenceResult) -> Self {
        Self {
            combined_findings: result
                .combined_findings
                .into_iter()
                .map(CombinedFinding::from)
                .collect(),
            clones_in_unused_files: result.clones_in_unused_files,
            clones_with_unused_exports: result.clones_with_unused_exports,
        }
    }
}

/// Cross-reference duplication findings with dead-code analysis results.
#[must_use]
pub fn cross_reference(
    duplication: &DuplicationReport,
    dead_code: &AnalysisResults,
) -> CrossReferenceResult {
    fallow_core::cross_reference::cross_reference(duplication, dead_code).into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

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
    fn cross_reference_result_converts_from_core_without_leaking_type() {
        let result =
            CrossReferenceResult::from(fallow_core::cross_reference::CrossReferenceResult {
                combined_findings: vec![fallow_core::cross_reference::CombinedFinding {
                    clone_instance: clone_instance("src/a.ts", 1, 3),
                    dead_code_kind: fallow_core::cross_reference::DeadCodeKind::UnusedType {
                        type_name: "UnusedType".to_string(),
                    },
                    group_index: 7,
                }],
                clones_in_unused_files: 0,
                clones_with_unused_exports: 1,
            });

        assert_eq!(result.total(), 1);
        assert_eq!(result.clones_with_unused_exports, 1);
        assert!(matches!(
            result.combined_findings[0].dead_code_kind,
            DeadCodeKind::UnusedType { ref type_name } if type_name == "UnusedType"
        ));
    }
}
