//! Suppression inventory output contracts (`fallow suppressions`).

use std::path::{Path, PathBuf};

use fallow_types::results::{ActiveSuppression, StaleSuppression, SuppressionOrigin};
use fallow_types::serde_path;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};

/// The `fallow suppressions --format json` schema version. Independently
/// versioned from the main contract, mirroring `SecuritySchemaVersion`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SuppressionInventorySchemaVersion {
    /// First release of the `fallow suppressions --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// The `fallow suppressions --format json` envelope. `FallowOutput`
/// discriminates it by the `kind: "suppression-inventory"` tag.
///
/// A read-only projection over the suppression markers present in analyzed
/// files this run: nothing here is a finding, and the command that emits it
/// always exits 0.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow suppressions --format json")
)]
pub struct SuppressionInventoryOutput {
    /// Schema version of this envelope.
    pub schema_version: SuppressionInventorySchemaVersion,
    /// What the run was asked to narrow and whether it did. See
    /// [`crate::RequestOutcomes`] for the full contract.
    ///
    /// `fallow suppressions` accepts `--changed-since`, and an unresolvable ref
    /// widens the inventory to the whole project rather than failing the run.
    /// Until this member existed the only account of that was a stderr line,
    /// which `--quiet` removes, so an inventory read as scoped to the change
    /// could silently be the whole project's (issue #2734).
    ///
    /// The command applies no diff filter, so the object carries the
    /// `changed-since` entry only. Omitted when the run was asked for nothing,
    /// which keeps an inventory that passed no narrowing flag byte-identical and
    /// leaves `schema_version` at `1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_outcomes: Option<crate::RequestOutcomes>,
    /// Project-level totals over the scoped inventory.
    pub summary: SuppressionInventorySummary,
    /// Per-file suppression listings, sorted by path then line.
    pub files: Vec<SuppressionInventoryFile>,
}

/// Project-level totals for the suppression inventory.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressionInventorySummary {
    /// Total suppression markers in scope.
    pub total: usize,
    /// Number of files carrying at least one marker.
    pub files: usize,
    /// Markers without a human-authored `--` reason.
    pub without_reason: usize,
    /// Markers that also appear as stale-suppression findings this run. This
    /// is a JOIN against the existing stale-suppression detector's output
    /// (matched by file and kind), not a new detection.
    pub stale: usize,
    /// Marker counts per suppressed kind, sorted by count (descending) then
    /// kind. `kind` is `null` for blanket markers, mirroring the per-entry
    /// contract.
    pub by_kind: Vec<SuppressionKindCount>,
}

/// One `by_kind` row in the suppression inventory summary.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressionKindCount {
    /// The suppressed issue kind in kebab-case, or `null` for blanket markers
    /// (rendered as "blanket" in human output; machine consumers branch on
    /// `null`).
    pub kind: Option<String>,
    /// Number of markers targeting this kind.
    pub count: usize,
}

/// One file's suppression listing.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressionInventoryFile {
    /// Project-root-relative path, forward-slash separated.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Markers in this file, sorted by line.
    pub suppressions: Vec<SuppressionInventoryEntry>,
}

/// One suppression marker in the inventory.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SuppressionInventoryEntry {
    /// 1-based line of the suppression comment itself; 0 only if unknown.
    pub line: u32,
    /// The suppressed issue kind in kebab-case (e.g. `"unused-export"`), or
    /// `null` for a blanket marker that suppresses every kind on its target.
    /// Human output renders the blanket case as the literal word "blanket";
    /// the JSON contract deliberately keeps `null` so machine consumers
    /// branch on `null` instead of a magic string.
    pub kind: Option<String>,
    /// Whether the marker is file-wide (`fallow-ignore-file`) or line-scoped
    /// (`fallow-ignore-next-line`).
    pub level: SuppressionInventoryLevel,
    /// How the suppression was authored. `"comment"` in v1; `"jsdoc_tag"` is
    /// reserved for a follow-up if `@expected-unused` entries enter the
    /// active inventory.
    pub origin: SuppressionInventoryOrigin,
    /// Human-authored reason after `--`; `null` when absent.
    pub reason: Option<String>,
    /// Whether a human-authored reason is present.
    pub reason_present: bool,
}

/// Suppression marker scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SuppressionInventoryLevel {
    /// A `fallow-ignore-file` marker covering the whole file.
    File,
    /// A `fallow-ignore-next-line` marker covering one line.
    Line,
}

/// How a suppression in the inventory was authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SuppressionInventoryOrigin {
    /// A `// fallow-ignore-next-line` or `// fallow-ignore-file` comment.
    Comment,
}

/// Inputs for building `fallow suppressions --format json`.
#[derive(Clone)]
pub struct SuppressionInventoryOutputInput<'a> {
    /// Scoped active suppressions (absolute paths).
    pub active: &'a [ActiveSuppression],
    /// This run's stale-suppression findings (absolute paths), used only for
    /// the `summary.stale` join.
    pub stale: &'a [StaleSuppression],
    /// Project root used to relativize paths.
    pub root: &'a Path,
    /// What became of the narrowing requests the run received, or `None` when
    /// it received none.
    pub request_outcomes: Option<crate::RequestOutcomes>,
}

