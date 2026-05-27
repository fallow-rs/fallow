use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{CheckSummary, ElapsedMs, EntryPoints, SchemaVersion, ToolVersion};

use super::{emit_json, normalize_uri};
use crate::explain;
use crate::output_dupes::DupesReportPayload;
use crate::output_envelope::{
    CheckGroupedEntry, CheckGroupedOutput, CheckOutput, DupesOutput, GroupByMode, HealthOutput,
};
use crate::report::grouping::{OwnershipResolver, ResultGroup};

/// Flip the position-0 `add-to-config` `auto_fixable` flag on every
/// `duplicate_exports` finding when the fix-applier is ready to write into
/// the user's config. Mirrors the per-instance flips for
/// `unused_catalog_entries` (handled inside `with_actions`). No-op when
/// `config_fixable` is `false`, since the typed wrapper already constructs
/// every finding with the conservative `auto_fixable: false` default.
fn apply_config_fixable_to_duplicate_exports(results: &mut AnalysisResults, config_fixable: bool) {
    if !config_fixable {
        return;
    }
    for finding in &mut results.duplicate_exports {
        finding.set_config_fixable(true);
    }
}

pub(super) fn print_json(
    results: &AnalysisResults,
    root: &Path,
    elapsed: Duration,
    explain: bool,
    regression: Option<&crate::regression::RegressionOutcome>,
    baseline_matched: Option<(usize, usize)>,
    config_fixable: bool,
) -> ExitCode {
    match build_json_with_config_fixable(results, root, elapsed, config_fixable) {
        Ok(mut output) => {
            if let Some(outcome) = regression
                && let serde_json::Value::Object(ref mut map) = output
            {
                map.insert("regression".to_string(), outcome.to_json());
            }
            if let Some((entries, matched)) = baseline_matched
                && let serde_json::Value::Object(ref mut map) = output
            {
                map.insert(
                    "baseline".to_string(),
                    serde_json::json!({
                        "entries": entries,
                        "matched": matched,
                    }),
                );
            }
            if explain {
                insert_meta(&mut output, explain::check_meta());
            }
            emit_json(&output, "JSON")
        }
        Err(e) => {
            eprintln!("Error: failed to serialize results: {e}");
            ExitCode::from(2)
        }
    }
}

/// Render grouped analysis results as a single JSON document.
///
/// Produces an envelope with `grouped_by` and `total_issues` at the top level,
/// then a `groups` array where each element contains the group `key`,
/// `total_issues`, and all the normal result fields with paths relativized.
#[must_use]
pub(super) fn print_grouped_json(
    groups: &[ResultGroup],
    original: &AnalysisResults,
    root: &Path,
    elapsed: Duration,
    explain: bool,
    resolver: &OwnershipResolver,
    config_fixable: bool,
) -> ExitCode {
    let entries: Vec<CheckGroupedEntry> = groups
        .iter()
        .map(|group| {
            let mut results = group.results.clone();
            apply_config_fixable_to_duplicate_exports(&mut results, config_fixable);
            CheckGroupedEntry {
                key: group.key.clone(),
                owners: group.owners.clone(),
                total_issues: results.total_issues(),
                results,
            }
        })
        .collect();

    let envelope = CheckGroupedOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        grouped_by: group_by_mode_from_label(resolver.mode_label()),
        total_issues: original.total_issues(),
        groups: entries,
        meta: None,
    };

    let mut output = match serde_json::to_value(&envelope) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Error: failed to serialize grouped results: {e}");
            return ExitCode::from(2);
        }
    };

    let root_prefix = format!("{}/", root.display());
    // Strip per group separately. `duplicate_exports` per-instance
    // `config_fixable` was layered into each group's `AnalysisResults`
    // above, so the typed `actions` array on every finding already carries
    // the correct `auto_fixable` bool. Only the suppression harmonizer
    // still walks the per-group JSON tree to dedupe `// fallow-ignore-next-line`
    // anchors across same-path+line findings.
    if let Some(arr) = output.get_mut("groups").and_then(|v| v.as_array_mut()) {
        for entry in arr {
            strip_root_prefix(entry, &root_prefix);
            harmonize_multi_kind_suppress_line_actions(entry);
        }
    }

    if explain {
        insert_meta(&mut output, explain::check_meta());
    }

    emit_json(&output, "JSON")
}

/// JSON output schema version as an integer (independent of tool version).
///
/// Bump this when the structure of the JSON output changes in a
/// backwards-incompatible way (removing/renaming fields, changing types).
/// Adding new fields is always backwards-compatible and does not require a bump.
#[allow(
    clippy::redundant_pub_crate,
    reason = "used through report module re-export by combined.rs, audit.rs, flags.rs"
)]
pub(crate) const SCHEMA_VERSION: u32 = 6;

/// Build the JSON output value for analysis results.
///
/// Metadata fields (`schema_version`, `version`, `elapsed_ms`, `total_issues`)
/// appear first in the output for readability. Paths are made relative to `root`.
///
/// # Errors
///
/// Returns an error if the results cannot be serialized to JSON.
#[allow(
    dead_code,
    reason = "used by the fallow-cli library target for embedders, but dead in the binary target"
)]
pub fn build_json(
    results: &AnalysisResults,
    root: &Path,
    elapsed: Duration,
) -> Result<serde_json::Value, serde_json::Error> {
    build_json_with_config_fixable(
        results,
        root,
        elapsed,
        crate::fix::is_config_fixable(root, None),
    )
}

/// Build the JSON output value with an explicit config-action fixability signal.
///
/// Use this when the caller already knows how the config was loaded, including
/// explicit `--config` paths that default discovery cannot see.
pub fn build_json_with_config_fixable(
    results: &AnalysisResults,
    root: &Path,
    elapsed: Duration,
    config_fixable: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut owned_results = results.clone();
    apply_config_fixable_to_duplicate_exports(&mut owned_results, config_fixable);
    let envelope = CheckOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        total_issues: owned_results.total_issues(),
        entry_points: owned_results
            .entry_point_summary
            .as_ref()
            .map(|ep| EntryPoints {
                total: ep.total,
                // Replace spaces with underscores so downstream dashboards can
                // drill into individual sources by stable keys (e.g.
                // `package.json`, `next.js`, `config_entry`).
                sources: ep
                    .by_source
                    .iter()
                    .map(|(k, v)| (k.replace(' ', "_"), *v))
                    .collect(),
            }),
        summary: build_check_summary(&owned_results),
        results: owned_results,
        baseline_deltas: None,
        baseline: None,
        regression: None,
        meta: None,
        workspace_diagnostics: crate::runtime_support::workspace_diagnostics_for(root),
    };

    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    // strip_root_prefix runs on the already-serialized envelope where the
    // typed `actions` arrays are already in place; the stripper skips
    // non-path string values via the prefix check.
    strip_root_prefix(&mut output, &root_prefix);
    harmonize_multi_kind_suppress_line_actions(&mut output);
    Ok(output)
}

/// Compute the per-category `CheckSummary` from analysis results.
///
/// The `unused_dependencies` field is the COMBINED count across regular,
/// dev, and optional dependencies (the JSON layer has folded these three
/// since the schema was first published; the per-section arrays still
/// live at the top level of `CheckOutput`).
fn build_check_summary(results: &AnalysisResults) -> CheckSummary {
    CheckSummary {
        total_issues: results.total_issues(),
        unused_files: results.unused_files.len(),
        unused_exports: results.unused_exports.len(),
        unused_types: results.unused_types.len(),
        private_type_leaks: results.private_type_leaks.len(),
        unused_dependencies: results.unused_dependencies.len()
            + results.unused_dev_dependencies.len()
            + results.unused_optional_dependencies.len(),
        unused_enum_members: results.unused_enum_members.len(),
        unused_class_members: results.unused_class_members.len(),
        unresolved_imports: results.unresolved_imports.len(),
        unlisted_dependencies: results.unlisted_dependencies.len(),
        duplicate_exports: results.duplicate_exports.len(),
        type_only_dependencies: results.type_only_dependencies.len(),
        test_only_dependencies: results.test_only_dependencies.len(),
        circular_dependencies: results.circular_dependencies.len(),
        re_export_cycles: results.re_export_cycles.len(),
        boundary_violations: results.boundary_violations.len(),
        stale_suppressions: results.stale_suppressions.len(),
        unused_catalog_entries: results.unused_catalog_entries.len(),
        empty_catalog_groups: results.empty_catalog_groups.len(),
        unresolved_catalog_references: results.unresolved_catalog_references.len(),
        unused_dependency_overrides: results.unused_dependency_overrides.len(),
        misconfigured_dependency_overrides: results.misconfigured_dependency_overrides.len(),
    }
}

/// Recursively strip the root prefix from all string values in the JSON tree.
///
/// This converts absolute paths (e.g., `/home/runner/work/repo/repo/src/utils.ts`)
/// to relative paths (`src/utils.ts`) for all output fields.
pub fn strip_root_prefix(value: &mut serde_json::Value, prefix: &str) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(rest) = s.strip_prefix(prefix) {
                *s = rest.to_string();
            } else {
                let normalized = normalize_uri(s);
                let normalized_prefix = normalize_uri(prefix);
                if let Some(rest) = normalized.strip_prefix(&normalized_prefix) {
                    *s = rest.to_string();
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                strip_root_prefix(item, prefix);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                strip_root_prefix(v, prefix);
            }
        }
        _ => {}
    }
}

type SuppressAnchor = (String, u64);

#[allow(
    clippy::redundant_pub_crate,
    reason = "used through report module re-export by audit.rs"
)]
pub(crate) fn harmonize_multi_kind_suppress_line_actions(output: &mut serde_json::Value) {
    let mut anchors: BTreeMap<SuppressAnchor, Vec<String>> = BTreeMap::new();
    collect_suppress_line_anchors(output, &mut anchors);

    anchors.retain(|_, kinds| {
        sort_suppression_kinds(kinds);
        kinds.dedup();
        kinds.len() > 1
    });
    if anchors.is_empty() {
        return;
    }

    rewrite_suppress_line_actions(output, &anchors);
}

fn collect_suppress_line_anchors(
    value: &serde_json::Value,
    anchors: &mut BTreeMap<SuppressAnchor, Vec<String>>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(anchor) = suppression_anchor(map)
                && let Some(actions) = map.get("actions").and_then(serde_json::Value::as_array)
            {
                for action in actions {
                    if let Some(comment) = suppress_line_comment(action) {
                        for kind in parse_suppress_line_comment(comment) {
                            let kinds = anchors.entry(anchor.clone()).or_default();
                            if !kinds.iter().any(|existing| existing == &kind) {
                                kinds.push(kind);
                            }
                        }
                    }
                }
            }

            for child in map.values() {
                collect_suppress_line_anchors(child, anchors);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_suppress_line_anchors(item, anchors);
            }
        }
        _ => {}
    }
}

