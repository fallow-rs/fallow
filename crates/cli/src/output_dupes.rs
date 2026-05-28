//! Typed envelope wrappers for the duplication findings emitted by `fallow
//! dupes --format json` (and the `dupes` block inside `fallow` and `fallow
//! audit`).
//!
//! Each wrapper flattens the bare finding via `#[serde(flatten)]` so the wire
//! shape matches the previous `actions`-grafted output byte-for-byte.
//! `actions` is populated at construction time via each wrapper's
//! `with_actions` constructor and replaces the legacy `inject_dupes_actions`
//! post-pass in `crates/cli/src/report/json.rs`. `introduced` on
//! `CloneGroupFinding` carries the optional audit breadcrumb that
//! `crates/cli/src/audit.rs::annotate_dupes_json` inserts into the JSON object
//! via `map.insert`; the wrapper-level field stays `None` when serialized
//! directly from Rust and is set by the audit pass only when the clone group
//! was introduced relative to the merge-base.
//!
//! Lives in `fallow-cli` rather than `fallow-types` because `CloneFamily`,
//! `CloneGroup`, and `MirroredDirectory` are defined in `fallow-core`
//! (`crates/core/src/duplicates/types.rs`) and `AttributedCloneGroup` is
//! defined in the CLI itself (`crates/cli/src/report/dupes_grouping.rs`);
//! `fallow-types` is the lower-level crate that neither of those reach.

use std::path::PathBuf;

use fallow_core::duplicates::{
    CloneFamily, CloneFingerprintSet, CloneGroup, DuplicationReport, DuplicationStats,
    MirroredDirectory, RefactoringSuggestion,
};
use fallow_types::envelope::AuditIntroduced;
use fallow_types::serde_path;
use serde::Serialize;

use crate::report::dupes_grouping::AttributedCloneGroup;

/// Per-action wire shape attached to each [`CloneGroupFinding`] and
/// [`AttributedCloneGroupFinding`]. Mirrors the action types previously
/// emitted by `inject_dupes_actions::build_clone_group_actions` in
/// `crates/cli/src/report/json.rs`: `extract-shared` plus `suppress-line`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroupAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneGroupActionType,
    /// Whether `fallow fix` can auto-apply this action. Both variants are
    /// manual today; the field is non-singleton so a future auto-applier
    /// does not need a schema change.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// The inline comment to insert (e.g.,
    /// `// fallow-ignore-next-line code-duplication`). Present on
    /// `suppress-line`; absent on `extract-shared`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`CloneGroupAction::kind`]. Mirrors the action types
/// emitted by the legacy `build_clone_group_actions` walker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CloneGroupActionType {
    /// Extract the duplicated code into a shared function.
    ExtractShared,
    /// Suppress the finding with an inline `// fallow-ignore-next-line
    /// code-duplication` comment above the duplicated code.
    SuppressLine,
}

/// Per-action wire shape attached to each [`CloneFamilyFinding`]. Mirrors
/// the action types previously emitted by
/// `build_clone_family_actions`: `extract-shared`, one `apply-suggestion`
/// per [`RefactoringSuggestion`] on the family, and a trailing
/// `suppress-line`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: CloneFamilyActionType,
    /// Whether `fallow fix` can auto-apply this action. All three variants
    /// are manual today.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context. Present on `extract-shared` (explaining that
    /// the family's clone groups share the same files); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The inline comment to insert (e.g.,
    /// `// fallow-ignore-next-line code-duplication`). Present on
    /// `suppress-line` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`CloneFamilyAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CloneFamilyActionType {
    /// Extract the duplicated code blocks into a shared module.
    ExtractShared,
    /// Apply one of the family's [`RefactoringSuggestion`]s. One action
    /// per suggestion entry on the bare family.
    ApplySuggestion,
    /// Suppress with an inline `// fallow-ignore-next-line code-duplication`
    /// comment above the duplicated code.
    SuppressLine,
}

