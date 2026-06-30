//! Shared duplication JSON payload contracts for programmatic consumers.

use std::path::{Path, PathBuf};

use fallow_engine::{
    CloneFingerprintSet, clone_fingerprint, dominant_identifier, fingerprint_for_fragment,
};
use fallow_output::{
    CloneFamilyAction, CloneGroupAction, CodeClimateIssue, CodeClimateIssueInput,
    CodeClimateSeverity, clone_family_actions, clone_group_actions, codeclimate_fingerprint_hash,
    normalize_uri,
};
use fallow_types::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats, MirroredDirectory,
    RefactoringSuggestion,
};
use fallow_types::envelope::AuditIntroduced;
use fallow_types::serde_path;
use serde::Serialize;

/// A clone instance plus its per-instance owner key (for inline JSON / SARIF
/// rendering).
///
/// Each instance carries its own `owner` field alongside the standard
/// `CloneInstance` shape (file / start_line / end_line / start_col / end_col /
/// fragment), so consumers can attribute instances to resolver keys without
/// re-resolving paths.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedInstance {
    /// The original clone instance.
    #[serde(flatten)]
    pub instance: CloneInstance,
    /// Resolver key for this specific instance (per-instance, not the
    /// group-level largest-owner).
    pub owner: String,
}

/// A clone group annotated with largest-owner attribution and per-instance
/// owner keys.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedCloneGroup {
    /// Largest-owner attribution: the resolver key with the most instances in
    /// this clone group. Ties broken alphabetically (smallest key wins).
    pub primary_owner: String,
    /// Number of tokens in the clone group.
    pub token_count: usize,
    /// Number of source lines in the clone group.
    pub line_count: usize,
    /// Each instance carries its own `owner` field alongside the standard
    /// CloneInstance shape.
    pub instances: Vec<AttributedInstance>,
}

impl AttributedCloneGroup {
    /// Return the report-scoped fingerprint for this attributed group.
    #[must_use]
    pub fn fingerprint(&self, fingerprints: &CloneFingerprintSet) -> String {
        let instances: Vec<_> = self
            .instances
            .iter()
            .map(|instance| instance.instance.clone())
            .collect();
        fingerprints.fingerprint_for_parts(&instances, self.token_count, self.line_count)
    }
}

/// Wire-shape envelope for an [`AttributedCloneGroup`] finding (per-bucket
/// duplication attribution emitted under `fallow dupes --group-by`).
/// Flattens the attributed group and carries the same typed
/// `CloneGroupAction` array as `CloneGroupFinding`; no `introduced`
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
    /// Build the wrapper from an [`AttributedCloneGroup`].
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(group: AttributedCloneGroup) -> Self {
        let fingerprint = group.instances.first().map_or_else(
            || fingerprint_for_fragment(""),
            |ai| fingerprint_for_fragment(&ai.instance.fragment),
        );
        Self::with_fingerprint(group, fingerprint)
    }

    /// Build the wrapper with a precomputed report-scoped fingerprint.
    #[must_use]
    pub fn with_fingerprint(group: AttributedCloneGroup, fingerprint: String) -> Self {
        let actions = clone_group_actions(group.line_count, group.instances.len());
        Self {
            group,
            fingerprint,
            actions,
        }
    }
}

/// A single grouped duplication bucket. Per-group `stats` are dedup-aware and
/// computed over the FULL group BEFORE any `--top` truncation.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicationGroup {
    /// Group label (owner / directory / package / section). `(unowned)` for
    /// files with no CODEOWNERS rule, `(no section)` for pre-section rules in
    /// section mode.
    pub key: String,
    /// Dedup-aware aggregate stats for the group.
    pub stats: DuplicationStats,
    /// Clone groups attributed to this owner, each wrapped with the typed
    /// `actions[]` array. Each group's `primary_owner` is its largest-owner
    /// key; per-instance `owner` lets consumers see cross-bucket fan-out
    /// without re-resolving paths.
    pub clone_groups: Vec<AttributedCloneGroupFinding>,
    /// Clone families overlapping this bucket, each wrapped with the typed
    /// `actions[]` array.
    pub clone_families: Vec<CloneFamilyFinding>,
}

