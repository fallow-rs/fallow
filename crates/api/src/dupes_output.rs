//! Shared duplication JSON payload contracts for programmatic consumers.

use std::path::PathBuf;

use fallow_engine::duplicates::{
    CloneFamily, CloneFingerprintSet, CloneGroup, DuplicationReport, DuplicationStats,
    MirroredDirectory, RefactoringSuggestion, clone_fingerprint, dominant_identifier,
};
use fallow_output::{
    CloneFamilyAction, CloneGroupAction, clone_family_actions, clone_group_actions,
};
use fallow_types::envelope::AuditIntroduced;
use fallow_types::serde_path;
use serde::Serialize;

/// Wire-shape envelope for a clone group finding.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroupFinding {
    /// The underlying clone group.
    #[serde(flatten)]
    pub group: CloneGroup,
    /// Stable content fingerprint, usually `dup:<8hex>`.
    pub fingerprint: String,
    /// Best-effort human-readable name for the clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_name: Option<String>,
    /// Suggested next steps.
    pub actions: Vec<CloneGroupAction>,
    /// Audit-mode introduced flag, populated by audit post-processing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl CloneGroupFinding {
    /// Build the wrapper from a raw [`CloneGroup`].
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(group: CloneGroup) -> Self {
        let fingerprint = clone_fingerprint(&group.instances);
        Self::with_fingerprint(group, fingerprint)
    }

    /// Build the wrapper with a precomputed report-scoped fingerprint.
    #[must_use]
    pub fn with_fingerprint(group: CloneGroup, fingerprint: String) -> Self {
        let suggested_name = dominant_identifier(&group);
        let actions = clone_group_actions(group.line_count, group.instances.len());
        Self {
            fingerprint,
            suggested_name,
            group,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a clone family finding.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyFinding {
    /// The files involved in this family.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Clone groups belonging to this family.
    pub groups: Vec<CloneGroupFinding>,
    /// Total number of duplicated lines across all groups.
    pub total_duplicated_lines: usize,
    /// Total number of duplicated tokens across all groups.
    pub total_duplicated_tokens: usize,
    /// Refactoring suggestions for this family.
    pub suggestions: Vec<RefactoringSuggestion>,
    /// Suggested next steps.
    pub actions: Vec<CloneFamilyAction>,
}

impl CloneFamilyFinding {
    /// Build the wrapper from a raw [`CloneFamily`].
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(family: CloneFamily) -> Self {
        let fingerprints = CloneFingerprintSet::from_groups(&family.groups);
        Self::with_fingerprints(family, &fingerprints)
    }

    /// Build the wrapper using the report-scoped fingerprint assignment shared
    /// by all duplication output surfaces.
    #[must_use]
    pub fn with_fingerprints(family: CloneFamily, fingerprints: &CloneFingerprintSet) -> Self {
        let actions = build_clone_family_actions(
            &family.groups,
            family.total_duplicated_lines,
            &family.suggestions,
        );
        Self {
            files: family.files,
            groups: family
                .groups
                .into_iter()
                .map(|group| {
                    let fingerprint = fingerprints.fingerprint_for_group(&group);
                    CloneGroupFinding::with_fingerprint(group, fingerprint)
                })
                .collect(),
            total_duplicated_lines: family.total_duplicated_lines,
            total_duplicated_tokens: family.total_duplicated_tokens,
            suggestions: family.suggestions,
            actions,
        }
    }
}

fn build_clone_family_actions(
    groups: &[CloneGroup],
    total_duplicated_lines: usize,
    suggestions: &[RefactoringSuggestion],
) -> Vec<CloneFamilyAction> {
    clone_family_actions(
        groups.len(),
        total_duplicated_lines,
        suggestions
            .iter()
            .map(|suggestion| suggestion.description.as_str()),
    )
}

/// Wire-shape payload for `fallow dupes --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DupesReportPayload {
    /// All detected clone groups, each wrapped with typed actions.
    pub clone_groups: Vec<CloneGroupFinding>,
    /// Clone families, each wrapped with typed actions.
    pub clone_families: Vec<CloneFamilyFinding>,
    /// Mirrored directory pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrored_directories: Vec<MirroredDirectory>,
    /// Aggregate duplication statistics.
    pub stats: DuplicationStats,
}

impl DupesReportPayload {
    /// Build the payload from a bare [`DuplicationReport`].
    #[must_use]
    pub fn from_report(report: &DuplicationReport) -> Self {
        let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
        Self {
            clone_groups: report
                .clone_groups
                .iter()
                .map(|group| {
                    CloneGroupFinding::with_fingerprint(
                        group.clone(),
                        fingerprints.fingerprint_for_group(group),
                    )
                })
                .collect(),
            clone_families: report
                .clone_families
                .iter()
                .map(|family| CloneFamilyFinding::with_fingerprints(family.clone(), &fingerprints))
                .collect(),
            mirrored_directories: report.mirrored_directories.clone(),
            stats: report.stats.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use fallow_engine::duplicates::{
        CloneInstance, DuplicationStats, RefactoringKind, RefactoringSuggestion,
    };
    use fallow_output::{CloneFamilyActionType, CloneGroupActionType};

    use super::*;

    fn instance(path: &str) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(instances: usize) -> CloneGroup {
        CloneGroup {
            instances: (0..instances)
                .map(|i| instance(&format!("/root/file_{i}.ts")))
                .collect(),
            token_count: 100,
            line_count: 20,
        }
    }

    #[test]
    fn clone_group_finding_position_0_is_extract_shared() {
        let finding = CloneGroupFinding::with_actions(group(2));
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(finding.actions[0].kind, CloneGroupActionType::ExtractShared);
        assert_eq!(finding.actions[1].kind, CloneGroupActionType::SuppressLine);
        assert!(finding.introduced.is_none());
    }

    #[test]
    fn clone_group_finding_surfaces_dominant_identifier() {
        let fragment = "function parseCsv() { parseCsv(); parseCsv(); return parseCsv; }";
        let g = CloneGroup {
            instances: vec![
                CloneInstance {
                    file: PathBuf::from("/root/a.ts"),
                    start_line: 1,
                    end_line: 3,
                    start_col: 0,
                    end_col: 0,
                    fragment: fragment.to_string(),
                },
                CloneInstance {
                    file: PathBuf::from("/root/b.ts"),
                    start_line: 1,
                    end_line: 3,
                    start_col: 0,
                    end_col: 0,
                    fragment: fragment.to_string(),
                },
            ],
            token_count: 100,
            line_count: 3,
        };
        let finding = CloneGroupFinding::with_actions(g);
        assert_eq!(finding.suggested_name.as_deref(), Some("parseCsv"));
    }

    #[test]
    fn clone_group_finding_suggested_name_none_for_unnamed_fragment() {
        let finding = CloneGroupFinding::with_actions(group(2));
        assert!(finding.suggested_name.is_none());
    }

    #[test]
    fn clone_group_finding_description_pluralises_instance_count() {
        let single = CloneGroupFinding::with_actions(group(1));
        assert!(single.actions[0].description.contains("1 instance"));
        assert!(!single.actions[0].description.contains("1 instances"));
        let multi = CloneGroupFinding::with_actions(group(3));
        assert!(multi.actions[0].description.contains("3 instances"));
    }

    #[test]
    fn clone_family_finding_position_0_is_extract_shared_then_suggestions_then_suppress() {
        let family = CloneFamily {
            files: vec![PathBuf::from("/root/a.ts"), PathBuf::from("/root/b.ts")],
            groups: vec![group(2), group(2)],
            total_duplicated_lines: 40,
            total_duplicated_tokens: 200,
            suggestions: vec![
                RefactoringSuggestion {
                    kind: RefactoringKind::ExtractFunction,
                    description: "Extract helper".to_string(),
                    estimated_savings: 10,
                },
                RefactoringSuggestion {
                    kind: RefactoringKind::ExtractModule,
                    description: "Extract module".to_string(),
                    estimated_savings: 30,
                },
            ],
        };
        let finding = CloneFamilyFinding::with_actions(family);
        assert_eq!(finding.actions.len(), 4);
        assert_eq!(
            finding.actions[0].kind,
            CloneFamilyActionType::ExtractShared
        );
        assert_eq!(
            finding.actions[1].kind,
            CloneFamilyActionType::ApplySuggestion
        );
        assert_eq!(finding.actions[1].description, "Extract helper");
        assert_eq!(
            finding.actions[2].kind,
            CloneFamilyActionType::ApplySuggestion
        );
        assert_eq!(finding.actions[2].description, "Extract module");
        assert_eq!(finding.actions[3].kind, CloneFamilyActionType::SuppressLine);
        assert_eq!(finding.groups.len(), 2);
        for inner in &finding.groups {
            assert_eq!(inner.actions.len(), 2);
            assert_eq!(inner.actions[0].kind, CloneGroupActionType::ExtractShared);
            assert_eq!(inner.actions[1].kind, CloneGroupActionType::SuppressLine);
        }
    }

    #[test]
    fn clone_family_finding_with_no_suggestions_emits_two_actions() {
        let family = CloneFamily {
            files: vec![PathBuf::from("/root/a.ts")],
            groups: vec![group(2)],
            total_duplicated_lines: 20,
            total_duplicated_tokens: 100,
            suggestions: Vec::new(),
        };
        let finding = CloneFamilyFinding::with_actions(family);
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(
            finding.actions[0].kind,
            CloneFamilyActionType::ExtractShared
        );
        assert_eq!(finding.actions[1].kind, CloneFamilyActionType::SuppressLine);
    }

    #[test]
    fn payload_from_report_wraps_all_findings() {
        let report = DuplicationReport {
            clone_groups: vec![group(2), group(3)],
            clone_families: vec![CloneFamily {
                files: vec![PathBuf::from("/root/a.ts")],
                groups: vec![group(2)],
                total_duplicated_lines: 20,
                total_duplicated_tokens: 100,
                suggestions: Vec::new(),
            }],
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        };
        let payload = DupesReportPayload::from_report(&report);
        assert_eq!(payload.clone_groups.len(), 2);
        assert_eq!(payload.clone_families.len(), 1);
        for finding in &payload.clone_groups {
            assert_eq!(finding.actions.len(), 2);
        }
        assert_eq!(payload.clone_families[0].actions.len(), 2);
    }
}