const SUPPRESS_COMMENT: &str = "// fallow-ignore-next-line code-duplication";
const SUPPRESS_DESCRIPTION: &str = "Suppress with an inline comment above the duplicated code";

/// Wire-shape envelope for a [`CloneGroup`] finding. Flattens the bare
/// group via `#[serde(flatten)]` and carries a typed `actions` array plus
/// the optional audit-mode `introduced` flag. Replaces the legacy
/// post-pass injection in `crates/cli/src/report/json.rs::inject_dupes_actions`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneGroupFinding {
    /// The underlying clone group.
    #[serde(flatten)]
    pub group: CloneGroup,
    /// Stable content fingerprint, usually `dup:<8hex>` and widened on rare
    /// report collisions. Addressable via `fallow dupes --trace dup:<fp>` (and
    /// the `trace_clone` MCP tool) to deep-dive this group; shown alongside
    /// each group in the human listing.
    pub fingerprint: String,
    /// Suggested next steps: an `extract-shared` primary and a
    /// `suppress-line` secondary. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<CloneGroupAction>,
    /// Set by the audit pass when this clone group is introduced relative
    /// to the merge-base. `None` when serialized directly from Rust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl CloneGroupFinding {
    /// Build the wrapper from a raw [`CloneGroup`], computing the typed
    /// `actions` array inline. `introduced` stays `None` and is set later
    /// by `annotate_dupes_json` if the audit pass runs.
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(group: CloneGroup) -> Self {
        let fingerprint = fallow_core::duplicates::clone_fingerprint(&group.instances);
        Self::with_fingerprint(group, fingerprint)
    }

    /// Build the wrapper with a precomputed report-scoped fingerprint.
    #[must_use]
    pub fn with_fingerprint(group: CloneGroup, fingerprint: String) -> Self {
        let line_count = group.line_count;
        let instance_count = group.instances.len();
        let actions = vec![
            CloneGroupAction {
                kind: CloneGroupActionType::ExtractShared,
                auto_fixable: false,
                description: format!(
                    "Extract duplicated code ({line_count} lines, {instance_count} instance{}) into a shared function",
                    if instance_count == 1 { "" } else { "s" },
                ),
                comment: None,
            },
            CloneGroupAction {
                kind: CloneGroupActionType::SuppressLine,
                auto_fixable: false,
                description: SUPPRESS_DESCRIPTION.to_string(),
                comment: Some(SUPPRESS_COMMENT.to_string()),
            },
        ];
        Self {
            fingerprint,
            group,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`CloneFamily`] finding.
///
/// Unlike most `*Finding` wrappers this one is NOT `#[serde(flatten)]` over
/// the bare [`CloneFamily`], because the family's nested
/// `groups: Vec<CloneGroup>` field needs to carry the typed
/// [`CloneGroupFinding`] wrapper too (so every nested clone group gets its
/// own `actions[]` array, matching the legacy post-pass behavior; see issue
/// #393 regression test). The wire shape stays byte-identical to the
/// previous post-pass output. No `introduced` field because `fallow audit`
/// attributes clone groups (not families) when running against a base ref.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyFinding {
    /// The files involved in this family (sorted for stable output).
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Clone groups belonging to this family, each wrapped with typed
    /// `actions[]` so consumers that read `clone_families[].groups[]`
    /// directly see the same shape as the top-level `clone_groups[]`.
    pub groups: Vec<CloneGroupFinding>,
    /// Total number of duplicated lines across all groups.
    pub total_duplicated_lines: usize,
    /// Total number of duplicated tokens across all groups.
    pub total_duplicated_tokens: usize,
    /// Refactoring suggestions for this family.
    pub suggestions: Vec<RefactoringSuggestion>,
    /// Suggested next steps: an `extract-shared` primary, one
    /// `apply-suggestion` per [`RefactoringSuggestion`] on the family, and
    /// a trailing `suppress-line`. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<CloneFamilyAction>,
}