fn rewrite_suppress_line_actions(
    value: &mut serde_json::Value,
    anchors: &BTreeMap<SuppressAnchor, Vec<String>>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(anchor) = suppression_anchor(map)
                && let Some(kinds) = anchors.get(&anchor)
            {
                let comment = format!("// fallow-ignore-next-line {}", kinds.join(", "));
                if let Some(actions) = map
                    .get_mut("actions")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    for action in actions {
                        if suppress_line_comment(action).is_some()
                            && let serde_json::Value::Object(action_map) = action
                        {
                            action_map.insert("comment".to_string(), serde_json::json!(comment));
                        }
                    }
                }
            }

            for child in map.values_mut() {
                rewrite_suppress_line_actions(child, anchors);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                rewrite_suppress_line_actions(item, anchors);
            }
        }
        _ => {}
    }
}

fn suppression_anchor(map: &serde_json::Map<String, serde_json::Value>) -> Option<SuppressAnchor> {
    let path = map
        .get("path")
        .or_else(|| map.get("from_path"))
        .and_then(serde_json::Value::as_str)?;
    let line = map.get("line").and_then(serde_json::Value::as_u64)?;
    Some((path.to_string(), line))
}

fn suppress_line_comment(action: &serde_json::Value) -> Option<&str> {
    (action.get("type").and_then(serde_json::Value::as_str) == Some("suppress-line"))
        .then_some(())
        .and_then(|()| action.get("comment").and_then(serde_json::Value::as_str))
}

