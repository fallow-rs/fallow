use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;
use fallow_types::envelope::{CheckSummary, ElapsedMs, EntryPoints, SchemaVersion, ToolVersion};

use super::shared::NAMESPACE_BARREL_HINT;
use super::{emit_json, normalize_uri};
use crate::explain;
use crate::output_envelope::{
    CheckGroupedEntry, CheckGroupedOutput, CheckOutput, DupesOutput, GroupByMode, HealthOutput,
};
use crate::report::grouping::{OwnershipResolver, ResultGroup};

/// JSON Pointer fragment URL describing the shape of the `value` field on an
/// `ignoreExports` `add-to-config` action. Agents can fetch and validate the
/// snippet before writing into a user's config.
const IGNORE_EXPORTS_VALUE_SCHEMA: &str =
    "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreExports";

/// JSON Pointer fragment URL describing the shape of the `value` field on an
/// `ignoreCatalogReferences` `add-to-config` action: one `{ package, catalog?,
/// consumer? }` entry to append.
const IGNORE_CATALOG_REFERENCES_VALUE_SCHEMA: &str = "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreCatalogReferences/items";

/// JSON Pointer fragment URL describing the shape of the `value` field on an
/// `ignoreDependencyOverrides` `add-to-config` action: one `{ package, source? }`
/// entry to append.
const IGNORE_DEPENDENCY_OVERRIDES_VALUE_SCHEMA: &str = "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreDependencyOverrides/items";

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
        .map(|group| CheckGroupedEntry {
            key: group.key.clone(),
            owners: group.owners.clone(),
            total_issues: group.results.total_issues(),
            results: group.results.clone(),
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
    // Strip and inject per group separately so each `groups[]` entry carries
    // its own action arrays (`inject_actions` and the suppression harmonizer
    // only walk the top-level map).
    if let Some(arr) = output.get_mut("groups").and_then(|v| v.as_array_mut()) {
        for entry in arr {
            strip_root_prefix(entry, &root_prefix);
            inject_actions(entry, config_fixable);
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
    let envelope = CheckOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        total_issues: results.total_issues(),
        entry_points: results.entry_point_summary.as_ref().map(|ep| EntryPoints {
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
        summary: build_check_summary(results),
        results: results.clone(),
        baseline_deltas: None,
        baseline: None,
        regression: None,
        meta: None,
    };

    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    // strip_root_prefix must run before inject_actions so that injected
    // action fields (static strings and package names) are not processed
    // by the path stripper.
    strip_root_prefix(&mut output, &root_prefix);
    inject_actions(&mut output, config_fixable);
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

// ── Fix action injection ────────────────────────────────────────

/// Suppress mechanism for an issue type.
enum SuppressKind {
    /// `// fallow-ignore-next-line <type>` on the line before.
    InlineComment,
    /// `# fallow-ignore-next-line <type>` on the line before (YAML / shell).
    YamlComment,
    /// Add to `ignoreCatalogReferences` in fallow config (with optional
    /// catalog + consumer scope).
    AddToConfigIgnoreCatalogReferences,
    /// Add to `ignoreDependencyOverrides` in fallow config (with optional
    /// source scope).
    AddToConfigIgnoreDependencyOverrides,
}

/// Specification for actions to inject per issue type.
struct ActionSpec {
    fix_type: &'static str,
    auto_fixable: bool,
    description: &'static str,
    note: Option<&'static str>,
    suppress: SuppressKind,
    issue_kind: &'static str,
}

/// Map an issue array key to its action specification.
fn actions_for_issue_type(key: &str) -> Option<ActionSpec> {
    match key {
        // `unused_files` is no longer post-pass-injected: the typed
        // `UnusedFileFinding` wrapper carries its `actions` array natively.
        // `unused_exports` and `unused_types` are no longer post-pass-injected:
        // the typed `UnusedExportFinding` / `UnusedTypeFinding` wrappers carry
        // their `actions` array natively, with the `is_re_export` note swap
        // applied at wrapper construction.
        // `private_type_leaks` is no longer post-pass-injected: the typed
        // `PrivateTypeLeakFinding` wrapper carries its `actions` array natively.
        // `unused_dependencies` / `unused_dev_dependencies` /
        // `unused_optional_dependencies` are no longer post-pass-injected:
        // the typed `UnusedDependencyFinding` / `UnusedDevDependencyFinding` /
        // `UnusedOptionalDependencyFinding` wrappers carry their `actions`
        // array natively, with the cross-workspace `move-dependency` swap
        // applied at wrapper construction.
        // `unused_enum_members` and `unused_class_members` are no longer
        // post-pass-injected: the typed `UnusedEnumMemberFinding` /
        // `UnusedClassMemberFinding` wrappers carry their `actions` array
        // natively.
        // `unresolved_imports` is no longer post-pass-injected: the typed
        // `UnresolvedImportFinding` wrapper carries its `actions` array natively.
        // `unlisted_dependencies` / `type_only_dependencies` /
        // `test_only_dependencies` are no longer post-pass-injected: the
        // typed `UnlistedDependencyFinding` / `TypeOnlyDependencyFinding` /
        // `TestOnlyDependencyFinding` wrappers carry their `actions` array
        // natively.
        "duplicate_exports" => Some(ActionSpec {
            fix_type: "remove-duplicate",
            auto_fixable: false,
            description: "Keep one canonical export location and remove the others",
            note: Some(NAMESPACE_BARREL_HINT),
            suppress: SuppressKind::InlineComment,
            issue_kind: "duplicate-export",
        }),
        // `circular_dependencies` and `boundary_violations` are no longer
        // post-pass-injected: the typed `CircularDependencyFinding` /
        // `BoundaryViolationFinding` wrappers carry their `actions` arrays
        // natively.
        "unused_catalog_entries" => Some(ActionSpec {
            fix_type: "remove-catalog-entry",
            auto_fixable: false,
            description: "Remove the entry from pnpm-workspace.yaml",
            note: Some(
                "If any consumer declares the same package with a hardcoded version, switch the consumer to `catalog:` before removing",
            ),
            suppress: SuppressKind::YamlComment,
            issue_kind: "unused-catalog-entry",
        }),
        "empty_catalog_groups" => Some(ActionSpec {
            fix_type: "remove-empty-catalog-group",
            auto_fixable: true,
            description: "Remove the empty named catalog group from pnpm-workspace.yaml",
            note: Some(
                "Only named groups under `catalogs:` are flagged; the top-level `catalog:` hook is intentionally ignored",
            ),
            suppress: SuppressKind::YamlComment,
            issue_kind: "empty-catalog-group",
        }),
        // The primary fix for unresolved-catalog-references is computed in
        // build_actions because it discriminates on `available_in_catalogs`.
        // This entry serves as the fallback / suppression spec only.
        "unresolved_catalog_references" => Some(ActionSpec {
            fix_type: "remove-catalog-reference",
            auto_fixable: false,
            description: "Remove the catalog reference and pin a hardcoded version in package.json",
            note: Some(
                "Use only when neither another catalog declares the package nor the named catalog should grow to include it",
            ),
            suppress: SuppressKind::AddToConfigIgnoreCatalogReferences,
            issue_kind: "unresolved-catalog-reference",
        }),
        "unused_dependency_overrides" => Some(ActionSpec {
            fix_type: "remove-dependency-override",
            auto_fixable: false,
            description: "Remove the override entry from pnpm-workspace.yaml or pnpm.overrides",
            note: Some(
                "Conservative static check; verify against `pnpm install --frozen-lockfile` before removing in case the override targets a transitive dependency (CVE-fix pattern)",
            ),
            suppress: SuppressKind::AddToConfigIgnoreDependencyOverrides,
            issue_kind: "unused-dependency-override",
        }),
        "misconfigured_dependency_overrides" => Some(ActionSpec {
            fix_type: "fix-dependency-override",
            auto_fixable: false,
            description: "Fix the override key or value: pnpm refuses to honor entries with an unparsable key or empty value",
            note: Some(
                "Common shapes: bare `pkg`, scoped `@scope/pkg`, version-selector `pkg@<2`, parent-chain `parent>child`. Valid values include semver ranges, `-` (removal), `$ref` (self-ref), and `npm:alias@^1`.",
            ),
            suppress: SuppressKind::AddToConfigIgnoreDependencyOverrides,
            issue_kind: "misconfigured-dependency-override",
        }),
        _ => None,
    }
}

/// Build the discriminated primary action for an `unresolved_catalog_references`
/// finding. Reads `available_in_catalogs` to decide between the two high-confidence
/// machine-actionable fixes; the result is appended ahead of the generic
/// `remove-catalog-reference` fallback in `build_actions`.
fn build_unresolved_catalog_reference_primary_action(
    item: &serde_json::Value,
) -> serde_json::Value {
    let available: Vec<String> = item
        .get("available_in_catalogs")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let package_name = item
        .get("entry_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("package");
    let current_catalog = item
        .get("catalog_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("default");
    if available.is_empty() {
        serde_json::json!({
            "type": "add-catalog-entry",
            "auto_fixable": false,
            "description": format!(
                "Add `{package_name}` to the `{current_catalog}` catalog in pnpm-workspace.yaml",
            ),
            "note": "Pin a version that satisfies the consumer's import; no other catalog declares this package today",
        })
    } else {
        // When exactly one alternative catalog declares this package, the fix
        // is unambiguous: name it as `suggested_target` so deterministic agents
        // (rule-based, not LLM-driven) can land the edit without picking from
        // a list. LLM agents that read `available_in_catalogs[0]` arrive at
        // the same answer; this field just makes the choice machine-explicit.
        let suggested_target = (available.len() == 1).then(|| available[0].clone());
        let mut action = serde_json::json!({
            "type": "update-catalog-reference",
            "auto_fixable": false,
            "description": format!(
                "Switch the reference from `catalog:{current_catalog}` to a catalog that declares `{package_name}`",
            ),
            "available_in_catalogs": available,
        });
        if let Some(target) = suggested_target
            && let serde_json::Value::Object(map) = &mut action
        {
            map.insert("suggested_target".to_string(), serde_json::json!(target));
        }
        action
    }
}

/// Build the `actions` array for a single issue item.
fn build_actions(
    item: &serde_json::Value,
    issue_key: &str,
    spec: &ActionSpec,
    config_fixable: bool,
) -> serde_json::Value {
    let mut actions = Vec::with_capacity(2);
    let cross_workspace_dependency = is_dependency_issue(issue_key)
        && item
            .get("used_in_workspaces")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|workspaces| !workspaces.is_empty());

    // unresolved-catalog-references emits a discriminated primary action
    // before the generic remove-catalog-reference fallback below.
    if issue_key == "unresolved_catalog_references" {
        actions.push(build_unresolved_catalog_reference_primary_action(item));
    }

    // duplicate-exports: emit the safe `add-to-config` (ignoreExports) action
    // FIRST so position-0 (the documented primary slot, see HealthFindingAction
    // schema description) recommends the non-destructive path for the common
    // shadcn / Radix / bits-ui namespace-barrel case. The fix action below
    // (remove-duplicate) stays as the secondary, for findings where the
    // duplication is genuinely accidental.
    if issue_key == "duplicate_exports"
        && let Some(value) = build_duplicate_exports_config_value(item)
    {
        actions.push(serde_json::json!({
            "type": "add-to-config",
            "auto_fixable": config_fixable,
            "description": "Add an ignoreExports rule so these files are excluded from duplicate-export grouping (use when this duplication is an intentional namespace-barrel API).",
            "config_key": "ignoreExports",
            "value": value,
            "value_schema": IGNORE_EXPORTS_VALUE_SCHEMA,
        }));
    }

    // Primary fix action. `auto_fixable` is per-instance for issue types
    // whose `fallow fix` applier has per-finding guards (e.g.
    // `unused_catalog_entries` skips entries with non-empty
    // `hardcoded_consumers`); for those types the ActionSpec carries the
    // pessimistic default `false` and the per-instance override flips it
    // to `true` here when the applier would actually run.
    let auto_fixable = if issue_key == "unused_catalog_entries" {
        item.get("hardcoded_consumers")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty)
    } else {
        spec.auto_fixable
    };
    let mut fix_action = if cross_workspace_dependency {
        serde_json::json!({
            "type": "move-dependency",
            "auto_fixable": false,
            "description": "Move this dependency to the workspace package.json that imports it",
            "note": "fallow fix will not remove dependencies that are imported by another workspace",
        })
    } else {
        serde_json::json!({
            "type": spec.fix_type,
            "auto_fixable": auto_fixable,
            "description": spec.description,
        })
    };
    if let Some(note) = spec.note {
        fix_action["note"] = serde_json::json!(note);
    }
    actions.push(fix_action);

    // Suppress action — every action carries `auto_fixable` for uniform filtering.
    match spec.suppress {
        SuppressKind::InlineComment => {
            let mut suppress = serde_json::json!({
                "type": "suppress-line",
                "auto_fixable": false,
                "description": "Suppress with an inline comment above the line",
                "comment": format!("// fallow-ignore-next-line {}", spec.issue_kind),
            });
            // duplicate_exports has N locations, not one — flag multi-location scope.
            if issue_key == "duplicate_exports" {
                suppress["scope"] = serde_json::json!("per-location");
            }
            actions.push(suppress);
        }
        SuppressKind::YamlComment => {
            actions.push(serde_json::json!({
                "type": "suppress-line",
                "auto_fixable": false,
                "description": "Suppress with a YAML comment above the line",
                "comment": format!("# fallow-ignore-next-line {}", spec.issue_kind),
            }));
        }
        SuppressKind::AddToConfigIgnoreCatalogReferences => {
            let package_name = item
                .get("entry_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("package");
            let catalog_name = item
                .get("catalog_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("default");
            let consumer_path = item
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let value = serde_json::json!({
                "package": package_name,
                "catalog": catalog_name,
                "consumer": consumer_path,
            });
            actions.push(serde_json::json!({
                "type": "add-to-config",
                "auto_fixable": false,
                "description": "Suppress this reference via ignoreCatalogReferences in fallow config (use when the catalog edit is intentionally landing in a separate PR or the package is a placeholder).",
                "config_key": "ignoreCatalogReferences",
                "value": value,
                "value_schema": IGNORE_CATALOG_REFERENCES_VALUE_SCHEMA,
            }));
        }
        SuppressKind::AddToConfigIgnoreDependencyOverrides => {
            // `target_package` is set on unused overrides (the key parsed
            // successfully) but absent on misconfigured ones whose `raw_key`
            // could not be parsed. Falling back to `raw_key` is unsafe in the
            // misconfigured case: it may be empty or malformed. Skip the
            // suppress action entirely when neither field yields a non-empty
            // name; an `ignoreDependencyOverrides` entry with `package: ""`
            // would be silently ignored by the config parser.
            let package_name = item
                .get("target_package")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    item.get("raw_key")
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty())
                });
            if let Some(package_name) = package_name {
                let source = item
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("pnpm-workspace.yaml");
                let value = serde_json::json!({
                    "package": package_name,
                    "source": source,
                });
                actions.push(serde_json::json!({
                    "type": "add-to-config",
                    "auto_fixable": false,
                    "description": "Suppress this override finding via ignoreDependencyOverrides in fallow config (use for CVE-fix overrides that target a purely-transitive package).",
                    "config_key": "ignoreDependencyOverrides",
                    "value": value,
                    "value_schema": IGNORE_DEPENDENCY_OVERRIDES_VALUE_SCHEMA,
                }));
            }
        }
    }

    serde_json::Value::Array(actions)
}

/// Build a paste-ready `ignoreExports` config snippet from a duplicate-exports
/// finding's `locations` array. Returns one `{ file, exports: ["*"] }` entry per
/// distinct file in the finding, in stable order.
fn build_duplicate_exports_config_value(item: &serde_json::Value) -> Option<serde_json::Value> {
    let locations = item.get("locations")?.as_array()?;
    let mut entries: Vec<serde_json::Value> = Vec::with_capacity(locations.len());
    let mut seen: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
    for loc in locations {
        let Some(path) = loc.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !seen.insert(path) {
            continue;
        }
        entries.push(serde_json::json!({
            "file": path,
            "exports": ["*"],
        }));
    }
    if entries.is_empty() {
        return None;
    }
    Some(serde_json::Value::Array(entries))
}

fn is_dependency_issue(issue_key: &str) -> bool {
    matches!(
        issue_key,
        "unused_dependencies" | "unused_dev_dependencies" | "unused_optional_dependencies"
    )
}

/// Inject `actions` arrays into every issue item in the JSON output.
///
/// Walks each known issue-type array and appends an `actions` field
/// to every item, providing machine-actionable fix and suppress hints.
fn inject_actions(output: &mut serde_json::Value, config_fixable: bool) {
    let Some(map) = output.as_object_mut() else {
        return;
    };

    for (key, value) in map.iter_mut() {
        let Some(spec) = actions_for_issue_type(key) else {
            continue;
        };
        let Some(arr) = value.as_array_mut() else {
            continue;
        };
        for item in arr {
            let actions = build_actions(item, key, &spec, config_fixable);
            if let serde_json::Value::Object(obj) = item {
                obj.insert("actions".to_string(), actions);
            }
        }
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
        "boundary-violation" => 10,
        "code-duplication" => 11,
        "complexity" => 12,
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

/// Cyclomatic distance from `max_cyclomatic_threshold` at which a
/// CRAP-only finding still warrants a secondary `refactor-function` action.
///
/// Reasoning: a function whose cyclomatic count is within this band of the
/// configured threshold is "almost too complex" already, so refactoring is a
/// useful complement to the primary coverage action. Keeping the boundary
/// expressed as a band (threshold minus N) rather than a ratio links it
/// to the existing `health.maxCyclomatic` knob: tightening the threshold
/// automatically widens the population that gets the secondary suggestion.
const SECONDARY_REFACTOR_BAND: u16 = 5;

/// Options controlling how `inject_health_actions` populates JSON output.
///
/// `omit_suppress_line` skips the `suppress-line` action across every
/// health finding. Set when:
/// - A baseline is active (`opts.baseline.is_some()` or
///   `opts.save_baseline.is_some()`): the baseline file already suppresses
///   findings, and adding `// fallow-ignore-next-line` comments on top
///   creates dead annotations once the baseline regenerates.
/// - The team has opted out via `health.suggestInlineSuppression: false`.
///
/// When omitted, a top-level `actions_meta` object on the report records
/// the omission and the reason so consumers can audit "where did
/// health finding suppress-line go?" without having to grep the config
/// or CLI history. Wire shape is documented by `HealthActionsMeta` in
/// `crates/cli/src/health_types/`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HealthActionOptions {
    /// Skip emission of `suppress-line` action entries.
    pub omit_suppress_line: bool,
    /// Human-readable reason surfaced in the `actions_meta` breadcrumb when
    /// `omit_suppress_line` is true. Stable codes:
    /// - `"baseline-active"`: `--baseline` or `--save-baseline` was passed
    /// - `"config-disabled"`: `health.suggestInlineSuppression: false`
    pub omit_reason: Option<&'static str>,
}

/// Inject `actions` arrays into complexity findings in a health JSON output.
///
/// Walks `findings` and `targets` arrays, appending machine-actionable
/// fix and suppress hints to each item. The `opts` argument controls
/// whether `suppress-line` actions are emitted; when suppressed, an
/// `actions_meta` breadcrumb at the report root records the omission.
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) needed, used by audit.rs via re-export, but not part of public API"
)]
pub(crate) fn inject_health_actions(output: &mut serde_json::Value, opts: HealthActionOptions) {
    let Some(map) = output.as_object_mut() else {
        return;
    };

    // The complexity thresholds live on `summary.*_threshold`; read once so
    // action selection for findings has access without re-walking the envelope.
    let max_cyclomatic_threshold = map
        .get("summary")
        .and_then(|s| s.get("max_cyclomatic_threshold"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(20);
    let max_cognitive_threshold = map
        .get("summary")
        .and_then(|s| s.get("max_cognitive_threshold"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(15);
    let max_crap_threshold = map
        .get("summary")
        .and_then(|s| s.get("max_crap_threshold"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(30.0);

    // Complexity findings: refactor the function to reduce complexity
    if let Some(findings) = map.get_mut("findings").and_then(|v| v.as_array_mut()) {
        for item in findings {
            let actions = build_health_finding_actions(
                item,
                opts,
                max_cyclomatic_threshold,
                max_cognitive_threshold,
                max_crap_threshold,
            );
            if let serde_json::Value::Object(obj) = item {
                obj.insert("actions".to_string(), actions);
            }
        }
    }

    // Refactoring targets: apply the recommended refactoring
    if let Some(targets) = map.get_mut("targets").and_then(|v| v.as_array_mut()) {
        for item in targets {
            let actions = build_refactoring_target_actions(item);
            if let serde_json::Value::Object(obj) = item {
                obj.insert("actions".to_string(), actions);
            }
        }
    }

    // Hotspots: files that are both complex and frequently changing
    if let Some(hotspots) = map.get_mut("hotspots").and_then(|v| v.as_array_mut()) {
        for item in hotspots {
            let actions = build_hotspot_actions(item);
            if let serde_json::Value::Object(obj) = item {
                obj.insert("actions".to_string(), actions);
            }
        }
    }

    // Coverage gaps (untested files / exports) are now serialized through
    // typed `UntestedFileFinding` / `UntestedExportFinding` envelope wrappers
    // built by `compute_coverage_gaps` in `crates/cli/src/health/scoring.rs`.
    // No post-pass injection needed; the typed `actions` field flows through
    // serde natively.

    // Runtime coverage actions are emitted by the sidecar and serialized
    // directly via serde (see `RuntimeCoverageAction` in
    // `crates/cli/src/health_types/runtime_coverage.rs`), so no post-hoc
    // injection is needed here.

    // Auditable breadcrumb: when the suppress-line hint was omitted, record
    // it at the report root so consumers don't have to infer the absence.
    // Wire shape mirrors `HealthActionsMeta` in `crates/cli/src/health_types/`;
    // the JSON injection lives here (rather than on the typed envelope) because
    // the suppression context only becomes known inside the report builder.
    if opts.omit_suppress_line {
        let reason = opts.omit_reason.unwrap_or("unspecified");
        map.insert(
            "actions_meta".to_string(),
            serde_json::json!({
                "suppression_hints_omitted": true,
                "reason": reason,
                "scope": "health-findings",
            }),
        );
    }
}

/// Build the `actions` array for a single complexity finding.
///
/// The primary action depends on which thresholds were exceeded and the
/// finding's bucketed coverage tier (`none`/`partial`/`high`):
///
/// - Exceeded cyclomatic/cognitive only (no CRAP): `refactor-function`.
/// - Exceeded CRAP, tier `none` or absent: `add-tests` (no test path
///   reaches this function; start from scratch).
/// - Exceeded CRAP, tier `partial`: `increase-coverage` (file already has
///   some test path; add targeted assertions for uncovered branches).
/// - Exceeded CRAP, full coverage can clear CRAP: tier-specific coverage
///   action (`add-tests` for `none`, `increase-coverage` for `partial`/
///   `high`).
/// - Exceeded CRAP, full coverage cannot clear CRAP: `refactor-function`
///   because reducing cyclomatic complexity is the remaining lever.
/// - Exceeded both CRAP and cyclomatic/cognitive: emit BOTH the
///   tier-appropriate coverage action AND `refactor-function`.
/// - CRAP-only with cyclomatic close to the threshold (within
///   `SECONDARY_REFACTOR_BAND`): also append `refactor-function` as a
///   secondary action; the function is "almost too complex" already.
///
/// `suppress-line` is appended last unless `opts.omit_suppress_line` is
/// true (baseline active or `health.suggestInlineSuppression: false`).
fn build_health_finding_actions(
    item: &serde_json::Value,
    opts: HealthActionOptions,
    max_cyclomatic_threshold: u16,
    max_cognitive_threshold: u16,
    max_crap_threshold: f64,
) -> serde_json::Value {
    let name = item
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("function");
    let path = item
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let exceeded = item
        .get("exceeded")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let includes_crap = matches!(
        exceeded,
        "crap" | "cyclomatic_crap" | "cognitive_crap" | "all"
    );
    let crap_only = exceeded == "crap";
    let tier = item
        .get("coverage_tier")
        .and_then(serde_json::Value::as_str);
    let cyclomatic = item
        .get("cyclomatic")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(0);
    let cognitive = item
        .get("cognitive")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(0);
    let full_coverage_can_clear_crap = !includes_crap || f64::from(cyclomatic) < max_crap_threshold;

    let mut actions: Vec<serde_json::Value> = Vec::new();

    // Coverage-leaning action: only emitted when CRAP contributed.
    if includes_crap {
        let coverage_action = build_crap_coverage_action(name, tier, full_coverage_can_clear_crap);
        if let Some(action) = coverage_action {
            actions.push(action);
        }
    }

    // Refactor action conditions:
    //   1. Exceeded cyclomatic/cognitive (with or without CRAP), or
    //   2. CRAP-only where even full coverage cannot bring CRAP below the
    //      configured threshold, so reducing complexity is the remaining
    //      lever), or
    //   3. CRAP-only with cyclomatic within SECONDARY_REFACTOR_BAND of the
    //      threshold AND cognitive complexity past the cognitive floor (the
    //      function is almost too complex anyway and the cognitive signal
    //      confirms that refactoring would actually help). Without the
    //      cognitive floor, flat type-tag dispatchers and JSX render maps
    //      (high CC, near-zero cog) get a misleading refactor suggestion.
    //
    // `build_crap_coverage_action` returns `None` for case 2 instead of
    // pushing `refactor-function` itself, so this branch unconditionally
    // pushes the refactor entry without needing to dedupe.
    let crap_only_needs_complexity_reduction = crap_only && !full_coverage_can_clear_crap;
    let cognitive_floor = max_cognitive_threshold / 2;
    let near_cyclomatic_threshold = crap_only
        && cyclomatic > 0
        && cyclomatic >= max_cyclomatic_threshold.saturating_sub(SECONDARY_REFACTOR_BAND)
        && cognitive >= cognitive_floor;
    let is_template = name == "<template>";
    if !crap_only || crap_only_needs_complexity_reduction || near_cyclomatic_threshold {
        let (description, note) = if is_template {
            (
                format!(
                    "Refactor `{name}` to reduce template complexity (simplify control flow and bindings)"
                ),
                "Consider splitting complex template branches into smaller components or simpler bindings",
            )
        } else {
            (
                format!(
                    "Refactor `{name}` to reduce complexity (extract helper functions, simplify branching)"
                ),
                "Consider splitting into smaller functions with single responsibilities",
            )
        };
        actions.push(serde_json::json!({
            "type": "refactor-function",
            "auto_fixable": false,
            "description": description,
            "note": note,
        }));
    }

    if !opts.omit_suppress_line {
        if is_template
            && Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        {
            actions.push(serde_json::json!({
                "type": "suppress-file",
                "auto_fixable": false,
                "description": "Suppress with an HTML comment at the top of the template",
                "comment": "<!-- fallow-ignore-file complexity -->",
                "placement": "top-of-template",
            }));
        } else if is_template {
            actions.push(serde_json::json!({
                "type": "suppress-line",
                "auto_fixable": false,
                "description": "Suppress with an inline comment above the Angular decorator",
                "comment": "// fallow-ignore-next-line complexity",
                "placement": "above-angular-decorator",
            }));
        } else {
            actions.push(serde_json::json!({
                "type": "suppress-line",
                "auto_fixable": false,
                "description": "Suppress with an inline comment above the function declaration",
                "comment": "// fallow-ignore-next-line complexity",
                "placement": "above-function-declaration",
            }));
        }
    }

    serde_json::Value::Array(actions)
}

/// Build the coverage-leaning action for a CRAP-contributing finding.
///
/// Returns `None` when even 100% coverage could not bring the function below
/// the configured CRAP threshold. In that case the primary action becomes
/// `refactor-function`, which the caller emits separately.
fn build_crap_coverage_action(
    name: &str,
    tier: Option<&str>,
    full_coverage_can_clear_crap: bool,
) -> Option<serde_json::Value> {
    if !full_coverage_can_clear_crap {
        return None;
    }

    match tier {
        // Partial coverage: the file already has some test path. Pivot
        // the action description from "add tests" to "increase coverage"
        // so agents add targeted assertions for uncovered branches
        // instead of scaffolding new tests from scratch.
        Some("partial" | "high") => Some(serde_json::json!({
            "type": "increase-coverage",
            "auto_fixable": false,
            "description": format!("Increase test coverage for `{name}` (file is reachable from existing tests; add targeted assertions for uncovered branches)"),
            "note": "CRAP = CC^2 * (1 - cov/100)^3 + CC; targeted branch coverage is more efficient than scaffolding new test files when the file already has coverage",
        })),
        // None / unknown tier: keep the original "add-tests" message.
        _ => Some(serde_json::json!({
            "type": "add-tests",
            "auto_fixable": false,
            "description": format!("Add test coverage for `{name}` to lower its CRAP score (coverage reduces risk even without refactoring)"),
            "note": "CRAP = CC^2 * (1 - cov/100)^3 + CC; higher coverage is the fastest way to bring CRAP under threshold",
        })),
    }
}

/// Build the `actions` array for a single hotspot entry.
fn build_hotspot_actions(item: &serde_json::Value) -> serde_json::Value {
    let path = item
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("file");

    let mut actions = vec![
        serde_json::json!({
            "type": "refactor-file",
            "auto_fixable": false,
            "description": format!("Refactor `{path}`, high complexity combined with frequent changes makes this a maintenance risk"),
            "note": "Prioritize extracting complex functions, adding tests, or splitting the module",
        }),
        serde_json::json!({
            "type": "add-tests",
            "auto_fixable": false,
            "description": format!("Add test coverage for `{path}` to reduce change risk"),
            "note": "Frequently changed complex files benefit most from comprehensive test coverage",
        }),
    ];

    if let Some(ownership) = item.get("ownership") {
        // Bus factor of 1 is the canonical "single point of failure" signal.
        if ownership
            .get("bus_factor")
            .and_then(serde_json::Value::as_u64)
            == Some(1)
        {
            let top = ownership.get("top_contributor");
            let owner = top
                .and_then(|t| t.get("identifier"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("the sole contributor");
            // Soften the note for files with very few commits — calling a
            // 3-commit file a "knowledge loss risk" reads as catastrophizing
            // for solo maintainers and small teams. Keep the action so
            // agents still see the signal, but soften the framing.
            let commits = top
                .and_then(|t| t.get("commits"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            // File-specific note: name the candidate reviewers from the
            // `suggested_reviewers` array when any exist, fall back to
            // softened framing for low-commit files, and otherwise omit
            // the note entirely (the description already carries the
            // actionable ask; adding generic boilerplate wastes tokens).
            let suggested: Vec<String> = ownership
                .get("suggested_reviewers")
                .and_then(serde_json::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|r| {
                            r.get("identifier")
                                .and_then(serde_json::Value::as_str)
                                .map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut low_bus_action = serde_json::json!({
                "type": "low-bus-factor",
                "auto_fixable": false,
                "description": format!(
                    "{owner} is the sole recent contributor to `{path}`; adding a second reviewer reduces knowledge-loss risk"
                ),
            });
            if !suggested.is_empty() {
                let list = suggested
                    .iter()
                    .map(|s| format!("@{s}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                low_bus_action["note"] =
                    serde_json::Value::String(format!("Candidate reviewers: {list}"));
            } else if commits < 5 {
                low_bus_action["note"] = serde_json::Value::String(
                    "Single recent contributor on a low-commit file. Consider a pair review for major changes."
                        .to_string(),
                );
            }
            // else: omit `note` entirely — description already carries the ask.
            actions.push(low_bus_action);
        }

        // Unowned-hotspot: file matches no CODEOWNERS rule. Skip when null
        // (no CODEOWNERS file discovered).
        if ownership
            .get("unowned")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            actions.push(serde_json::json!({
                "type": "unowned-hotspot",
                "auto_fixable": false,
                "description": format!("Add a CODEOWNERS entry for `{path}`"),
                "note": "Frequently-changed files without declared owners create review bottlenecks",
                "suggested_pattern": suggest_codeowners_pattern(path),
                "heuristic": "directory-deepest",
            }));
        }

        // Drift: original author no longer maintains; add a notice action so
        // agents can route the next change to the new top contributor.
        if ownership.get("drift").and_then(serde_json::Value::as_bool) == Some(true) {
            let reason = ownership
                .get("drift_reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("ownership has shifted from the original author");
            actions.push(serde_json::json!({
                "type": "ownership-drift",
                "auto_fixable": false,
                "description": format!("Update CODEOWNERS for `{path}`: {reason}"),
                "note": "Drift suggests the declared or original owner is no longer the right reviewer",
            }));
        }
    }

    serde_json::Value::Array(actions)
}

/// Suggest a CODEOWNERS pattern for an unowned hotspot.
///
/// Picks the deepest directory containing the file
/// (e.g. `src/api/users/handlers.ts` -> `/src/api/users/`) so agents can
/// paste a tightly-scoped default. Earlier versions used the first two
/// directory levels but that catches too many siblings in monorepos
/// (`/src/api/` could span 200 files across 8 sub-domains). The deepest
/// directory keeps the suggestion reviewable while still being a directory
/// pattern rather than a per-file rule.
///
/// The action emits this alongside `"heuristic": "directory-deepest"` so
/// consumers can branch on the strategy if it evolves.
fn suggest_codeowners_pattern(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let mut components: Vec<&str> = trimmed.split('/').collect();
    components.pop(); // drop the file itself
    if components.is_empty() {
        return format!("/{trimmed}");
    }
    format!("/{}/", components.join("/"))
}

/// Build the `actions` array for a single refactoring target.
fn build_refactoring_target_actions(item: &serde_json::Value) -> serde_json::Value {
    let recommendation = item
        .get("recommendation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Apply the recommended refactoring");

    let category = item
        .get("category")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("refactoring");

    let mut actions = vec![serde_json::json!({
        "type": "apply-refactoring",
        "auto_fixable": false,
        "description": recommendation,
        "category": category,
    })];

    // Targets with evidence linking to specific functions get a suppress action
    if item.get("evidence").is_some() {
        actions.push(serde_json::json!({
            "type": "suppress-line",
            "auto_fixable": false,
            "description": "Suppress the underlying complexity finding",
            "comment": "// fallow-ignore-next-line complexity",
        }));
    }

    serde_json::Value::Array(actions)
}

// ── Duplication action injection ────────────────────────────────

/// Inject `actions` arrays into clone families/groups in a duplication JSON output.
///
/// Walks `clone_families` and `clone_groups` arrays, appending
/// machine-actionable fix and config hints to each item.
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) needed — used by audit.rs via re-export, but not part of public API"
)]
pub(crate) fn inject_dupes_actions(output: &mut serde_json::Value) {
    let Some(map) = output.as_object_mut() else {
        return;
    };

    // Clone families: extract shared module/function. Each family also
    // carries a nested `groups[]` array of `CloneGroup` items that must
    // get their own `actions` field; the committed schema marks `actions`
    // as required on every `CloneGroup`, but without this nested walk the
    // inner groups silently shipped without it and JSON Schema strict
    // consumers saw a `required 'actions' is a required property`
    // violation on every nested group (issue #393).
    if let Some(families) = map.get_mut("clone_families").and_then(|v| v.as_array_mut()) {
        for item in families {
            let actions = build_clone_family_actions(item);
            if let serde_json::Value::Object(obj) = item {
                if let Some(serde_json::Value::Array(groups)) = obj.get_mut("groups") {
                    for inner in groups {
                        let inner_actions = build_clone_group_actions(inner);
                        if let serde_json::Value::Object(inner_obj) = inner {
                            inner_obj.insert("actions".to_string(), inner_actions);
                        }
                    }
                }
                obj.insert("actions".to_string(), actions);
            }
        }
    }

    // Clone groups: extract shared code
    if let Some(groups) = map.get_mut("clone_groups").and_then(|v| v.as_array_mut()) {
        for item in groups {
            let actions = build_clone_group_actions(item);
            if let serde_json::Value::Object(obj) = item {
                obj.insert("actions".to_string(), actions);
            }
        }
    }
}

/// Build the `actions` array for a single clone family.
fn build_clone_family_actions(item: &serde_json::Value) -> serde_json::Value {
    let group_count = item
        .get("groups")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);

    let total_lines = item
        .get("total_duplicated_lines")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let mut actions = vec![serde_json::json!({
        "type": "extract-shared",
        "auto_fixable": false,
        "description": format!(
            "Extract {group_count} duplicated code block{} ({total_lines} lines) into a shared module",
            if group_count == 1 { "" } else { "s" }
        ),
        "note": "These clone groups share the same files, indicating a structural relationship; refactor together",
    })];

    // Include any refactoring suggestions from the family
    if let Some(suggestions) = item.get("suggestions").and_then(|v| v.as_array()) {
        for suggestion in suggestions {
            if let Some(desc) = suggestion
                .get("description")
                .and_then(serde_json::Value::as_str)
            {
                actions.push(serde_json::json!({
                    "type": "apply-suggestion",
                    "auto_fixable": false,
                    "description": desc,
                }));
            }
        }
    }

    actions.push(serde_json::json!({
        "type": "suppress-line",
        "auto_fixable": false,
        "description": "Suppress with an inline comment above the duplicated code",
        "comment": "// fallow-ignore-next-line code-duplication",
    }));

    serde_json::Value::Array(actions)
}

/// Build the `actions` array for a single clone group.
fn build_clone_group_actions(item: &serde_json::Value) -> serde_json::Value {
    let instance_count = item
        .get("instances")
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len);

    let line_count = item
        .get("line_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let actions = vec![
        serde_json::json!({
            "type": "extract-shared",
            "auto_fixable": false,
            "description": format!(
                "Extract duplicated code ({line_count} lines, {instance_count} instance{}) into a shared function",
                if instance_count == 1 { "" } else { "s" }
            ),
        }),
        serde_json::json!({
            "type": "suppress-line",
            "auto_fixable": false,
            "description": "Suppress with an inline comment above the duplicated code",
            "comment": "// fallow-ignore-next-line code-duplication",
        }),
    ];

    serde_json::Value::Array(actions)
}

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
    action_opts: HealthActionOptions,
) -> Result<serde_json::Value, serde_json::Error> {
    let envelope = HealthOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(elapsed.as_millis() as u64),
        report: report.clone(),
        grouped_by: None,
        groups: None,
        meta: None,
    };
    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut output, &root_prefix);
    inject_health_actions(&mut output, action_opts);
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
    action_opts: HealthActionOptions,
) -> ExitCode {
    match build_health_json(report, root, elapsed, explain, action_opts) {
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
///   `vital_signs`, `health_score`, `findings`, `file_scores`, `hotspots`,
///   `large_functions`, `targets`, plus `key`, `owners` (section mode), and
///   the per-group `files_analyzed` / `functions_above_threshold` counts.
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
    action_opts: HealthActionOptions,
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
    };
    let mut output = serde_json::to_value(&envelope)?;
    strip_root_prefix(&mut output, &root_prefix);
    inject_health_actions(&mut output, action_opts);

    let group_values: Vec<serde_json::Value> = grouping
        .groups
        .iter()
        .map(|g| {
            let mut value = serde_json::to_value(g)?;
            strip_root_prefix(&mut value, &root_prefix);
            inject_health_actions(&mut value, action_opts);
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
    action_opts: HealthActionOptions,
) -> ExitCode {
    match build_grouped_health_json(report, grouping, root, elapsed, explain, action_opts) {
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
        report: report.clone(),
        grouped_by: None,
        total_issues: None,
        groups: None,
        meta: None,
    };
    let mut output = serde_json::to_value(&envelope)?;
    let root_prefix = format!("{}/", root.display());
    strip_root_prefix(&mut output, &root_prefix);
    inject_dupes_actions(&mut output);

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
        report: report.clone(),
        grouped_by: Some(group_by_mode_from_label(grouping.mode)),
        total_issues: Some(report.clone_groups.len()),
        // Per-group buckets are serialized separately below so each carries
        // its own path-stripping + actions injection; splice them in via the
        // `Value` post-pass.
        groups: None,
        meta: None,
    };
    let mut output = serde_json::to_value(&envelope)?;
    strip_root_prefix(&mut output, &root_prefix);
    inject_dupes_actions(&mut output);

    let group_values: Vec<serde_json::Value> = grouping
        .groups
        .iter()
        .map(|g| {
            let mut value = serde_json::to_value(g)?;
            strip_root_prefix(&mut value, &root_prefix);
            inject_dupes_actions(&mut value);
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
        };
        let mut output = serde_json::to_value(&envelope).expect("should serialize health envelope");
        strip_root_prefix(&mut output, "/project/");
        inject_health_actions(&mut output, HealthActionOptions::default());

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
        results.duplicate_exports.push(DuplicateExport {
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
        });
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
        results.duplicate_exports.push(DuplicateExport {
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
        });
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
        results.duplicate_exports.push(DuplicateExport {
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
        });

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
        results.duplicate_exports.push(DuplicateExport {
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
        });

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
        results.duplicate_exports.push(DuplicateExport {
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
        });

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

    #[test]
    fn health_finding_has_actions() {
        let mut output = serde_json::json!({
            "findings": [{
                "path": "src/utils.ts",
                "name": "processData",
                "line": 10,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 150,
                "exceeded": "both"
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["findings"][0]["actions"].as_array().unwrap();
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

    #[test]
    fn refactoring_target_has_actions() {
        let mut output = serde_json::json!({
            "targets": [{
                "path": "src/big-module.ts",
                "priority": 85.0,
                "efficiency": 42.5,
                "recommendation": "Split module: 12 exports, 4 unused",
                "category": "split_high_impact",
                "effort": "medium",
                "confidence": "high",
                "evidence": { "unused_exports": 4 }
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["targets"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["type"], "apply-refactoring");
        assert_eq!(
            actions[0]["description"],
            "Split module: 12 exports, 4 unused"
        );
        assert_eq!(actions[0]["category"], "split_high_impact");
        // Target with evidence gets suppress action
        assert_eq!(actions[1]["type"], "suppress-line");
    }

    #[test]
    fn refactoring_target_without_evidence_has_no_suppress() {
        let mut output = serde_json::json!({
            "targets": [{
                "path": "src/simple.ts",
                "priority": 30.0,
                "efficiency": 15.0,
                "recommendation": "Consider extracting helper functions",
                "category": "extract_complex_functions",
                "effort": "small",
                "confidence": "medium"
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["targets"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["type"], "apply-refactoring");
    }

    #[test]
    fn health_empty_findings_no_actions() {
        let mut output = serde_json::json!({
            "findings": [],
            "targets": []
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        assert!(output["findings"].as_array().unwrap().is_empty());
        assert!(output["targets"].as_array().unwrap().is_empty());
    }

    #[test]
    fn hotspot_has_actions() {
        let mut output = serde_json::json!({
            "hotspots": [{
                "path": "src/utils.ts",
                "complexity_score": 45.0,
                "churn_score": 12,
                "hotspot_score": 540.0
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["hotspots"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["type"], "refactor-file");
        assert!(
            actions[0]["description"]
                .as_str()
                .unwrap()
                .contains("src/utils.ts")
        );
        assert_eq!(actions[1]["type"], "add-tests");
    }

    #[test]
    fn hotspot_low_bus_factor_emits_action() {
        let mut output = serde_json::json!({
            "hotspots": [{
                "path": "src/api.ts",
                "ownership": {
                    "bus_factor": 1,
                    "contributor_count": 1,
                    "top_contributor": {"identifier": "alice@x", "share": 1.0, "stale_days": 5, "commits": 30},
                    "unowned": null,
                    "drift": false,
                }
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["hotspots"][0]["actions"].as_array().unwrap();
        assert!(
            actions
                .iter()
                .filter_map(|a| a["type"].as_str())
                .any(|t| t == "low-bus-factor"),
            "low-bus-factor action should be present",
        );
        let bus = actions
            .iter()
            .find(|a| a["type"] == "low-bus-factor")
            .unwrap();
        assert!(bus["description"].as_str().unwrap().contains("alice@x"));
    }

    #[test]
    fn hotspot_unowned_emits_action_with_pattern() {
        let mut output = serde_json::json!({
            "hotspots": [{
                "path": "src/api/users.ts",
                "ownership": {
                    "bus_factor": 2,
                    "contributor_count": 4,
                    "top_contributor": {"identifier": "alice@x", "share": 0.5, "stale_days": 5, "commits": 10},
                    "unowned": true,
                    "drift": false,
                }
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["hotspots"][0]["actions"].as_array().unwrap();
        let unowned = actions
            .iter()
            .find(|a| a["type"] == "unowned-hotspot")
            .expect("unowned-hotspot action should be present");
        // Deepest directory containing the file -> /src/api/
        // (file `users.ts` is at depth 2, so the deepest dir is `/src/api/`).
        assert_eq!(unowned["suggested_pattern"], "/src/api/");
        assert_eq!(unowned["heuristic"], "directory-deepest");
    }

    #[test]
    fn hotspot_unowned_skipped_when_codeowners_missing() {
        let mut output = serde_json::json!({
            "hotspots": [{
                "path": "src/api.ts",
                "ownership": {
                    "bus_factor": 2,
                    "contributor_count": 4,
                    "top_contributor": {"identifier": "alice@x", "share": 0.5, "stale_days": 5, "commits": 10},
                    "unowned": null,
                    "drift": false,
                }
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["hotspots"][0]["actions"].as_array().unwrap();
        assert!(
            !actions.iter().any(|a| a["type"] == "unowned-hotspot"),
            "unowned action must not fire when CODEOWNERS file is absent"
        );
    }

    #[test]
    fn hotspot_drift_emits_action() {
        let mut output = serde_json::json!({
            "hotspots": [{
                "path": "src/old.ts",
                "ownership": {
                    "bus_factor": 1,
                    "contributor_count": 2,
                    "top_contributor": {"identifier": "bob@x", "share": 0.9, "stale_days": 1, "commits": 18},
                    "unowned": null,
                    "drift": true,
                    "drift_reason": "original author alice@x has 5% share",
                }
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let actions = output["hotspots"][0]["actions"].as_array().unwrap();
        let drift = actions
            .iter()
            .find(|a| a["type"] == "ownership-drift")
            .expect("ownership-drift action should be present");
        assert!(drift["description"].as_str().unwrap().contains("alice@x"));
    }

    // ── suggest_codeowners_pattern ─────────────────────────────────

    #[test]
    fn codeowners_pattern_uses_deepest_directory() {
        // Deepest dir keeps the suggestion tightly-scoped; the prior
        // "first two levels" heuristic over-generalized in monorepos.
        assert_eq!(
            suggest_codeowners_pattern("src/api/users/handlers.ts"),
            "/src/api/users/"
        );
    }

    #[test]
    fn codeowners_pattern_for_root_file() {
        assert_eq!(suggest_codeowners_pattern("README.md"), "/README.md");
    }

    #[test]
    fn codeowners_pattern_normalizes_backslashes() {
        assert_eq!(
            suggest_codeowners_pattern("src\\api\\users.ts"),
            "/src/api/"
        );
    }

    #[test]
    fn codeowners_pattern_two_level_path() {
        assert_eq!(suggest_codeowners_pattern("src/foo.ts"), "/src/");
    }

    #[test]
    fn health_finding_suppress_has_placement() {
        let mut output = serde_json::json!({
            "findings": [{
                "path": "src/utils.ts",
                "name": "processData",
                "line": 10,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 150,
                "exceeded": "both"
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let suppress = &output["findings"][0]["actions"][1];
        assert_eq!(suppress["placement"], "above-function-declaration");
    }

    #[test]
    fn html_template_health_finding_uses_html_suppression() {
        let mut output = serde_json::json!({
            "findings": [{
                "path": "src/app.component.html",
                "name": "<template>",
                "line": 1,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 40,
                "exceeded": "both"
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let suppress = &output["findings"][0]["actions"][1];
        assert_eq!(suppress["type"], "suppress-file");
        assert_eq!(
            suppress["comment"],
            "<!-- fallow-ignore-file complexity -->"
        );
        assert_eq!(suppress["placement"], "top-of-template");
    }

    #[test]
    fn inline_template_health_finding_uses_decorator_suppression() {
        let mut output = serde_json::json!({
            "findings": [{
                "path": "src/app.component.ts",
                "name": "<template>",
                "line": 5,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 30,
                "line_count": 40,
                "exceeded": "both"
            }]
        });

        inject_health_actions(&mut output, HealthActionOptions::default());

        let refactor = &output["findings"][0]["actions"][0];
        assert_eq!(refactor["type"], "refactor-function");
        assert!(
            refactor["description"]
                .as_str()
                .unwrap()
                .contains("template complexity")
        );
        let suppress = &output["findings"][0]["actions"][1];
        assert_eq!(suppress["type"], "suppress-line");
        assert_eq!(
            suppress["description"],
            "Suppress with an inline comment above the Angular decorator"
        );
        assert_eq!(suppress["placement"], "above-angular-decorator");
    }

    // ── Duplication actions injection ─────────────────────────────

    #[test]
    fn clone_family_has_actions() {
        let mut output = serde_json::json!({
            "clone_families": [{
                "files": ["src/a.ts", "src/b.ts"],
                "groups": [
                    { "instances": [{"file": "src/a.ts"}, {"file": "src/b.ts"}], "token_count": 100, "line_count": 20 }
                ],
                "total_duplicated_lines": 20,
                "total_duplicated_tokens": 100,
                "suggestions": [
                    { "kind": "ExtractFunction", "description": "Extract shared validation logic", "estimated_savings": 15 }
                ]
            }]
        });

        inject_dupes_actions(&mut output);

        let actions = output["clone_families"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0]["type"], "extract-shared");
        assert_eq!(actions[0]["auto_fixable"], false);
        assert!(
            actions[0]["description"]
                .as_str()
                .unwrap()
                .contains("20 lines")
        );
        // Suggestion forwarded as action
        assert_eq!(actions[1]["type"], "apply-suggestion");
        assert!(
            actions[1]["description"]
                .as_str()
                .unwrap()
                .contains("validation logic")
        );
        // Suppress action
        assert_eq!(actions[2]["type"], "suppress-line");
        assert_eq!(
            actions[2]["comment"],
            "// fallow-ignore-next-line code-duplication"
        );
    }

    #[test]
    fn clone_group_has_actions() {
        let mut output = serde_json::json!({
            "clone_groups": [{
                "instances": [
                    {"file": "src/a.ts", "start_line": 1, "end_line": 10},
                    {"file": "src/b.ts", "start_line": 5, "end_line": 14}
                ],
                "token_count": 50,
                "line_count": 10
            }]
        });

        inject_dupes_actions(&mut output);

        let actions = output["clone_groups"][0]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["type"], "extract-shared");
        assert!(
            actions[0]["description"]
                .as_str()
                .unwrap()
                .contains("10 lines")
        );
        assert!(
            actions[0]["description"]
                .as_str()
                .unwrap()
                .contains("2 instances")
        );
        assert_eq!(actions[1]["type"], "suppress-line");
    }

    #[test]
    fn dupes_empty_results_no_actions() {
        let mut output = serde_json::json!({
            "clone_families": [],
            "clone_groups": []
        });

        inject_dupes_actions(&mut output);

        assert!(output["clone_families"].as_array().unwrap().is_empty());
        assert!(output["clone_groups"].as_array().unwrap().is_empty());
    }

    /// Regression for issue #393: `inject_dupes_actions` must also walk
    /// every `clone_families[i].groups[j]` (each a `CloneGroup`) and inject
    /// `actions` on the inner item. The schema marks `actions` as required
    /// on every `CloneGroup`; without the nested walk, JSON Schema strict
    /// consumers saw a `required 'actions' is a required property`
    /// violation on every nested group.
    #[test]
    fn clone_family_nested_groups_have_actions() {
        let mut output = serde_json::json!({
            "clone_families": [{
                "files": ["src/a.ts", "src/b.ts"],
                "groups": [
                    {
                        "instances": [
                            {"file": "src/a.ts", "start_line": 1, "end_line": 10},
                            {"file": "src/b.ts", "start_line": 5, "end_line": 14}
                        ],
                        "token_count": 50,
                        "line_count": 10
                    },
                    {
                        "instances": [
                            {"file": "src/a.ts", "start_line": 20, "end_line": 30},
                            {"file": "src/b.ts", "start_line": 25, "end_line": 35}
                        ],
                        "token_count": 60,
                        "line_count": 11
                    }
                ],
                "total_duplicated_lines": 21,
                "total_duplicated_tokens": 110
            }]
        });

        inject_dupes_actions(&mut output);

        let family = &output["clone_families"][0];
        assert!(
            family.get("actions").and_then(|v| v.as_array()).is_some(),
            "family carries top-level actions"
        );
        let groups = family["groups"].as_array().expect("groups array");
        for (idx, group) in groups.iter().enumerate() {
            let actions = group["actions"]
                .as_array()
                .unwrap_or_else(|| panic!("inner group {idx} missing actions"));
            assert!(
                !actions.is_empty(),
                "inner group {idx} has at least one action"
            );
            assert_eq!(actions[0]["type"], "extract-shared");
            assert_eq!(actions.last().unwrap()["type"], "suppress-line");
        }
    }

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

    fn crap_only_finding_envelope_with_max_crap(
        coverage_tier: Option<&str>,
        cyclomatic: u16,
        cognitive: u16,
        max_cyclomatic_threshold: u16,
        max_cognitive_threshold: u16,
        max_crap_threshold: f64,
    ) -> serde_json::Value {
        let mut finding = serde_json::json!({
            "path": "src/risk.ts",
            "name": "computeScore",
            "line": 12,
            "col": 0,
            "cyclomatic": cyclomatic,
            "cognitive": cognitive,
            "line_count": 40,
            "exceeded": "crap",
            "crap": 35.5,
        });
        if let Some(tier) = coverage_tier {
            finding["coverage_tier"] = serde_json::Value::String(tier.to_owned());
        }
        serde_json::json!({
            "findings": [finding],
            "summary": {
                "max_cyclomatic_threshold": max_cyclomatic_threshold,
                "max_cognitive_threshold": max_cognitive_threshold,
                "max_crap_threshold": max_crap_threshold,
            },
        })
    }

    #[test]
    fn crap_only_tier_none_emits_add_tests() {
        let mut output = crap_only_finding_envelope(Some("none"), 6, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope(Some("partial"), 6, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope(Some("high"), 20, 30);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output =
            crap_only_finding_envelope_with_max_crap(Some("high"), 35, 12, 50, 15, 30.0);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope(Some("none"), 16, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope(Some("none"), 6, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope_with_cognitive(Some("none"), 17, 2, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = crap_only_finding_envelope_with_cognitive(Some("none"), 16, 10, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
        let mut output = serde_json::json!({
            "findings": [{
                "path": "src/cyclo.ts",
                "name": "branchy",
                "line": 5,
                "col": 0,
                "cyclomatic": 25,
                "cognitive": 10,
                "line_count": 80,
                "exceeded": "cyclomatic",
            }],
            "summary": { "max_cyclomatic_threshold": 20 },
        });
        inject_health_actions(&mut output, HealthActionOptions::default());
        let actions = output["findings"][0]["actions"].as_array().unwrap();
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
        let mut output = crap_only_finding_envelope(Some("none"), 6, 20);
        inject_health_actions(
            &mut output,
            HealthActionOptions {
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
        let mut output = crap_only_finding_envelope(Some("none"), 6, 20);
        inject_health_actions(
            &mut output,
            HealthActionOptions {
                omit_suppress_line: true,
                omit_reason: Some("config-disabled"),
            },
        );
        assert_eq!(output["actions_meta"]["reason"], "config-disabled");
    }

    #[test]
    fn suppress_line_emitted_by_default() {
        let mut output = crap_only_finding_envelope(Some("none"), 6, 20);
        inject_health_actions(&mut output, HealthActionOptions::default());
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
            let mut output = serde_json::json!({
                "findings": [finding],
                "summary": { "max_cyclomatic_threshold": max },
            });
            inject_health_actions(&mut output, HealthActionOptions::default());
            for action in output["findings"][0]["actions"].as_array().unwrap() {
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
}