impl CloneFamilyFinding {
    /// Build the wrapper from a raw [`CloneFamily`], computing the typed
    /// `actions` array inline and wrapping each inner clone group with its
    /// own typed actions.
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
    let group_count = groups.len();
    let mut actions = Vec::with_capacity(2 + suggestions.len());
    actions.push(CloneFamilyAction {
        kind: CloneFamilyActionType::ExtractShared,
        auto_fixable: false,
        description: format!(
            "Extract {group_count} duplicated code block{} ({total_duplicated_lines} lines) into a shared module",
            if group_count == 1 { "" } else { "s" },
        ),
        note: Some(
            "These clone groups share the same files, indicating a structural relationship; refactor together"
                .to_string(),
        ),
        comment: None,
    });
    for suggestion in suggestions {
        actions.push(CloneFamilyAction {
            kind: CloneFamilyActionType::ApplySuggestion,
            auto_fixable: false,
            description: suggestion.description.clone(),
            note: None,
            comment: None,
        });
    }
    actions.push(CloneFamilyAction {
        kind: CloneFamilyActionType::SuppressLine,
        auto_fixable: false,
        description: SUPPRESS_DESCRIPTION.to_string(),
        note: None,
        comment: Some(SUPPRESS_COMMENT.to_string()),
    });
    actions
}

/// Wire-shape envelope for an [`AttributedCloneGroup`] finding (per-bucket
/// duplication attribution emitted under `fallow dupes --group-by`).
/// Flattens the attributed group and carries the same typed
/// `CloneGroupAction` array as [`CloneGroupFinding`]; no `introduced`
/// field because `fallow audit` does not run on grouped output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedCloneGroupFinding {
    /// The underlying attributed clone group.
    #[serde(flatten)]
    pub group: AttributedCloneGroup,
    /// Stable content fingerprint, usually `dup:<8hex>` and widened on rare
    /// report collisions. Addressable via `fallow dupes --trace dup:<fp>`.
    /// Computed from the group's instances, so it matches the top-level
    /// `clone_groups[].fingerprint` for the same clone.
    pub fingerprint: String,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<CloneGroupAction>,
}

impl AttributedCloneGroupFinding {
    /// Build the wrapper from an [`AttributedCloneGroup`], computing the
    /// typed `actions` array inline from the attributed group's
    /// `line_count` and instance count.
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(group: AttributedCloneGroup) -> Self {
        let fingerprint = group.instances.first().map_or_else(
            || fallow_core::duplicates::fingerprint_for_fragment(""),
            |ai| fallow_core::duplicates::fingerprint_for_fragment(&ai.instance.fragment),
        );
        Self::with_fingerprint(group, fingerprint)
    }

    /// Build the wrapper with a precomputed report-scoped fingerprint.
    #[must_use]
    pub fn with_fingerprint(group: AttributedCloneGroup, fingerprint: String) -> Self {
        let line_count = group.line_count;
        let instance_count = group.instances.len();
        let actions = vec![
            CloneGroupAction {
                kind: CloneGroupActionType::ExtractShared,
                auto_fixable: false,
                description: format!(
                    "Extract duplicated code ({line_count} lines, {instance_count} instance{}) into a shared function",
                    if instance_count == 1 { "" } else { "s" },
                ),
                comment: None,
            },
            CloneGroupAction {
                kind: CloneGroupActionType::SuppressLine,
                auto_fixable: false,
                description: SUPPRESS_DESCRIPTION.to_string(),
                comment: Some(SUPPRESS_COMMENT.to_string()),
            },
        ];
        Self {
            group,
            fingerprint,
            actions,
        }
    }
}