fn parse_suppress_line_comment(comment: &str) -> Vec<String> {
    comment
        .strip_prefix("// fallow-ignore-next-line ")
        .map(|rest| {
            rest.split(|c: char| c == ',' || c.is_whitespace())
                .filter(|token| !token.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn sort_suppression_kinds(kinds: &mut [String]) {
    kinds.sort_by_key(|kind| suppression_kind_rank(kind));
}

fn suppression_kind_rank(kind: &str) -> usize {
    match kind {
        "unused-file" => 0,
        "unused-export" => 1,
        "unused-type" => 2,
        "private-type-leak" => 3,
        "unused-enum-member" => 4,
        "unused-class-member" => 5,
        "unresolved-import" => 6,
        "unlisted-dependency" => 7,
        "duplicate-export" => 8,
        "circular-dependency" => 9,
        "re-export-cycle" => 10,
        "boundary-violation" => 11,
        "code-duplication" => 12,
        "complexity" => 13,
        _ => usize::MAX,
    }
}

// ── Health action injection ─────────────────────────────────────

/// Build a JSON representation of baseline deltas for the combined JSON envelope.
///
/// Accepts a total delta and an iterator of per-category entries to avoid
/// coupling the report module (compiled in both lib and bin) to the
/// binary-only `baseline` module.
pub fn build_baseline_deltas_json<'a>(
    total_delta: i64,
    per_category: impl Iterator<Item = (&'a str, usize, usize, i64)>,
) -> serde_json::Value {
    let mut per_cat = serde_json::Map::new();
    for (cat, current, baseline, delta) in per_category {
        per_cat.insert(
            cat.to_string(),
            serde_json::json!({
                "current": current,
                "baseline": baseline,
                "delta": delta,
            }),
        );
    }
    serde_json::json!({
        "total_delta": total_delta,
        "per_category": per_cat
    })
}

// Health JSON post-pass action injection has been fully retired.
//
// - Complexity findings carry their `actions` natively via the typed
//   [`HealthFinding`](crate::health_types::HealthFinding) wrapper (PR B2
//   of issue #384).
// - Hotspots and refactoring targets carry their typed `actions` arrays
//   via the [`HotspotFinding`](crate::health_types::HotspotFinding) and
//   [`RefactoringTargetFinding`](crate::health_types::RefactoringTargetFinding)
//   wrappers (PR B3 of issue #384).
// - Coverage gaps flow through
//   [`UntestedFileFinding`](crate::health_types::UntestedFileFinding) /
//   [`UntestedExportFinding`](crate::health_types::UntestedExportFinding).
// - Runtime coverage actions ship from the sidecar via serde directly.
//
// The `actions_meta` breadcrumb is set on the typed
// [`HealthReport`](crate::health_types::HealthReport) at construction
// time and flows through serde natively, so no post-pass walker is
// needed for any health output anymore.

// The dupes `actions` arrays are set on the typed `CloneGroupFinding`,
// `CloneFamilyFinding`, and `AttributedCloneGroupFinding` wrappers in
// `crate::output_dupes` at construction time and flow through serde
// natively, so no post-pass walker is needed for any duplication output
// anymore.

/// Insert a `_meta` key into a JSON object value.
fn insert_meta(output: &mut serde_json::Value, meta: serde_json::Value) {
    if let serde_json::Value::Object(map) = output {
        map.insert("_meta".to_string(), meta);
    }
}

/// Build the JSON envelope + health payload shared by `print_health_json` and
/// the CLI integration test suite. Exposed so snapshot tests can lock the
/// on-the-wire shape without routing through stdout capture.
///
/// # Errors
///
/// Returns an error if the report cannot be serialized to JSON.
pub fn build_health_json(
    report: &crate::health_types::HealthReport,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let envelope = HealthOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        report: report.clone(),
        grouped_by: None,
        groups: None,
        meta: None,
        workspace_diagnostics: crate::runtime_support::workspace_diagnostics_for(root),
    };
    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut output, &root_prefix);
    if explain {
        insert_meta(&mut output, explain::health_meta());
    }
    Ok(output)
}

pub(super) fn print_health_json(
    report: &crate::health_types::HealthReport,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> ExitCode {
    match build_health_json(report, root, elapsed, explain) {
        Ok(output) => emit_json(&output, "JSON"),
        Err(e) => {
            eprintln!("Error: failed to serialize health report: {e}");
            ExitCode::from(2)
        }
    }
}

/// Build a grouped health JSON envelope when `--group-by` is active.
///
/// The envelope keeps the active run's `summary`, `vital_signs`, and
/// `health_score` at the top level (so consumers that ignore grouping still
/// see meaningful aggregates) and adds:
///
/// - `grouped_by`: the resolver mode (`"package"`, `"owner"`, etc.).
/// - `groups`: one entry per resolver bucket. Each entry carries its own
///   `vital_signs`, `health_score`, `findings`, `file_scores` sorted by
///   risk-aware triage concern, `hotspots`, `large_functions`, `targets`, plus
///   `key`, `owners` (section mode), and the per-group `files_analyzed` /
///   `functions_above_threshold` counts.
///
/// Paths inside groups are relativised the same way as the project-level
/// payload.
///
/// # Errors
///
/// Returns an error if either the project report or any group cannot be
/// serialised to JSON.
pub fn build_grouped_health_json(
    report: &crate::health_types::HealthReport,
    grouping: &crate::health_types::HealthGrouping,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let root_prefix = format!("{}/", root.display());
    // Per-group sub-envelopes share the project-level suppression state:
    // baseline-active and config-disabled apply uniformly, so each group's
    // `actions` array honors the same opts AND each group emits its own
    // `actions_meta` breadcrumb. The redundancy with the top-level breadcrumb
    // is intentional: consumers that only walk the `groups` array (e.g.,
    // per-team dashboards) still see the omission reason without needing to
    // walk back up to the report root.
    let envelope = HealthOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        report: report.clone(),
        grouped_by: Some(group_by_mode_from_label(grouping.mode)),
        // `groups` is serialised separately below so each entry can run
        // through the path-stripping + actions injection passes (which only
        // walk the top-level map).
        groups: None,
        meta: None,
        workspace_diagnostics: crate::runtime_support::workspace_diagnostics_for(root),
    };
    let mut output = serde_json::to_value(&envelope)?;
    strip_root_prefix(&mut output, &root_prefix);

    let group_values: Vec<serde_json::Value> = grouping
        .groups
        .iter()
        .map(|g| {
            let mut value = serde_json::to_value(g)?;
            strip_root_prefix(&mut value, &root_prefix);
            Ok(value)
        })
        .collect::<Result<_, serde_json::Error>>()?;

    if let serde_json::Value::Object(ref mut map) = output {
        map.insert("groups".to_string(), serde_json::Value::Array(group_values));
    }

    if explain {
        insert_meta(&mut output, explain::health_meta());
    }

    Ok(output)
}

pub(super) fn print_grouped_health_json(
    report: &crate::health_types::HealthReport,
    grouping: &crate::health_types::HealthGrouping,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> ExitCode {
    match build_grouped_health_json(report, grouping, root, elapsed, explain) {
        Ok(output) => emit_json(&output, "JSON"),
        Err(e) => {
            eprintln!("Error: failed to serialize grouped health report: {e}");
            ExitCode::from(2)
        }
    }
}

/// Build the JSON envelope + duplication payload shared by `print_duplication_json`
/// and the programmatic API surface.
///
/// # Errors
///
/// Returns an error if the report cannot be serialized to JSON.
pub fn build_duplication_json(
    report: &DuplicationReport,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let envelope = DupesOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        report: DupesReportPayload::from_report(report),
        grouped_by: None,
        total_issues: None,
        groups: None,
        meta: None,
        workspace_diagnostics: crate::runtime_support::workspace_diagnostics_for(root),
    };
    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut output, &root_prefix);

    if explain {
        insert_meta(&mut output, explain::dupes_meta());
    }

    Ok(output)
}

pub(super) fn print_duplication_json(
    report: &DuplicationReport,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> ExitCode {
    match build_duplication_json(report, root, elapsed, explain) {
        Ok(output) => emit_json(&output, "JSON"),
        Err(e) => {
            eprintln!("Error: failed to serialize duplication report: {e}");
            ExitCode::from(2)
        }
    }
}

/// Build a grouped duplication JSON envelope when `--group-by` is active.
///
/// The envelope keeps the project-level duplication payload (`stats`,
/// `clone_groups`, `clone_families`) at the top level so consumers that ignore
/// grouping still see project-wide aggregates, and adds:
///
/// - `grouped_by`: the resolver mode (`"owner"`, `"directory"`, `"package"`,
///   `"section"`).
/// - `groups`: one entry per resolver bucket. Each entry carries its own
///   per-group `stats` (dedup-aware, computed over the FULL group before
///   `--top` truncation), `clone_groups` (each tagged with `primary_owner`
///   and per-instance `owner`), and `clone_families`.
///
/// Paths inside groups are relativised the same way as the project-level
/// payload via `strip_root_prefix`.
///
/// # Errors
///
/// Returns an error if either the project report or any group cannot be
/// serialised to JSON.
pub fn build_grouped_duplication_json(
    report: &DuplicationReport,
    grouping: &super::dupes_grouping::DuplicationGrouping,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let root_prefix = format!("{}/", root.display());
    // Mirror the grouped check / health envelopes which expose `total_issues`
    // so MCP and CI consumers can read the same key across commands. For
    // dupes the count is total clone groups (sum is preserved across
    // grouping; each clone group is attributed to exactly one bucket).
    let envelope = DupesOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        report: DupesReportPayload::from_report(report),
        grouped_by: Some(group_by_mode_from_label(grouping.mode)),
        total_issues: Some(report.clone_groups.len()),
        // Per-group buckets are serialized separately below so each carries
        // its own path-stripping; splice them in via the `Value` post-pass.
        // Wrapper-level `actions[]` is already typed on each bucket's
        // `clone_groups` / `clone_families`.
        groups: None,
        meta: None,
        workspace_diagnostics: crate::runtime_support::workspace_diagnostics_for(root),
    };
    let mut output = serde_json::to_value(&envelope)?;
    strip_root_prefix(&mut output, &root_prefix);

    let group_values: Vec<serde_json::Value> = grouping
        .groups
        .iter()
        .map(|g| {
            let mut value = serde_json::to_value(g)?;
            strip_root_prefix(&mut value, &root_prefix);
            Ok(value)
        })
        .collect::<Result<_, serde_json::Error>>()?;

    if let serde_json::Value::Object(ref mut map) = output {
        map.insert("groups".to_string(), serde_json::Value::Array(group_values));
    }

    if explain {
        insert_meta(&mut output, explain::dupes_meta());
    }

    Ok(output)
}

/// Map a free-form grouping mode label (`"package"`, `"owner"`, ...) to the
/// typed [`GroupByMode`] enum. Unknown labels fall through to
/// [`GroupByMode::Owner`] because the upstream grouping pipeline only emits
/// the four documented modes; the fallback exists to keep this helper
/// infallible without piping a Result through every grouped envelope builder.
fn group_by_mode_from_label(label: &str) -> GroupByMode {
    match label {
        "directory" => GroupByMode::Directory,
        "package" => GroupByMode::Package,
        "section" => GroupByMode::Section,
        _ => GroupByMode::Owner,
    }
}

pub(super) fn print_grouped_duplication_json(
    report: &DuplicationReport,
    grouping: &super::dupes_grouping::DuplicationGrouping,
    root: &Path,
    elapsed: Duration,
    explain: bool,
) -> ExitCode {
    match build_grouped_duplication_json(report, grouping, root, elapsed, explain) {
        Ok(output) => emit_json(&output, "JSON"),
        Err(e) => {
            eprintln!("Error: failed to serialize grouped duplication report: {e}");
            ExitCode::from(2)
        }
    }
}

pub(super) fn print_trace_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("Error: failed to serialize trace output: {e}");
            #[expect(
                clippy::exit,
                reason = "fatal serialization error requires immediate exit"
            )]
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_types::{
        RuntimeCoverageAction, RuntimeCoverageConfidence, RuntimeCoverageDataSource,
        RuntimeCoverageEvidence, RuntimeCoverageFinding, RuntimeCoverageHotPath,
        RuntimeCoverageMessage, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
        RuntimeCoverageSchemaVersion, RuntimeCoverageSummary, RuntimeCoverageVerdict,
        RuntimeCoverageWatermark,
    };
    use crate::report::test_helpers::sample_results;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn json_output_has_metadata_fields() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(123);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["schema_version"], 6);
        assert!(output["version"].is_string());
        assert_eq!(output["elapsed_ms"], 123);
        assert_eq!(output["total_issues"], 0);
    }

    #[test]
    fn json_output_includes_issue_arrays() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let elapsed = Duration::from_millis(50);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["unused_files"].as_array().unwrap().len(), 1);
        assert_eq!(output["unused_exports"].as_array().unwrap().len(), 1);
        assert_eq!(output["unused_types"].as_array().unwrap().len(), 1);
        assert_eq!(output["unused_dependencies"].as_array().unwrap().len(), 1);
        assert_eq!(
            output["unused_dev_dependencies"].as_array().unwrap().len(),
            1
        );
        assert_eq!(output["unused_enum_members"].as_array().unwrap().len(), 1);
        assert_eq!(output["unused_class_members"].as_array().unwrap().len(), 1);
        assert_eq!(output["unresolved_imports"].as_array().unwrap().len(), 1);
        assert_eq!(output["unlisted_dependencies"].as_array().unwrap().len(), 1);
        assert_eq!(output["duplicate_exports"].as_array().unwrap().len(), 1);
        assert_eq!(
            output["type_only_dependencies"].as_array().unwrap().len(),
            1
        );
        assert_eq!(output["circular_dependencies"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn health_json_includes_runtime_coverage_with_relative_paths_and_actions() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
                verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
                signals: Vec::new(),
                summary: RuntimeCoverageSummary {
                    data_source: RuntimeCoverageDataSource::Local,
                    last_received_at: None,
                    functions_tracked: 3,
                    functions_hit: 1,
                    functions_unhit: 1,
                    functions_untracked: 1,
                    coverage_percent: 33.3,
                    trace_count: 2_847_291,
                    period_days: 30,
                    deployments_seen: 14,
                    capture_quality: Some(crate::health_types::RuntimeCoverageCaptureQuality {
                        window_seconds: 720,
                        instances_observed: 1,
                        lazy_parse_warning: true,
                        untracked_ratio_percent: 42.5,
                    }),
                },
                findings: vec![RuntimeCoverageFinding {
                    id: "fallow:prod:deadbeef".to_owned(),
                    path: root.join("src/cold.ts"),
                    function: "coldPath".to_owned(),
                    line: 12,
                    verdict: RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: RuntimeCoverageConfidence::Medium,
                    evidence: RuntimeCoverageEvidence {
                        static_status: "used".to_owned(),
                        test_coverage: "not_covered".to_owned(),
                        v8_tracking: "tracked".to_owned(),
                        untracked_reason: None,
                        observation_days: 30,
                        deployments_observed: 14,
                    },
                    actions: vec![RuntimeCoverageAction {
                        kind: "review-deletion".to_owned(),
                        description: "Tracked in runtime coverage with zero invocations."
                            .to_owned(),
                        auto_fixable: false,
                    }],
                }],
                hot_paths: vec![RuntimeCoverageHotPath {
                    id: "fallow:hot:cafebabe".to_owned(),
                    path: root.join("src/hot.ts"),
                    function: "hotPath".to_owned(),
                    line: 3,
                    end_line: 9,
                    invocations: 250,
                    percentile: 99,
                    actions: vec![],
                }],
                blast_radius: vec![],
                importance: vec![],
                watermark: Some(RuntimeCoverageWatermark::LicenseExpiredGrace),
                warnings: vec![RuntimeCoverageMessage {
                    code: "partial-merge".to_owned(),
                    message: "Merged coverage omitted one chunk.".to_owned(),
                }],
            }),
            ..Default::default()
        };

        let envelope = HealthOutput {
            schema_version: SchemaVersion(SCHEMA_VERSION),
            version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: ElapsedMs(7),
            report,
            grouped_by: None,
            groups: None,
            meta: None,
            workspace_diagnostics: Vec::new(),
        };
        let mut output = serde_json::to_value(&envelope).expect("should serialize health envelope");
        strip_root_prefix(&mut output, "/project/");

        assert_eq!(
            output["runtime_coverage"]["verdict"],
            serde_json::Value::String("cold-code-detected".to_owned())
        );
        assert_eq!(
            output["runtime_coverage"]["schema_version"],
            serde_json::Value::String("1".to_owned())
        );
        assert_eq!(
            output["runtime_coverage"]["summary"]["functions_tracked"],
            serde_json::Value::from(3)
        );
        assert_eq!(
            output["runtime_coverage"]["summary"]["coverage_percent"],
            serde_json::Value::from(33.3)
        );
        let finding = &output["runtime_coverage"]["findings"][0];
        assert_eq!(finding["path"], "src/cold.ts");
        assert_eq!(finding["verdict"], "review_required");
        assert_eq!(finding["id"], "fallow:prod:deadbeef");
        assert_eq!(finding["actions"][0]["type"], "review-deletion");
        let hot_path = &output["runtime_coverage"]["hot_paths"][0];
        assert_eq!(hot_path["path"], "src/hot.ts");
        assert_eq!(hot_path["function"], "hotPath");
        assert_eq!(hot_path["percentile"], 99);
        assert_eq!(
            output["runtime_coverage"]["watermark"],
            serde_json::Value::String("license-expired-grace".to_owned())
        );
        assert_eq!(
            output["runtime_coverage"]["warnings"][0]["code"],
            serde_json::Value::String("partial-merge".to_owned())
        );
    }

    #[test]
    fn json_metadata_fields_appear_first() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");
        let keys: Vec<&String> = output.as_object().unwrap().keys().collect();
        assert_eq!(keys[0], "schema_version");
        assert_eq!(keys[1], "version");
        assert_eq!(keys[2], "elapsed_ms");
        assert_eq!(keys[3], "total_issues");
    }

    #[test]
    fn json_total_issues_matches_results() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let total = results.total_issues();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["total_issues"], total);
    }

    #[test]
    fn json_unused_export_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let export = &output["unused_exports"][0];
        assert_eq!(export["export_name"], "helperFn");
        assert_eq!(export["line"], 10);
        assert_eq!(export["col"], 4);
        assert_eq!(export["is_type_only"], false);
        assert_eq!(export["span_start"], 120);
        assert_eq!(export["is_re_export"], false);
    }

    #[test]
    fn json_serializes_to_valid_json() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let elapsed = Duration::from_millis(42);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let json_str = serde_json::to_string_pretty(&output).expect("should stringify");
        let reparsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("JSON output should be valid JSON");
        assert_eq!(reparsed, output);
    }

    // ── Empty results ───────────────────────────────────────────────

    #[test]
    fn json_empty_results_produce_valid_structure() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["total_issues"], 0);
        assert_eq!(output["unused_files"].as_array().unwrap().len(), 0);
        assert_eq!(output["unused_exports"].as_array().unwrap().len(), 0);
        assert_eq!(output["unused_types"].as_array().unwrap().len(), 0);
        assert_eq!(output["unused_dependencies"].as_array().unwrap().len(), 0);
        assert_eq!(
            output["unused_dev_dependencies"].as_array().unwrap().len(),
            0
        );
        assert_eq!(output["unused_enum_members"].as_array().unwrap().len(), 0);
        assert_eq!(output["unused_class_members"].as_array().unwrap().len(), 0);
        assert_eq!(output["unresolved_imports"].as_array().unwrap().len(), 0);
        assert_eq!(output["unlisted_dependencies"].as_array().unwrap().len(), 0);
        assert_eq!(output["duplicate_exports"].as_array().unwrap().len(), 0);
        assert_eq!(
            output["type_only_dependencies"].as_array().unwrap().len(),
            0
        );
        assert_eq!(output["circular_dependencies"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn json_empty_results_round_trips_through_string() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let json_str = serde_json::to_string(&output).expect("should stringify");
        let reparsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("should parse back");
        assert_eq!(reparsed["total_issues"], 0);
    }

    // ── Path stripping ──────────────────────────────────────────────

    #[test]
    fn json_paths_are_relative_to_root() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/deep/nested/file.ts"),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let path = output["unused_files"][0]["path"].as_str().unwrap();
        assert_eq!(path, "src/deep/nested/file.ts");
        assert!(!path.starts_with("/project"));
    }

    #[test]
    fn json_strips_root_from_nested_locations() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_string(),
                    imported_from: vec![ImportSite {
                        path: root.join("src/cli.ts"),
                        line: 2,
                        col: 0,
                    }],
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let site_path = output["unlisted_dependencies"][0]["imported_from"][0]["path"]
            .as_str()
            .unwrap();
        assert_eq!(site_path, "src/cli.ts");
    }

    #[test]
    fn json_strips_root_from_duplicate_export_locations() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/config.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/types.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let loc0 = output["duplicate_exports"][0]["locations"][0]["path"]
            .as_str()
            .unwrap();
        let loc1 = output["duplicate_exports"][0]["locations"][1]["path"]
            .as_str()
            .unwrap();
        assert_eq!(loc0, "src/config.ts");
        assert_eq!(loc1, "src/types.ts");
    }

    #[test]
    fn json_strips_root_from_circular_dependency_files() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    is_cross_package: false,
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let files = output["circular_dependencies"][0]["files"]
            .as_array()
            .unwrap();
        assert_eq!(files[0].as_str().unwrap(), "src/a.ts");
        assert_eq!(files[1].as_str().unwrap(), "src/b.ts");
    }

    #[test]
    fn json_path_outside_root_not_stripped() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/other/project/src/file.ts"),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let path = output["unused_files"][0]["path"].as_str().unwrap();
        assert!(path.contains("/other/project/"));
    }

    // ── Individual issue type field verification ────────────────────

    #[test]
    fn json_unused_file_contains_path() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/orphan.ts"),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let file = &output["unused_files"][0];
        assert_eq!(file["path"], "src/orphan.ts");
    }

    #[test]
    fn json_unused_type_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/types.ts"),
                export_name: "OldInterface".to_string(),
                is_type_only: true,
                line: 20,
                col: 0,
                span_start: 300,
                is_re_export: false,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let typ = &output["unused_types"][0];
        assert_eq!(typ["export_name"], "OldInterface");
        assert_eq!(typ["is_type_only"], true);
        assert_eq!(typ["line"], 20);
        assert_eq!(typ["path"], "src/types.ts");
    }

    #[test]
    fn json_unused_dependency_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "axios".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 10,
                used_in_workspaces: Vec::new(),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["unused_dependencies"][0];
        assert_eq!(dep["package_name"], "axios");
        assert_eq!(dep["line"], 10);
        assert!(dep.get("used_in_workspaces").is_none());
    }

    #[test]
    fn json_unused_dependency_includes_cross_workspace_context() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/shared/package.json"),
                line: 6,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["unused_dependencies"][0];
        assert_eq!(
            dep["used_in_workspaces"],
            serde_json::json!(["packages/consumer"])
        );
    }

    #[test]
    fn json_unused_dev_dependency_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "vitest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: root.join("package.json"),
                line: 15,
                used_in_workspaces: Vec::new(),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["unused_dev_dependencies"][0];
        assert_eq!(dep["package_name"], "vitest");
    }

    #[test]
    fn json_unused_optional_dependency_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: root.join("package.json"),
                    line: 12,
                    used_in_workspaces: Vec::new(),
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["unused_optional_dependencies"][0];
        assert_eq!(dep["package_name"], "fsevents");
        assert_eq!(output["total_issues"], 1);
    }

    #[test]
    fn json_unused_enum_member_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Color".to_string(),
                member_name: "Purple".to_string(),
                kind: MemberKind::EnumMember,
                line: 5,
                col: 2,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let member = &output["unused_enum_members"][0];
        assert_eq!(member["parent_name"], "Color");
        assert_eq!(member["member_name"], "Purple");
        assert_eq!(member["line"], 5);
        assert_eq!(member["path"], "src/enums.ts");
    }

    #[test]
    fn json_unused_class_member_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/api.ts"),
                parent_name: "ApiClient".to_string(),
                member_name: "deprecatedFetch".to_string(),
                kind: MemberKind::ClassMethod,
                line: 100,
                col: 4,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let member = &output["unused_class_members"][0];
        assert_eq!(member["parent_name"], "ApiClient");
        assert_eq!(member["member_name"], "deprecatedFetch");
        assert_eq!(member["line"], 100);
    }

    #[test]
    fn json_unresolved_import_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/app.ts"),
                specifier: "@acme/missing-pkg".to_string(),
                line: 7,
                col: 0,
                specifier_col: 0,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let import = &output["unresolved_imports"][0];
        assert_eq!(import["specifier"], "@acme/missing-pkg");
        assert_eq!(import["line"], 7);
        assert_eq!(import["path"], "src/app.ts");
    }

    #[test]
    fn json_unlisted_dependency_contains_import_sites() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "dotenv".to_string(),
                    imported_from: vec![
                        ImportSite {
                            path: root.join("src/config.ts"),
                            line: 1,
                            col: 0,
                        },
                        ImportSite {
                            path: root.join("src/server.ts"),
                            line: 3,
                            col: 0,
                        },
                    ],
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["unlisted_dependencies"][0];
        assert_eq!(dep["package_name"], "dotenv");
        let sites = dep["imported_from"].as_array().unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0]["path"], "src/config.ts");
        assert_eq!(sites[1]["path"], "src/server.ts");
    }

    #[test]
    fn json_duplicate_export_contains_locations() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Button".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/ui.ts"),
                        line: 10,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/components.ts"),
                        line: 25,
                        col: 0,
                    },
                ],
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dup = &output["duplicate_exports"][0];
        assert_eq!(dup["export_name"], "Button");
        let locs = dup["locations"].as_array().unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0]["line"], 10);
        assert_eq!(locs[1]["line"], 25);
    }

    #[test]
    fn duplicate_export_add_to_config_is_auto_fixable_when_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(".fallowrc.json"), "{}\n").unwrap();
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Button".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/ui.ts"),
                        line: 10,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/components.ts"),
                        line: 25,
                        col: 0,
                    },
                ],
            }));

        let output = build_json(&results, root, Duration::ZERO).unwrap();
        let actions = output["duplicate_exports"][0]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["type"], "add-to-config");
        assert_eq!(actions[0]["auto_fixable"], true);
    }

    #[test]
    fn duplicate_export_add_to_config_is_auto_fixable_when_create_fallback_allowed() {
        // No config file exists, and the dir is NOT inside a monorepo
        // subpackage, so `fallow fix` can safely create one. The action
        // must surface as auto_fixable: true (per the per-instance flip
        // documented on AddToConfigAction).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Button".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/ui.ts"),
                        line: 10,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/components.ts"),
                        line: 25,
                        col: 0,
                    },
                ],
            }));

        let output = build_json(&results, root, Duration::ZERO).unwrap();
        let actions = output["duplicate_exports"][0]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["type"], "add-to-config");
        assert_eq!(actions[0]["auto_fixable"], true);
    }

    #[test]
    fn duplicate_export_add_to_config_is_not_auto_fixable_in_monorepo_subpackage() {
        // A workspace marker above the invocation directory means
        // `fallow fix` would refuse to create a per-subpackage config.
        // The action must surface as auto_fixable: false so agents and
        // IDEs don't blindly pipe into `fallow fix --yes`.
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        std::fs::write(
            workspace.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .unwrap();
        let sub = workspace.join("packages/ui");
        std::fs::create_dir_all(&sub).unwrap();
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Button".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: sub.join("src/ui.ts"),
                        line: 10,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: sub.join("src/components.ts"),
                        line: 25,
                        col: 0,
                    },
                ],
            }));

        let output = build_json(&results, &sub, Duration::ZERO).unwrap();
        let actions = output["duplicate_exports"][0]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["type"], "add-to-config");
        assert_eq!(actions[0]["auto_fixable"], false);
    }

    #[test]
    fn json_type_only_dependency_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let dep = &output["type_only_dependencies"][0];
        assert_eq!(dep["package_name"], "zod");
        assert_eq!(dep["line"], 8);
    }

    #[test]
    fn json_circular_dependency_contains_expected_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        root.join("src/a.ts"),
                        root.join("src/b.ts"),
                        root.join("src/c.ts"),
                    ],
                    length: 3,
                    line: 5,
                    col: 0,
                    is_cross_package: false,
                },
            ));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let cycle = &output["circular_dependencies"][0];
        assert_eq!(cycle["length"], 3);
        assert_eq!(cycle["line"], 5);
        let files = cycle["files"].as_array().unwrap();
        assert_eq!(files.len(), 3);
    }

    // ── Re-export tagging ───────────────────────────────────────────

    #[test]
    fn json_re_export_flagged_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "reExported".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["unused_exports"][0]["is_re_export"], true);
    }

    // ── Schema version stability ────────────────────────────────────

    #[test]
    fn json_schema_version_is_pinned() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["schema_version"], SCHEMA_VERSION);
        assert_eq!(output["schema_version"], 6);
    }

    // ── Version string ──────────────────────────────────────────────

    #[test]
    fn json_version_matches_cargo_pkg_version() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["version"], env!("CARGO_PKG_VERSION"));
    }

    // ── Elapsed time encoding ───────────────────────────────────────

    #[test]
    fn json_elapsed_ms_zero_duration() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let output = build_json(&results, &root, Duration::ZERO).expect("should serialize");

        assert_eq!(output["elapsed_ms"], 0);
    }

    #[test]
    fn json_elapsed_ms_large_duration() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_mins(2);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["elapsed_ms"], 120_000);
    }

    #[test]
    fn json_elapsed_ms_sub_millisecond_truncated() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        // 500 microseconds = 0 milliseconds (truncated)
        let elapsed = Duration::from_micros(500);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["elapsed_ms"], 0);
    }

    // ── Multiple issues of same type ────────────────────────────────

    #[test]
    fn json_multiple_unused_files() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/b.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/c.ts"),
            }));
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        assert_eq!(output["unused_files"].as_array().unwrap().len(), 3);
        assert_eq!(output["total_issues"], 3);
    }

    // ── strip_root_prefix unit tests ────────────────────────────────

    #[test]
    fn strip_root_prefix_on_string_value() {
        let mut value = serde_json::json!("/project/src/file.ts");
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value, "src/file.ts");
    }

    #[test]
    fn strip_root_prefix_leaves_non_matching_string() {
        let mut value = serde_json::json!("/other/src/file.ts");
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value, "/other/src/file.ts");
    }

    #[test]
    fn strip_root_prefix_recurses_into_arrays() {
        let mut value = serde_json::json!(["/project/a.ts", "/project/b.ts", "/other/c.ts"]);
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value[0], "a.ts");
        assert_eq!(value[1], "b.ts");
        assert_eq!(value[2], "/other/c.ts");
    }

    #[test]
    fn strip_root_prefix_recurses_into_nested_objects() {
        let mut value = serde_json::json!({
            "outer": {
                "path": "/project/src/nested.ts"
            }
        });
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value["outer"]["path"], "src/nested.ts");
    }

    #[test]
    fn strip_root_prefix_leaves_numbers_and_booleans() {
        let mut value = serde_json::json!({
            "line": 42,
            "is_type_only": false,
            "path": "/project/src/file.ts"
        });
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value["line"], 42);
        assert_eq!(value["is_type_only"], false);
        assert_eq!(value["path"], "src/file.ts");
    }

    #[test]
    fn strip_root_prefix_normalizes_windows_separators() {
        let mut value = serde_json::json!(r"/project\src\file.ts");
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value, "src/file.ts");
    }

    #[test]
    fn strip_root_prefix_handles_empty_string_after_strip() {
        // Edge case: the string IS the prefix (without trailing content).
        // This shouldn't happen in practice but should not panic.
        let mut value = serde_json::json!("/project/");
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value, "");
    }

    #[test]
    fn strip_root_prefix_deeply_nested_array_of_objects() {
        let mut value = serde_json::json!({
            "groups": [{
                "instances": [{
                    "file": "/project/src/a.ts"
                }, {
                    "file": "/project/src/b.ts"
                }]
            }]
        });
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value["groups"][0]["instances"][0]["file"], "src/a.ts");
        assert_eq!(value["groups"][0]["instances"][1]["file"], "src/b.ts");
    }

    // ── Full sample results round-trip ──────────────────────────────

    #[test]
    fn json_full_sample_results_total_issues_correct() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let elapsed = Duration::from_millis(100);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        // sample_results adds one of each issue type (12 total).
        // unused_files + unused_exports + unused_types + unused_dependencies
        // + unused_dev_dependencies + unused_enum_members + unused_class_members
        // + unresolved_imports + unlisted_dependencies + duplicate_exports
        // + type_only_dependencies + circular_dependencies
        assert_eq!(output["total_issues"], results.total_issues());
    }

    #[test]
    fn json_full_sample_no_absolute_paths_in_output() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let json_str = serde_json::to_string(&output).expect("should stringify");
        // The root prefix should be stripped from all paths.
        assert!(!json_str.contains("/project/src/"));
        assert!(!json_str.contains("/project/package.json"));
    }

    // ── JSON output is deterministic ────────────────────────────────

    #[test]
    fn json_output_is_deterministic() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let elapsed = Duration::from_millis(50);

        let output1 = build_json(&results, &root, elapsed).expect("first build");
        let output2 = build_json(&results, &root, elapsed).expect("second build");

        assert_eq!(output1, output2);
    }

    // ── Metadata not overwritten by results fields ──────────────────

    #[test]
    fn json_results_fields_do_not_shadow_metadata() {
        // Ensure that serialized results don't contain keys like "schema_version"
        // that could overwrite the metadata fields we insert first.
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(99);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        // Metadata should reflect our explicit values, not anything from AnalysisResults.
        assert_eq!(output["schema_version"], 6);
        assert_eq!(output["elapsed_ms"], 99);
    }

    // ── All 14 issue type arrays present ────────────────────────────

    #[test]
    fn json_all_issue_type_arrays_present_in_empty_results() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let expected_arrays = [
            "unused_files",
            "unused_exports",
            "unused_types",
            "unused_dependencies",
            "unused_dev_dependencies",
            "unused_optional_dependencies",
            "unused_enum_members",
            "unused_class_members",
            "unresolved_imports",
            "unlisted_dependencies",
            "duplicate_exports",
            "type_only_dependencies",
            "test_only_dependencies",
            "circular_dependencies",
        ];
        for key in &expected_arrays {
            assert!(
                output[key].is_array(),
                "expected '{key}' to be an array in JSON output"
            );
        }
    }

    // ── insert_meta ─────────────────────────────────────────────────

    #[test]
    fn insert_meta_adds_key_to_object() {
        let mut output = serde_json::json!({ "foo": 1 });
        let meta = serde_json::json!({ "docs": "https://example.com" });
        insert_meta(&mut output, meta.clone());
        assert_eq!(output["_meta"], meta);
    }

    #[test]
    fn insert_meta_noop_on_non_object() {
        let mut output = serde_json::json!([1, 2, 3]);
        let meta = serde_json::json!({ "docs": "https://example.com" });
        insert_meta(&mut output, meta);
        // Should not panic or add anything
        assert!(output.is_array());
    }

    #[test]
    fn insert_meta_overwrites_existing_meta() {
        let mut output = serde_json::json!({ "_meta": "old" });
        let meta = serde_json::json!({ "new": true });
        insert_meta(&mut output, meta.clone());
        assert_eq!(output["_meta"], meta);
    }

    // ── strip_root_prefix with null value ──

    #[test]
    fn strip_root_prefix_null_unchanged() {
        let mut value = serde_json::Value::Null;
        strip_root_prefix(&mut value, "/project/");
        assert!(value.is_null());
    }

    // ── strip_root_prefix with empty string ──

    #[test]
    fn strip_root_prefix_empty_string() {
        let mut value = serde_json::json!("");
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value, "");
    }

    // ── strip_root_prefix on mixed nested structure ──

    #[test]
    fn strip_root_prefix_mixed_types() {
        let mut value = serde_json::json!({
            "path": "/project/src/file.ts",
            "line": 42,
            "flag": true,
            "nested": {
                "items": ["/project/a.ts", 99, null, "/project/b.ts"],
                "deep": { "path": "/project/c.ts" }
            }
        });
        strip_root_prefix(&mut value, "/project/");
        assert_eq!(value["path"], "src/file.ts");
        assert_eq!(value["line"], 42);
        assert_eq!(value["flag"], true);
        assert_eq!(value["nested"]["items"][0], "a.ts");
        assert_eq!(value["nested"]["items"][1], 99);
        assert!(value["nested"]["items"][2].is_null());
        assert_eq!(value["nested"]["items"][3], "b.ts");
        assert_eq!(value["nested"]["deep"]["path"], "c.ts");
    }

    // ── JSON with explain meta for check ──

    #[test]
    fn json_check_meta_integrates_correctly() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let elapsed = Duration::from_millis(0);
        let mut output = build_json(&results, &root, elapsed).expect("should serialize");
        insert_meta(&mut output, crate::explain::check_meta());

        assert!(output["_meta"]["docs"].is_string());
        assert!(output["_meta"]["rules"].is_object());
    }

    // ── JSON unused member kind serialization ──

    #[test]
    fn json_unused_member_kind_serialized() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Color".to_string(),
                member_name: "Red".to_string(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 2,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/class.ts"),
                parent_name: "Foo".to_string(),
                member_name: "bar".to_string(),
                kind: MemberKind::ClassMethod,
                line: 10,
                col: 4,
            }));

        let elapsed = Duration::from_millis(0);
        let output = build_json(&results, &root, elapsed).expect("should serialize");

        let enum_member = &output["unused_enum_members"][0];
        assert!(enum_member["kind"].is_string());
        let class_member = &output["unused_class_members"][0];
        assert!(class_member["kind"].is_string());
    }

    // ── Actions injection ──────────────────────────────────────────

    #[test]
    fn json_unused_export_has_actions() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let actions = output["unused_exports"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);

        // Fix action
        assert_eq!(actions[0]["type"], "remove-export");
        assert_eq!(actions[0]["auto_fixable"], true);
        assert!(actions[0].get("note").is_none());

        // Suppress action
        assert_eq!(actions[1]["type"], "suppress-line");
        assert_eq!(
            actions[1]["comment"],
            "// fallow-ignore-next-line unused-export"
        );
    }

    #[test]
    fn json_same_line_findings_share_multi_kind_suppression_comment() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/api.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/api.ts"),
                export_name: "OldType".to_string(),
                is_type_only: true,
                line: 10,
                col: 0,
                span_start: 60,
                is_re_export: false,
            }));
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let export_actions = output["unused_exports"][0]["actions"].as_array().unwrap();
        let type_actions = output["unused_types"][0]["actions"].as_array().unwrap();
        assert_eq!(
            export_actions[1]["comment"],
            "// fallow-ignore-next-line unused-export, unused-type"
        );
        assert_eq!(
            type_actions[1]["comment"],
            "// fallow-ignore-next-line unused-export, unused-type"
        );
    }

    #[test]
    fn audit_like_json_shares_suppression_comment_across_dead_code_and_complexity() {
        let mut output = serde_json::json!({
            "dead_code": {
                "unused_exports": [{
                    "path": "src/main.ts",
                    "line": 1,
                    "actions": [
                        { "type": "remove-export", "auto_fixable": true },
                        {
                            "type": "suppress-line",
                            "auto_fixable": false,
                            "comment": "// fallow-ignore-next-line unused-export"
                        }
                    ]
                }]
            },
            "complexity": {
                "findings": [{
                    "path": "src/main.ts",
                    "line": 1,
                    "actions": [
                        { "type": "refactor-function", "auto_fixable": false },
                        {
                            "type": "suppress-line",
                            "auto_fixable": false,
                            "comment": "// fallow-ignore-next-line complexity"
                        }
                    ]
                }]
            }
        });

        harmonize_multi_kind_suppress_line_actions(&mut output);

        assert_eq!(
            output["dead_code"]["unused_exports"][0]["actions"][1]["comment"],
            "// fallow-ignore-next-line unused-export, complexity"
        );
        assert_eq!(
            output["complexity"]["findings"][0]["actions"][1]["comment"],
            "// fallow-ignore-next-line unused-export, complexity"
        );
    }

    #[test]
    fn json_unused_file_has_file_suppress_and_note() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let actions = output["unused_files"][0]["actions"].as_array().unwrap();
        assert_eq!(actions[0]["type"], "delete-file");
        assert_eq!(actions[0]["auto_fixable"], false);
        assert!(actions[0]["note"].is_string());
        assert_eq!(actions[1]["type"], "suppress-file");
        assert_eq!(actions[1]["comment"], "// fallow-ignore-file unused-file");
    }

    #[test]
    fn json_unused_dependency_has_config_suppress_with_package_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let actions = output["unused_dependencies"][0]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["type"], "remove-dependency");
        assert_eq!(actions[0]["auto_fixable"], true);

        // Config suppress includes actual package name
        assert_eq!(actions[1]["type"], "add-to-config");
        assert_eq!(actions[1]["config_key"], "ignoreDependencies");
        assert_eq!(actions[1]["value"], "lodash");
    }

    #[test]
    fn json_cross_workspace_dependency_is_not_auto_fixable() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/shared/package.json"),
                line: 5,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }));
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let actions = output["unused_dependencies"][0]["actions"]
            .as_array()
            .unwrap();
        assert_eq!(actions[0]["type"], "move-dependency");
        assert_eq!(actions[0]["auto_fixable"], false);
        assert!(
            actions[0]["note"]
                .as_str()
                .unwrap()
                .contains("will not remove")
        );
        assert_eq!(actions[1]["type"], "add-to-config");
    }

    #[test]
    fn json_empty_results_have_no_actions_in_empty_arrays() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        // Empty arrays should remain empty
        assert!(output["unused_exports"].as_array().unwrap().is_empty());
        assert!(output["unused_files"].as_array().unwrap().is_empty());
    }

    #[test]
    fn json_all_issue_types_have_actions() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let output = build_json(&results, &root, Duration::ZERO).unwrap();

        let issue_keys = [
            "unused_files",
            "unused_exports",
            "unused_types",
            "unused_dependencies",
            "unused_dev_dependencies",
            "unused_optional_dependencies",
            "unused_enum_members",
            "unused_class_members",
            "unresolved_imports",
            "unlisted_dependencies",
            "duplicate_exports",
            "type_only_dependencies",
            "test_only_dependencies",
            "circular_dependencies",
        ];

        for key in &issue_keys {
            let arr = output[key].as_array().unwrap();
            if !arr.is_empty() {
                let actions = arr[0]["actions"].as_array();
                assert!(
                    actions.is_some() && !actions.unwrap().is_empty(),
                    "missing actions for {key}"
                );
            }
        }
    }

    // ── Health actions injection ───────────────────────────────────

    /// Test helper: deserialize a JSON finding shape into a typed
    /// [`ComplexityViolation`], run [`HealthFinding::with_actions`] with
    /// the supplied thresholds, and return the resulting `actions` array
    /// as `serde_json::Value` so existing JSON-shape assertions keep
    /// working after PR B2 of #384 moved finding action selection from
    /// the JSON post-pass into the typed wrapper.
    fn build_actions_for_finding_json(
        finding_json: serde_json::Value,
        opts: crate::health_types::HealthActionOptions,
        max_cyclomatic_threshold: u16,
        max_cognitive_threshold: u16,
        max_crap_threshold: f64,
    ) -> Vec<serde_json::Value> {
        let mut value = finding_json;
        // The CV struct skip-serializes `Option::None` fields, but the
        // shorthand `serde_json::json!({})` test fixtures sometimes omit
        // optional fields. Adapt missing defaults so deserialization
        // succeeds for synthetic test inputs.
        if let Some(map) = value.as_object_mut() {
            map.entry("col".to_string())
                .or_insert(serde_json::Value::from(0_u32));
            map.entry("line_count".to_string())
                .or_insert(serde_json::Value::from(0_u32));
            map.entry("param_count".to_string())
                .or_insert(serde_json::Value::from(0_u8));
            map.entry("severity".to_string())
                .or_insert(serde_json::Value::String("moderate".to_string()));
        }
        // ComplexityViolation derives Serialize only; for tests we
        // reconstruct it field by field via a small helper that reads
        // the JSON shape used by these tests.
        let violation = synthesize_complexity_violation(&value);
        let ctx = crate::health_types::HealthActionContext {
            opts,
            max_cyclomatic_threshold,
            max_cognitive_threshold,
            max_crap_threshold,
        };
        let finding = crate::health_types::HealthFinding::with_actions(violation, &ctx);
        let serialized = serde_json::to_value(&finding).expect("serialize HealthFinding");
        serialized["actions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Reads a JSON object with finding-shape fields and produces a
    /// [`ComplexityViolation`]. Test-only: panics on schema mismatches so
    /// authors notice when synthetic fixtures drift from the canonical
    /// shape.
    fn synthesize_complexity_violation(
        value: &serde_json::Value,
    ) -> crate::health_types::ComplexityViolation {
        use crate::health_types::{
            CoverageSource, CoverageTier, ExceededThreshold, FindingSeverity,
        };
        let exceeded = match value["exceeded"].as_str().unwrap_or("crap") {
            "cyclomatic" => ExceededThreshold::Cyclomatic,
            "cognitive" => ExceededThreshold::Cognitive,
            "both" => ExceededThreshold::Both,
            "crap" => ExceededThreshold::Crap,
            "cyclomatic_crap" => ExceededThreshold::CyclomaticCrap,
            "cognitive_crap" => ExceededThreshold::CognitiveCrap,
            "all" => ExceededThreshold::All,
            other => panic!("unknown exceeded label: {other}"),
        };
        let severity = match value["severity"].as_str().unwrap_or("moderate") {
            "moderate" => FindingSeverity::Moderate,
            "high" => FindingSeverity::High,
            "critical" => FindingSeverity::Critical,
            other => panic!("unknown severity label: {other}"),
        };
        let coverage_tier = value
            .get("coverage_tier")
            .and_then(|v| v.as_str())
            .map(|t| match t {
                "none" => CoverageTier::None,
                "partial" => CoverageTier::Partial,
                "high" => CoverageTier::High,
                other => panic!("unknown coverage_tier label: {other}"),
            });
        let coverage_source =
            value
                .get("coverage_source")
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "istanbul" => CoverageSource::Istanbul,
                    "estimated" => CoverageSource::Estimated,
                    "estimated_component_inherited" => CoverageSource::EstimatedComponentInherited,
                    other => panic!("unknown coverage_source label: {other}"),
                });
        crate::health_types::ComplexityViolation {
            path: std::path::PathBuf::from(value["path"].as_str().unwrap_or("src/x.ts")),
            name: value["name"].as_str().unwrap_or("fn").to_string(),
            line: u32::try_from(value["line"].as_u64().unwrap_or(0)).unwrap_or(0),
            col: u32::try_from(value["col"].as_u64().unwrap_or(0)).unwrap_or(0),
            cyclomatic: u16::try_from(value["cyclomatic"].as_u64().unwrap_or(0)).unwrap_or(0),
            cognitive: u16::try_from(value["cognitive"].as_u64().unwrap_or(0)).unwrap_or(0),
            line_count: u32::try_from(value["line_count"].as_u64().unwrap_or(0)).unwrap_or(0),
            param_count: u8::try_from(value["param_count"].as_u64().unwrap_or(0)).unwrap_or(0),
            exceeded,
            severity,
            crap: value.get("crap").and_then(|v| v.as_f64()),
            coverage_pct: value.get("coverage_pct").and_then(|v| v.as_f64()),
            coverage_tier,
            coverage_source,
            inherited_from: value
                .get("inherited_from")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from),
            component_rollup: value.get("component_rollup").and_then(|v| {
                let map = v.as_object()?;
                Some(crate::health_types::ComponentRollup {
                    component: map.get("component")?.as_str()?.to_string(),
                    class_worst_function: map.get("class_worst_function")?.as_str()?.to_string(),
                    class_cyclomatic: u16::try_from(map.get("class_cyclomatic")?.as_u64()?).ok()?,
                    class_cognitive: u16::try_from(map.get("class_cognitive")?.as_u64()?).ok()?,
                    template_path: std::path::PathBuf::from(map.get("template_path")?.as_str()?),
                    template_cyclomatic: u16::try_from(map.get("template_cyclomatic")?.as_u64()?)
                        .ok()?,
                    template_cognitive: u16::try_from(map.get("template_cognitive")?.as_u64()?)
                        .ok()?,
                })
            }),
        }
    }

    #[test]
    fn health_finding_has_actions() {
        let actions = build_actions_for_finding_json(
            serde_json::json!({
                "path": "src/utils.ts",
                "name": "processData",
                "line": 10,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 150,
                "exceeded": "both"
            }),
            crate::health_types::HealthActionOptions::default(),
            20,
            15,
            30.0,
        );

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["type"], "refactor-function");
        assert_eq!(actions[0]["auto_fixable"], false);
        assert!(
            actions[0]["description"]
                .as_str()
                .unwrap()
                .contains("processData")
        );
        assert_eq!(actions[1]["type"], "suppress-line");
        assert_eq!(
            actions[1]["comment"],
            "// fallow-ignore-next-line complexity"
        );
    }

    // Tests for the retired hotspot / target action post-pass now live in
    // `crates/cli/src/health_types/finding.rs::hotspot_target_tests`
    // alongside the typed `HotspotFinding` and `RefactoringTargetFinding`
    // wrappers and the typed `build_hotspot_actions` /
    // `build_refactoring_target_actions` helpers they exercise. The four
    // direct `suggest_codeowners_pattern` tests moved alongside that helper.

    #[test]
    fn health_finding_suppress_has_placement() {
        let actions = build_actions_for_finding_json(
            serde_json::json!({
                "path": "src/utils.ts",
                "name": "processData",
                "line": 10,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 150,
                "exceeded": "both"
            }),
            crate::health_types::HealthActionOptions::default(),
            20,
            15,
            30.0,
        );

        assert_eq!(actions[1]["placement"], "above-function-declaration");
    }

    #[test]
    fn html_template_health_finding_uses_html_suppression() {
        let actions = build_actions_for_finding_json(
            serde_json::json!({
                "path": "src/app.component.html",
                "name": "<template>",
                "line": 1,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 40,
                "exceeded": "both"
            }),
            crate::health_types::HealthActionOptions::default(),
            20,
            15,
            30.0,
        );

        let suppress = &actions[1];
        assert_eq!(suppress["type"], "suppress-file");
        assert_eq!(
            suppress["comment"],
            "<!-- fallow-ignore-file complexity -->"
        );
        assert_eq!(suppress["placement"], "top-of-template");
    }

    #[test]
    fn inline_template_health_finding_uses_decorator_suppression() {
        let actions = build_actions_for_finding_json(
            serde_json::json!({
                "path": "src/app.component.ts",
                "name": "<template>",
                "line": 5,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 40,
                "exceeded": "both"
            }),
            crate::health_types::HealthActionOptions::default(),
            20,
            15,
            30.0,
        );

        let refactor = &actions[0];
        assert_eq!(refactor["type"], "refactor-function");
        assert!(
            refactor["description"]
                .as_str()
                .unwrap()
                .contains("template complexity")
        );
        let suppress = &actions[1];
        assert_eq!(suppress["type"], "suppress-line");
        assert_eq!(
            suppress["description"],
            "Suppress with an inline comment above the Angular decorator"
        );
        assert_eq!(suppress["placement"], "above-angular-decorator");
    }

    // Duplication action injection tests retired in #409: the wire shape
    // now flows through typed wrappers in `crate::output_dupes`; the
    // wrappers carry their own unit tests covering the same per-finding
    // `actions[]` semantics (extract-shared / apply-suggestion /
    // suppress-line position-0 invariants, issue #393 nested groups).

    // ── Tier-aware health action emission ──────────────────────────

    /// Helper: build a health JSON envelope with a single CRAP-only finding.
    /// Default cognitive complexity is 12 (above the cognitive floor at the
    /// default `max_cognitive_threshold / 2 = 7.5`); use
    /// `crap_only_finding_envelope_with_cognitive` to exercise low-cog cases
    /// (flat dispatchers, JSX render maps) where the cognitive floor should
    /// suppress the secondary refactor.
    fn crap_only_finding_envelope(
        coverage_tier: Option<&str>,
        cyclomatic: u16,
        max_cyclomatic_threshold: u16,
    ) -> serde_json::Value {
        crap_only_finding_envelope_with_max_crap(
            coverage_tier,
            cyclomatic,
            12,
            max_cyclomatic_threshold,
            15,
            30.0,
        )
    }

    fn crap_only_finding_envelope_with_cognitive(
        coverage_tier: Option<&str>,
        cyclomatic: u16,
        cognitive: u16,
        max_cyclomatic_threshold: u16,
    ) -> serde_json::Value {
        crap_only_finding_envelope_with_max_crap(
            coverage_tier,
            cyclomatic,
            cognitive,
            max_cyclomatic_threshold,
            15,
            30.0,
        )
    }

    /// Build a synthetic health JSON envelope around a single typed
    /// [`HealthFinding`] so the existing JSON-shaped assertions in this
    /// module keep working after PR B2 of #384 moved action selection from
    /// the JSON post-pass into [`HealthFinding::with_actions`]. Defaults to
    /// the un-suppressed action context; callers that want to exercise the
    /// `omit_suppress_line` path should go through
    /// [`build_finding_envelope_with_ctx`].
    fn crap_only_finding_envelope_with_max_crap(
        coverage_tier: Option<&str>,
        cyclomatic: u16,
        cognitive: u16,
        max_cyclomatic_threshold: u16,
        max_cognitive_threshold: u16,
        max_crap_threshold: f64,
    ) -> serde_json::Value {
        build_finding_envelope_with_ctx(
            coverage_tier,
            cyclomatic,
            cognitive,
            max_cyclomatic_threshold,
            max_cognitive_threshold,
            max_crap_threshold,
            crate::health_types::HealthActionOptions::default(),
        )
    }

    /// Build a single-finding health JSON envelope with the supplied action
    /// context. Used by the suppress-line gating tests to exercise the
    /// `baseline-active` / `config-disabled` reasons.
    fn build_finding_envelope_with_ctx(
        coverage_tier: Option<&str>,
        cyclomatic: u16,
        cognitive: u16,
        max_cyclomatic_threshold: u16,
        max_cognitive_threshold: u16,
        max_crap_threshold: f64,
        action_opts: crate::health_types::HealthActionOptions,
    ) -> serde_json::Value {
        let tier = coverage_tier.map(|t| match t {
            "none" => crate::health_types::CoverageTier::None,
            "partial" => crate::health_types::CoverageTier::Partial,
            "high" => crate::health_types::CoverageTier::High,
            other => panic!("unknown coverage tier label: {other}"),
        });
        let violation = crate::health_types::ComplexityViolation {
            path: std::path::PathBuf::from("src/risk.ts"),
            name: "computeScore".to_string(),
            line: 12,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 40,
            param_count: 0,
            exceeded: crate::health_types::ExceededThreshold::Crap,
            severity: crate::health_types::FindingSeverity::Moderate,
            crap: Some(35.5),
            coverage_pct: None,
            coverage_tier: tier,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
        };
        let ctx = crate::health_types::HealthActionContext {
            opts: action_opts,
            max_cyclomatic_threshold,
            max_cognitive_threshold,
            max_crap_threshold,
        };
        let finding = crate::health_types::HealthFinding::with_actions(violation, &ctx);
        let actions_meta = if action_opts.omit_suppress_line {
            Some(serde_json::json!({
                "suppression_hints_omitted": true,
                "reason": action_opts.omit_reason.unwrap_or("unspecified"),
                "scope": "health-findings",
            }))
        } else {
            None
        };
        let mut envelope = serde_json::json!({
            "findings": [serde_json::to_value(&finding).unwrap()],
            "summary": {
                "max_cyclomatic_threshold": max_cyclomatic_threshold,
                "max_cognitive_threshold": max_cognitive_threshold,
                "max_crap_threshold": max_crap_threshold,
            },
        });
        if let Some(meta) = actions_meta
            && let Some(map) = envelope.as_object_mut()
        {
            map.insert("actions_meta".to_string(), meta);
        }
        envelope
    }

    #[test]
    fn crap_only_tier_none_emits_add_tests() {
        let output = crap_only_finding_envelope(Some("none"), 6, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "add-tests"),
            "tier=none crap-only must emit add-tests, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "increase-coverage"),
            "tier=none must not emit increase-coverage"
        );
    }

    #[test]
    fn crap_only_tier_partial_emits_increase_coverage() {
        let output = crap_only_finding_envelope(Some("partial"), 6, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "increase-coverage"),
            "tier=partial crap-only must emit increase-coverage, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "add-tests"),
            "tier=partial must not emit add-tests"
        );
    }

    #[test]
    fn crap_only_tier_high_emits_increase_coverage_when_full_coverage_can_clear_crap() {
        // CC=20 at 70% coverage has CRAP 30.8, but at 100% coverage CRAP
        // falls to 20.0, below the default max_crap_threshold=30. Coverage
        // is therefore still a valid remediation even though tier=high.
        let output = crap_only_finding_envelope(Some("high"), 20, 30);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "increase-coverage"),
            "tier=high crap-only must still emit increase-coverage when full coverage can clear CRAP, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "refactor-function"),
            "coverage-remediable crap-only findings should not get refactor-function unless near the cyclomatic threshold"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "add-tests"),
            "tier=high must not emit add-tests"
        );
    }

    #[test]
    fn crap_only_emits_refactor_when_full_coverage_cannot_clear_crap() {
        // At 100% coverage CRAP bottoms out at CC. With CC=35 and a CRAP
        // threshold of 30, tests alone can reduce risk but cannot clear the
        // finding; the primary action should be complexity reduction.
        let output = crap_only_finding_envelope_with_max_crap(Some("high"), 35, 12, 50, 15, 30.0);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "refactor-function"),
            "full-coverage-impossible CRAP-only finding must emit refactor-function, got {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "increase-coverage"),
            "must not emit increase-coverage when even 100% coverage cannot clear CRAP"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "add-tests"),
            "must not emit add-tests when even 100% coverage cannot clear CRAP"
        );
    }

    #[test]
    fn crap_only_high_cc_appends_secondary_refactor() {
        // CC=16 with threshold=20 => within SECONDARY_REFACTOR_BAND (5)
        // of the threshold; refactor is a useful complement to coverage.
        let output = crap_only_finding_envelope(Some("none"), 16, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "add-tests"),
            "near-threshold crap-only still emits the primary tier action"
        );
        assert!(
            actions.iter().any(|a| a["type"] == "refactor-function"),
            "near-threshold crap-only must also emit secondary refactor-function"
        );
    }

    #[test]
    fn crap_only_far_below_threshold_no_secondary_refactor() {
        // CC=6 with threshold=20 => far outside the band; refactor not added.
        let output = crap_only_finding_envelope(Some("none"), 6, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            !actions.iter().any(|a| a["type"] == "refactor-function"),
            "low-CC crap-only should not get a secondary refactor-function"
        );
    }

    #[test]
    fn crap_only_near_threshold_low_cognitive_no_secondary_refactor() {
        // Cognitive floor regression. Real-world example from vrs-portals:
        // a flat type-tag dispatcher with CC=17 (within SECONDARY_REFACTOR_BAND
        // of the default cyclomatic threshold of 20) but cognitive=2 (a single
        // switch, no nesting). Suggesting "extract helpers, simplify branching"
        // is wrong-target advice for declarative dispatchers; the cognitive
        // floor at `max_cognitive_threshold / 2` (default 7) suppresses the
        // secondary refactor in this case while still firing it for genuinely
        // tangled functions (CC>=15 + cog>=8) where refactor would help.
        let output = crap_only_finding_envelope_with_cognitive(Some("none"), 17, 2, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "add-tests"),
            "primary tier action still emits"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "refactor-function"),
            "near-threshold CC with cognitive below floor must NOT emit secondary refactor (got {actions:?})"
        );
    }

    #[test]
    fn crap_only_near_threshold_high_cognitive_emits_secondary_refactor() {
        // Companion to the cognitive-floor regression: when cognitive is at or
        // above the floor, the secondary refactor should still fire. CC=16
        // and cognitive=10 (above default floor of 7) is the canonical
        // "tangled but near-threshold" function that genuinely benefits from
        // both coverage AND refactoring.
        let output = crap_only_finding_envelope_with_cognitive(Some("none"), 16, 10, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "add-tests"),
            "primary tier action still emits"
        );
        assert!(
            actions.iter().any(|a| a["type"] == "refactor-function"),
            "near-threshold CC with cognitive above floor must emit secondary refactor (got {actions:?})"
        );
    }

    #[test]
    fn cyclomatic_only_emits_only_refactor_function() {
        let actions = build_actions_for_finding_json(
            serde_json::json!({
                "path": "src/cyclo.ts",
                "name": "branchy",
                "line": 5,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 10,
                "line_count": 80,
                "exceeded": "cyclomatic",
            }),
            crate::health_types::HealthActionOptions::default(),
            20,
            15,
            30.0,
        );
        assert!(
            actions.iter().any(|a| a["type"] == "refactor-function"),
            "non-CRAP findings emit refactor-function"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "add-tests"),
            "non-CRAP findings must not emit add-tests"
        );
        assert!(
            !actions.iter().any(|a| a["type"] == "increase-coverage"),
            "non-CRAP findings must not emit increase-coverage"
        );
    }

    // ── Suppress-line gating ──────────────────────────────────────

    #[test]
    fn suppress_line_omitted_when_baseline_active() {
        let output = build_finding_envelope_with_ctx(
            Some("none"),
            6,
            12,
            20,
            15,
            30.0,
            crate::health_types::HealthActionOptions {
                omit_suppress_line: true,
                omit_reason: Some("baseline-active"),
            },
        );
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            !actions.iter().any(|a| a["type"] == "suppress-line"),
            "baseline-active must not emit suppress-line, got {actions:?}"
        );
        assert_eq!(
            output["actions_meta"]["suppression_hints_omitted"],
            serde_json::Value::Bool(true)
        );
        assert_eq!(output["actions_meta"]["reason"], "baseline-active");
        assert_eq!(output["actions_meta"]["scope"], "health-findings");
    }

    #[test]
    fn suppress_line_omitted_when_config_disabled() {
        let output = build_finding_envelope_with_ctx(
            Some("none"),
            6,
            12,
            20,
            15,
            30.0,
            crate::health_types::HealthActionOptions {
                omit_suppress_line: true,
                omit_reason: Some("config-disabled"),
            },
        );
        assert_eq!(output["actions_meta"]["reason"], "config-disabled");
    }

    #[test]
    fn suppress_line_emitted_by_default() {
        let output = crap_only_finding_envelope(Some("none"), 6, 20);
        let actions = output["findings"][0]["actions"].as_array().unwrap();
        assert!(
            actions.iter().any(|a| a["type"] == "suppress-line"),
            "default opts must emit suppress-line"
        );
        assert!(
            output.get("actions_meta").is_none(),
            "actions_meta must be absent when no omission occurred"
        );
    }

    /// Drift guard: every action `type` value emitted by the action builder
    /// must appear in `docs/output-schema.json`'s `HealthFindingAction.type`
    /// enum. Previously the schema listed only `[refactor-function,
    /// suppress-line]` while the code emitted `add-tests` for CRAP findings,
    /// silently producing schema-invalid output for any consumer using the
    /// schema for validation.
    #[test]
    fn every_emitted_health_action_type_is_in_schema_enum() {
        // Exercise every distinct emission path. The list mirrors the match
        // in `build_crap_coverage_action` and the surrounding refactor/
        // suppress-line emissions in `build_health_finding_actions`.
        let cases = [
            // (exceeded, coverage_tier, cyclomatic, max_cyclomatic_threshold)
            ("crap", Some("none"), 6_u16, 20_u16),
            ("crap", Some("partial"), 6, 20),
            ("crap", Some("high"), 12, 20),
            ("crap", Some("none"), 16, 20), // near threshold => secondary refactor
            ("cyclomatic", None, 25, 20),
            ("cognitive_crap", Some("partial"), 6, 20),
            ("all", Some("none"), 25, 20),
        ];

        let mut emitted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (exceeded, tier, cc, max) in cases {
            let mut finding = serde_json::json!({
                "path": "src/x.ts",
                "name": "fn",
                "line": 1,
                "col": 0,
                "cyclomatic": cc,
                "cognitive": 5,
                "line_count": 10,
                "exceeded": exceeded,
                "crap": 35.0,
            });
            if let Some(t) = tier {
                finding["coverage_tier"] = serde_json::Value::String(t.to_owned());
            }
            let actions = build_actions_for_finding_json(
                finding,
                crate::health_types::HealthActionOptions::default(),
                max,
                15,
                30.0,
            );
            for action in &actions {
                if let Some(ty) = action["type"].as_str() {
                    emitted.insert(ty.to_owned());
                }
            }
        }

        // Load the schema enum once. Phase 8 derives `HealthFindingActionType`
        // from a Rust enum whose schemars derive produces `oneOf: [{const: ...},
        // ...]` (one branch per variant) rather than a flat `enum: [...]`. We
        // accept either form here so the drift guard tolerates a future
        // schemars-version flip back to the flat shape.
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("output-schema.json");
        let raw = std::fs::read_to_string(&schema_path)
            .expect("docs/output-schema.json must be readable for the drift-guard test");
        let schema: serde_json::Value = serde_json::from_str(&raw).expect("schema parses");
        let type_field = &schema["definitions"]["HealthFindingAction"]["properties"]["type"];
        let type_def = if let Some(reference) = type_field.get("$ref").and_then(|r| r.as_str()) {
            let name = reference
                .strip_prefix("#/definitions/")
                .expect("HealthFindingAction.type $ref points into #/definitions/");
            &schema["definitions"][name]
        } else {
            type_field
        };
        let mut enum_values: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if let Some(arr) = type_def.get("enum").and_then(|e| e.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    enum_values.insert(s.to_owned());
                }
            }
        }
        if let Some(arr) = type_def.get("oneOf").and_then(|e| e.as_array()) {
            for branch in arr {
                if let Some(s) = branch.get("const").and_then(|c| c.as_str()) {
                    enum_values.insert(s.to_owned());
                }
            }
        }
        assert!(
            !enum_values.is_empty(),
            "could not extract HealthFindingActionType variants from schema (neither `enum` nor `oneOf` with `const` branches)"
        );

        for ty in &emitted {
            assert!(
                enum_values.contains(ty),
                "build_health_finding_actions emitted action type `{ty}` but \
                 docs/output-schema.json HealthFindingAction.type enum does \
                 not list it. Add it to the schema (and any downstream \
                 typed consumers) when introducing a new action type."
            );
        }
    }

    /// Regression for issue #412: prevent reintroduction of the legacy
    /// `inject_*` / `augment_*` post-pass pattern in this file. Every
    /// JSON `actions[]` array on every finding type should flow from a
    /// typed `serde(flatten)` envelope, not from a post-construction
    /// mutation of a `serde_json::Value` tree.
    ///
    /// The allow-list mirrors the `HAND_MAINTAINED_ALLOW_LIST` pattern
    /// in `crates/cli/src/bin/schema_emit.rs`: each entry pairs a name
    /// with the issue that retires it. It is empty today; any addition
    /// needs an issue reference in the same commit. The gate also
    /// asserts no STALE entries, so removing a function without
    /// removing its allow-list entry fails the test and forces the
    /// cleanup commit.
    #[test]
    fn no_new_post_pass_helpers_in_json_rs() {
        const POST_PASS_ALLOW_LIST: &[(&str, &str)] = &[];
        let source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("report")
            .join("json.rs");
        let source = std::fs::read_to_string(&source_path).expect(
            "crates/cli/src/report/json.rs must be readable for the post-pass drift-guard test",
        );
        let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for line in source.lines() {
            if let Some(name) = extract_post_pass_fn_name(line) {
                found.insert(name.to_owned());
            }
        }
        let allow: std::collections::BTreeSet<&'static str> =
            POST_PASS_ALLOW_LIST.iter().map(|(name, _)| *name).collect();
        let unexpected: Vec<&str> = found
            .iter()
            .filter(|name| !allow.contains(name.as_str()))
            .map(String::as_str)
            .collect();
        let stale: Vec<&str> = allow
            .iter()
            .filter(|name| !found.contains(**name))
            .copied()
            .collect();
        assert!(
            unexpected.is_empty(),
            "new post-pass helper(s) defined in crates/cli/src/report/json.rs are not in \
             POST_PASS_ALLOW_LIST: {unexpected:?}.\n\
             The typed `serde(flatten)` envelope is the source of truth for `actions[]` on \
             every finding. If a new post-pass is genuinely needed, file a tracking issue, \
             add the entry to POST_PASS_ALLOW_LIST with the issue link as the reason, and \
             reference the issue in the PR body. See issue #412 for context."
        );
        assert!(
            stale.is_empty(),
            "stale entries in POST_PASS_ALLOW_LIST (function no longer defined in \
             crates/cli/src/report/json.rs): {stale:?}.\n\
             Remove them in the same commit that retired the function."
        );
    }

    /// Extracts an `inject_<name>` or `augment_<name>` identifier from a
    /// Rust function-definition line, handling `pub`, `pub(...)`,
    /// `async`, `const`, and `unsafe` modifiers. Returns `None` for
    /// non-definition lines (comments, call sites, doc strings).
    fn extract_post_pass_fn_name(line: &str) -> Option<&str> {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            return None;
        }
        let mut rest = trimmed;
        if let Some(after) = rest.strip_prefix("pub") {
            let after = after.trim_start();
            rest = if let Some(after) = after.strip_prefix('(') {
                let close = after.find(')')?;
                after[close + 1..].trim_start()
            } else {
                after
            };
        }
        for prefix in ["async ", "const ", "unsafe "] {
            if let Some(after) = rest.strip_prefix(prefix) {
                rest = after.trim_start();
            }
        }
        let after_fn = rest.strip_prefix("fn ")?;
        let name_end = after_fn
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after_fn.len());
        let name = &after_fn[..name_end];
        if name.starts_with("inject_") || name.starts_with("augment_") {
            Some(name)
        } else {
            None
        }
    }
}