/// Build the typed suppression inventory envelope: sort by `(path, line)`,
/// group per file, and compute the summary (including the stale join).
#[must_use]
pub fn build_suppression_inventory_output(
    input: SuppressionInventoryOutputInput<'_>,
) -> SuppressionInventoryOutput {
    let mut sorted: Vec<&ActiveSuppression> = input.active.iter().collect();
    sorted.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.comment_line.cmp(&b.comment_line))
            .then(a.kind.cmp(&b.kind))
    });

    let files = group_by_file(&sorted, input.root);
    let summary = build_summary(&sorted, &files, input.stale);

    SuppressionInventoryOutput {
        schema_version: SuppressionInventorySchemaVersion::V1,
        request_outcomes: input.request_outcomes,
        summary,
        files,
    }
}

/// Group sorted suppressions into per-file listings with root-relative paths.
fn group_by_file(sorted: &[&ActiveSuppression], root: &Path) -> Vec<SuppressionInventoryFile> {
    let mut files: Vec<SuppressionInventoryFile> = Vec::new();
    let mut current: Option<&Path> = None;
    for supp in sorted {
        if current != Some(supp.path.as_path()) {
            files.push(SuppressionInventoryFile {
                path: supp
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&supp.path)
                    .to_path_buf(),
                suppressions: Vec::new(),
            });
            current = Some(supp.path.as_path());
        }
        if let Some(file) = files.last_mut() {
            file.suppressions.push(inventory_entry(supp));
        }
    }
    files
}

fn inventory_entry(supp: &ActiveSuppression) -> SuppressionInventoryEntry {
    SuppressionInventoryEntry {
        line: supp.comment_line,
        kind: supp.kind.clone(),
        level: if supp.is_file_level {
            SuppressionInventoryLevel::File
        } else {
            SuppressionInventoryLevel::Line
        },
        origin: SuppressionInventoryOrigin::Comment,
        reason: supp.reason.clone(),
        reason_present: supp.reason.is_some(),
    }
}

fn build_summary(
    sorted: &[&ActiveSuppression],
    files: &[SuppressionInventoryFile],
    stale: &[StaleSuppression],
) -> SuppressionInventorySummary {
    let without_reason = sorted.iter().filter(|s| s.reason.is_none()).count();

    let mut kind_counts: FxHashMap<Option<&str>, usize> = FxHashMap::default();
    for supp in sorted {
        *kind_counts.entry(supp.kind.as_deref()).or_default() += 1;
    }
    let mut by_kind: Vec<SuppressionKindCount> = kind_counts
        .into_iter()
        .map(|(kind, count)| SuppressionKindCount {
            kind: kind.map(str::to_owned),
            count,
        })
        .collect();
    by_kind.sort_by(|a, b| b.count.cmp(&a.count).then(a.kind.cmp(&b.kind)));

    SuppressionInventorySummary {
        total: sorted.len(),
        files: files.len(),
        without_reason,
        stale: stale_join_count(sorted, stale),
        by_kind,
    }
}

/// Count this run's stale-suppression findings whose `(path, kind)` matches a
/// scoped active marker. A JOIN against the stale detector's existing output,
/// never a new detection: missing-reason findings and JSDoc-tag origins are
/// excluded (the former are counted by `without_reason`, the latter do not
/// flow through the active inventory in v1).
fn stale_join_count(sorted: &[&ActiveSuppression], stale: &[StaleSuppression]) -> usize {
    let active_keys: FxHashSet<(&Path, Option<&str>)> = sorted
        .iter()
        .map(|s| (s.path.as_path(), s.kind.as_deref()))
        .collect();

    stale
        .iter()
        .filter(|entry| !entry.missing_reason)
        .filter(|entry| match &entry.origin {
            SuppressionOrigin::Comment { issue_kind, .. } => {
                active_keys.contains(&(entry.path.as_path(), issue_kind.as_deref()))
            }
            SuppressionOrigin::JsdocTag { .. } => false,
        })
        .count()
}