/// Wire-shape payload for `fallow dupes --format json` (the body that
/// flattens into [`crate::output_envelope::DupesOutput`] and is also
/// emitted under the `dupes` / `duplication` key inside the combined and
/// audit envelopes).
///
/// Mirrors [`DuplicationReport`] field-for-field, except `clone_groups`
/// and `clone_families` carry the typed wrapper envelopes instead of bare
/// findings, so the schema (and any TS / agent consumer) sees the typed
/// `actions[]` natively.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DupesReportPayload {
    /// All detected clone groups, each wrapped with typed actions.
    pub clone_groups: Vec<CloneGroupFinding>,
    /// Clone families, each wrapped with typed actions. Inner `groups`
    /// inside each [`CloneFamilyFinding`] are themselves wrapped as
    /// [`CloneGroupFinding`] entries carrying their own `actions[]` (and
    /// optional audit-mode `introduced` flag), so JSON-Schema strict
    /// consumers and TS consumers reading `clone_families[].groups[]` see
    /// the same shape as the top-level `clone_groups[]` array (preserves
    /// the issue #393 regression contract).
    pub clone_families: Vec<CloneFamilyFinding>,
    /// Mirrored directory pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrored_directories: Vec<MirroredDirectory>,
    /// Aggregate duplication statistics.
    pub stats: DuplicationStats,
}

impl DupesReportPayload {
    /// Build the payload from a bare [`DuplicationReport`]. Wraps each
    /// clone group and family with its typed actions; clones the
    /// `mirrored_directories` and `stats` through unchanged.
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
    use std::path::PathBuf;

    use fallow_core::duplicates::{
        CloneInstance, DuplicationStats, RefactoringKind, RefactoringSuggestion,
    };

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
        assert_eq!(
            finding.actions[0].kind,
            CloneGroupActionType::ExtractShared,
            "position 0 of a clone group must be `extract-shared` (jq scripts read .actions[0].type)",
        );
        assert_eq!(finding.actions[1].kind, CloneGroupActionType::SuppressLine);
        assert!(finding.introduced.is_none());
    }

    #[test]
    fn clone_group_finding_description_pluralises_instance_count() {
        let single = CloneGroupFinding::with_actions(group(1));
        assert!(
            single.actions[0].description.contains("1 instance"),
            "single instance should be singular: {}",
            single.actions[0].description
        );
        assert!(
            !single.actions[0].description.contains("1 instances"),
            "single instance must not pluralise: {}",
            single.actions[0].description
        );
        let multi = CloneGroupFinding::with_actions(group(3));
        assert!(
            multi.actions[0].description.contains("3 instances"),
            "multiple instances must pluralise: {}",
            multi.actions[0].description
        );
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
        // 1 extract-shared + 2 apply-suggestion + 1 suppress-line = 4
        assert_eq!(finding.actions.len(), 4);
        assert_eq!(
            finding.actions[0].kind,
            CloneFamilyActionType::ExtractShared,
            "position 0 of a clone family must be `extract-shared`",
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
        // Issue #393 regression: every nested clone group inside a family
        // must also carry its own typed actions array.
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
        // Sanity check: every group has the canonical 2-action array.
        for finding in &payload.clone_groups {
            assert_eq!(finding.actions.len(), 2);
        }
        // Sanity check: family with zero suggestions has 2 actions.
        assert_eq!(payload.clone_families[0].actions.len(), 2);
    }

    #[test]
    fn attributed_clone_group_finding_actions_match_clone_group_shape() {
        use crate::report::dupes_grouping::AttributedInstance;
        let attributed = AttributedCloneGroup {
            primary_owner: "src".to_string(),
            token_count: 100,
            line_count: 20,
            instances: vec![
                AttributedInstance {
                    instance: instance("/root/src/a.ts"),
                    owner: "src".to_string(),
                },
                AttributedInstance {
                    instance: instance("/root/src/b.ts"),
                    owner: "src".to_string(),
                },
            ],
        };
        let finding = AttributedCloneGroupFinding::with_actions(attributed);
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(finding.actions[0].kind, CloneGroupActionType::ExtractShared);
        assert_eq!(finding.actions[1].kind, CloneGroupActionType::SuppressLine);
    }
}