/// Wrapper carrying the resolver mode label and grouped buckets.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicationGrouping {
    /// Resolver mode label (`"owner"`, `"directory"`, `"package"`, `"section"`).
    pub mode: &'static str,
    /// One bucket per resolver key.
    pub groups: Vec<DuplicationGroup>,
}

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
    /// Best-effort human-readable name for the clone: the dominant repeated
    /// identifier across the duplicated fragment (e.g. a shared `parseCsv`
    /// function). `None` when the clone has no clear dominant name (generic or
    /// tied identifiers); consumers then fall back to a file-based label. Lets
    /// editors and agents label a clone by what it is rather than an opaque
    /// ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_name: Option<String>,
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

/// Wire-shape envelope for a [`CloneFamily`] finding.
///
/// Unlike most `*Finding` wrappers this one is NOT `#[serde(flatten)]` over
/// the bare [`CloneFamily`], because the family's nested
/// `groups: Vec<CloneGroup>` field needs to carry the typed
/// `CloneGroupFinding` wrapper too (so every nested clone group gets its
/// own `actions[]` array, matching the legacy post-pass behavior; see issue
/// #393 regression test). The wire shape stays byte-identical to the
/// previous post-pass output. No `introduced` field because `fallow audit`
/// attributes clone groups (not families) when running against a base ref.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneFamilyFinding {
    /// The files involved in this family.
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
    /// `apply-suggestion` per `RefactoringSuggestion` on the family, and
    /// a trailing `suppress-line`. Always emitted (possibly empty for
    /// forward-compat).
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

/// Wire-shape payload for `fallow dupes --format json` (the body that
/// flattens into the `DupesOutput` envelope and is also
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
    /// inside each `CloneFamilyFinding` are themselves wrapped as
    /// `CloneGroupFinding` entries carrying their own `actions[]` (and
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

/// Build CodeClimate issues from duplication analysis results.
///
/// `fallow-output` owns the CodeClimate wire DTOs. This API layer combines
/// those DTOs with the engine-owned duplication report so CLI and future
/// embedders can share the same issue construction policy.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "line numbers are bounded by source size"
)]
pub fn build_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    let mut issues = Vec::new();

    for (i, group) in report.clone_groups.iter().enumerate() {
        let token_str = group.token_count.to_string();
        let line_count_str = group.line_count.to_string();
        let fragment_prefix: String = group
            .instances
            .first()
            .map(|inst| inst.fragment.chars().take(64).collect())
            .unwrap_or_default();

        for instance in &group.instances {
            let path = codeclimate_path(&instance.file, root);
            let start_str = instance.start_line.to_string();
            let fp = codeclimate_fingerprint_hash(&[
                "fallow/code-duplication",
                &path,
                &start_str,
                &token_str,
                &line_count_str,
                &fragment_prefix,
            ]);
            issues.push(fallow_output::build_codeclimate_issue(
                CodeClimateIssueInput {
                    check_name: "fallow/code-duplication",
                    description: &format!(
                        "Code clone group {} ({} lines, {} instances)",
                        i + 1,
                        group.line_count,
                        group.instances.len()
                    ),
                    severity: CodeClimateSeverity::Minor,
                    category: "Duplication",
                    path: &path,
                    begin_line: Some(instance.start_line as u32),
                    fingerprint: &fp,
                },
            ));
        }
    }

    issues
}

fn codeclimate_path(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fallow_output::{CloneFamilyActionType, CloneGroupActionType};
    use fallow_types::duplicates::{
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
        assert_eq!(finding.actions[0].kind, CloneGroupActionType::ExtractShared);
        assert_eq!(finding.actions[1].kind, CloneGroupActionType::SuppressLine);
        assert!(finding.introduced.is_none());
    }

    #[test]
    fn attributed_clone_group_finding_actions_match_clone_group_shape() {
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

    #[test]
    fn duplication_codeclimate_uses_relative_normalized_paths() {
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: PathBuf::from("/root/app/[id]/page.tsx"),
                    start_line: 4,
                    end_line: 8,
                    start_col: 0,
                    end_col: 0,
                    fragment: "const duplicate = 1;".to_string(),
                }],
                token_count: 42,
                line_count: 5,
            }],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        };

        let issues = build_duplication_codeclimate(&report, Path::new("/root"));

        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.check_name, "fallow/code-duplication");
        assert_eq!(issue.location.path, "app/%5Bid%5D/page.tsx");
        assert_eq!(issue.location.lines.begin, 4);
        assert_eq!(issue.categories, vec!["Duplication"]);
        assert!(issue.description.contains("Code clone group 1"));
    }
}