/// Serialize `fallow suppressions --format json`.
///
/// # Errors
///
/// Returns a serde error when the suppression inventory output cannot be
/// converted to JSON.
pub fn serialize_suppression_inventory_json_output(
    output: SuppressionInventoryOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "suppression-inventory", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::output::IssueAction;

    fn active(
        path: &str,
        line: u32,
        kind: Option<&str>,
        reason: Option<&str>,
        file_level: bool,
    ) -> ActiveSuppression {
        ActiveSuppression {
            path: PathBuf::from(path),
            kind: kind.map(str::to_owned),
            is_file_level: file_level,
            reason: reason.map(str::to_owned),
            comment_line: line,
        }
    }

    fn stale(path: &str, line: u32, kind: Option<&str>, missing_reason: bool) -> StaleSuppression {
        StaleSuppression {
            path: PathBuf::from(path),
            line,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: kind.map(str::to_owned),
                reason: None,
                is_file_level: false,
                kind_known: true,
            },
            missing_reason,
            actions: Vec::<IssueAction>::new(),
        }
    }

    #[test]
    fn inventory_groups_sorts_and_relativizes() {
        let actives = vec![
            active("/repo/src/b.ts", 9, Some("unused-export"), None, false),
            active(
                "/repo/src/a.ts",
                4,
                Some("unused-export"),
                Some("public compatibility export"),
                false,
            ),
            active("/repo/src/a.ts", 2, None, None, true),
        ];
        let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
            active: &actives,
            stale: &[],
            root: Path::new("/repo"),
            request_outcomes: None,
        });

        assert_eq!(output.summary.total, 3);
        assert_eq!(output.summary.files, 2);
        assert_eq!(output.summary.without_reason, 2);
        assert_eq!(output.files.len(), 2);
        let first = &output.files[0];
        assert_eq!(first.path.to_string_lossy().replace('\\', "/"), "src/a.ts");
        assert_eq!(first.suppressions[0].line, 2);
        assert_eq!(first.suppressions[0].kind, None);
        assert_eq!(first.suppressions[0].level, SuppressionInventoryLevel::File);
        assert_eq!(first.suppressions[1].line, 4);
        assert!(first.suppressions[1].reason_present);
    }

    #[test]
    fn by_kind_sorts_by_count_desc_then_kind() {
        let actives = vec![
            active("/repo/a.ts", 1, Some("unused-export"), None, false),
            active("/repo/a.ts", 3, Some("unused-export"), None, false),
            active("/repo/a.ts", 5, Some("complexity"), None, false),
            active("/repo/a.ts", 7, None, None, false),
        ];
        let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
            active: &actives,
            stale: &[],
            root: Path::new("/repo"),
            request_outcomes: None,
        });

        let by_kind = &output.summary.by_kind;
        assert_eq!(by_kind[0].kind.as_deref(), Some("unused-export"));
        assert_eq!(by_kind[0].count, 2);
        // Ties sort blanket (None) before named kinds.
        assert_eq!(by_kind[1].kind, None);
        assert_eq!(by_kind[2].kind.as_deref(), Some("complexity"));
    }

    #[test]
    fn stale_join_counts_matching_path_and_kind_only() {
        let actives = vec![
            active("/repo/a.ts", 1, Some("unused-export"), None, false),
            active("/repo/b.ts", 1, Some("complexity"), None, false),
        ];
        let stales = vec![
            // Joins: path+kind matches an active marker.
            stale("/repo/a.ts", 1, Some("unused-export"), false),
            // Does not join: missing-reason findings are counted by
            // `without_reason`, not `stale`.
            stale("/repo/a.ts", 1, Some("unused-export"), true),
            // Does not join: no active marker on that path+kind.
            stale("/repo/c.ts", 1, Some("unused-export"), false),
        ];
        let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
            active: &actives,
            stale: &stales,
            root: Path::new("/repo"),
            request_outcomes: None,
        });

        assert_eq!(output.summary.stale, 1);
    }

    #[test]
    fn json_output_uses_output_owned_root_contract() {
        let actives = vec![active(
            "/repo/src/api/client.ts",
            4,
            Some("unused-export"),
            Some("public compatibility export"),
            false,
        )];
        let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
            active: &actives,
            stale: &[],
            root: Path::new("/repo"),
            request_outcomes: None,
        });

        let value = serialize_suppression_inventory_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-suppressions"),
        )
        .expect("suppression inventory output should serialize");

        assert_eq!(value["kind"], "suppression-inventory");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["files"][0]["path"], "src/api/client.ts");
        let entry = &value["files"][0]["suppressions"][0];
        assert_eq!(entry["line"], 4);
        assert_eq!(entry["kind"], "unused-export");
        assert_eq!(entry["level"], "line");
        assert_eq!(entry["origin"], "comment");
        assert_eq!(entry["reason"], "public compatibility export");
        assert_eq!(entry["reason_present"], true);
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-suppressions"
        );
    }

    #[test]
    fn blanket_marker_serializes_null_kind() {
        let actives = vec![active("/repo/a.ts", 2, None, None, true)];
        let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
            active: &actives,
            stale: &[],
            root: Path::new("/repo"),
            request_outcomes: None,
        });

        let value =
            serialize_suppression_inventory_json_output(output, RootEnvelopeMode::Tagged, None)
                .expect("suppression inventory output should serialize");

        let entry = &value["files"][0]["suppressions"][0];
        assert!(entry["kind"].is_null(), "blanket kind must stay JSON null");
        assert_eq!(entry["level"], "file");
        assert_eq!(entry["reason_present"], false);
        assert!(entry["reason"].is_null());
        assert!(value["summary"]["by_kind"][0]["kind"].is_null());
    }
}
