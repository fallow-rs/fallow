use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{RulesConfig, Severity};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{
    AnalysisResults, BoundaryViolation, CircularDependency, DuplicateExport, PrivateTypeLeak,
    StaleSuppression, TestOnlyDependency, TypeOnlyDependency, UnlistedDependency, UnresolvedImport,
    UnusedDependency, UnusedExport, UnusedFile, UnusedMember,
};
use rustc_hash::FxHashMap;

use super::ci::{fingerprint, severity};
use super::grouping::{self, OwnershipResolver};
use super::{emit_json, relative_uri};
use crate::explain;

/// Intermediate fields extracted from an issue for SARIF result construction.
struct SarifFields {
    rule_id: &'static str,
    level: &'static str,
    message: String,
    uri: String,
    region: Option<(u32, u32)>,
    source_path: Option<PathBuf>,
    properties: Option<serde_json::Value>,
}

#[derive(Default)]
struct SourceSnippetCache {
    files: FxHashMap<PathBuf, Vec<String>>,
}

impl SourceSnippetCache {
    fn line(&mut self, path: &Path, line: u32) -> Option<String> {
        if line == 0 {
            return None;
        }
        if !self.files.contains_key(path) {
            let lines = std::fs::read_to_string(path)
                .ok()
                .map(|source| source.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            self.files.insert(path.to_path_buf(), lines);
        }
        self.files
            .get(path)
            .and_then(|lines| lines.get(line.saturating_sub(1) as usize))
            .cloned()
    }
}

fn severity_to_sarif_level(s: Severity) -> &'static str {
    severity::sarif_level(s)
}

fn configured_sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Error | Severity::Warn => severity_to_sarif_level(s),
        Severity::Off => "none",
    }
}

/// Build a single SARIF result object.
///
/// When `region` is `Some((line, col))`, a `region` block with 1-based
/// `startLine` and `startColumn` is included in the physical location.
fn sarif_result(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
) -> serde_json::Value {
    sarif_result_with_snippet(rule_id, level, message, uri, region, None)
}

fn sarif_result_with_snippet(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
    snippet: Option<&str>,
) -> serde_json::Value {
    let mut physical_location = serde_json::json!({
        "artifactLocation": { "uri": uri }
    });
    if let Some((line, col)) = region {
        physical_location["region"] = serde_json::json!({
            "startLine": line,
            "startColumn": col
        });
    }
    let line = region.map_or_else(String::new, |(line, _)| line.to_string());
    let col = region.map_or_else(String::new, |(_, col)| col.to_string());
    let normalized_snippet = snippet
        .map(fingerprint::normalize_snippet)
        .filter(|snippet| !snippet.is_empty());
    let partial_fingerprint = normalized_snippet.as_ref().map_or_else(
        || fingerprint::fingerprint_hash(&[rule_id, uri, &line, &col]),
        |snippet| fingerprint::finding_fingerprint(rule_id, uri, snippet),
    );
    let partial_fingerprint_ghas = partial_fingerprint.clone();
    serde_json::json!({
        "ruleId": rule_id,
        "level": level,
        "message": { "text": message },
        "locations": [{ "physicalLocation": physical_location }],
        "partialFingerprints": {
            fingerprint::FINGERPRINT_KEY: partial_fingerprint,
            fingerprint::GHAS_FINGERPRINT_KEY: partial_fingerprint_ghas
        }
    })
}

/// Append SARIF results for a slice of items using a closure to extract fields.
fn push_sarif_results<T>(
    sarif_results: &mut Vec<serde_json::Value>,
    items: &[T],
    snippets: &mut SourceSnippetCache,
    mut extract: impl FnMut(&T) -> SarifFields,
) {
    for item in items {
        let fields = extract(item);
        let source_snippet = fields
            .source_path
            .as_deref()
            .zip(fields.region)
            .and_then(|(path, (line, _))| snippets.line(path, line));
        let mut result = sarif_result_with_snippet(
            fields.rule_id,
            fields.level,
            &fields.message,
            &fields.uri,
            fields.region,
            source_snippet.as_deref(),
        );
        if let Some(props) = fields.properties {
            result["properties"] = props;
        }
        sarif_results.push(result);
    }
}

/// Build a SARIF rule definition with optional `fullDescription` and `helpUri`
/// sourced from the centralized explain module.
fn sarif_rule(id: &str, fallback_short: &str, level: &str) -> serde_json::Value {
    explain::rule_by_id(id).map_or_else(
        || {
            serde_json::json!({
                "id": id,
                "shortDescription": { "text": fallback_short },
                "defaultConfiguration": { "level": level }
            })
        },
        |def| {
            serde_json::json!({
                "id": id,
                "shortDescription": { "text": def.short },
                "fullDescription": { "text": def.full },
                "helpUri": explain::rule_docs_url(def),
                "defaultConfiguration": { "level": level }
            })
        },
    )
}

/// Extract SARIF fields for an unused export or type export.
fn sarif_export_fields(
    export: &UnusedExport,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    kind: &str,
    re_kind: &str,
) -> SarifFields {
    let label = if export.is_re_export { re_kind } else { kind };
    SarifFields {
        rule_id,
        level,
        message: format!(
            "{} '{}' is never imported by other modules",
            label, export.export_name
        ),
        uri: relative_uri(&export.path, root),
        region: Some((export.line, export.col + 1)),
        source_path: Some(export.path.clone()),
        properties: if export.is_re_export {
            Some(serde_json::json!({ "is_re_export": true }))
        } else {
            None
        },
    }
}

fn sarif_private_type_leak_fields(
    leak: &PrivateTypeLeak,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/private-type-leak",
        level,
        message: format!(
            "Export '{}' references private type '{}'",
            leak.export_name, leak.type_name
        ),
        uri: relative_uri(&leak.path, root),
        region: Some((leak.line, leak.col + 1)),
        source_path: Some(leak.path.clone()),
        properties: None,
    }
}

/// Extract SARIF fields for an unused dependency.
fn sarif_dep_fields(
    dep: &UnusedDependency,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    section: &str,
) -> SarifFields {
    let workspace_context = if dep.used_in_workspaces.is_empty() {
        String::new()
    } else {
        let workspaces = dep
            .used_in_workspaces
            .iter()
            .map(|path| relative_uri(path, root))
            .collect::<Vec<_>>()
            .join(", ");
        format!("; imported in other workspaces: {workspaces}")
    };
    SarifFields {
        rule_id,
        level,
        message: format!(
            "Package '{}' is in {} but never imported{}",
            dep.package_name, section, workspace_context
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, 1))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

/// Extract SARIF fields for an unused enum or class member.
fn sarif_member_fields(
    member: &UnusedMember,
    root: &Path,
    rule_id: &'static str,
    level: &'static str,
    kind: &str,
) -> SarifFields {
    SarifFields {
        rule_id,
        level,
        message: format!(
            "{} member '{}.{}' is never referenced",
            kind, member.parent_name, member.member_name
        ),
        uri: relative_uri(&member.path, root),
        region: Some((member.line, member.col + 1)),
        source_path: Some(member.path.clone()),
        properties: None,
    }
}

fn sarif_unused_file_fields(file: &UnusedFile, root: &Path, level: &'static str) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unused-file",
        level,
        message: "File is not reachable from any entry point".to_string(),
        uri: relative_uri(&file.path, root),
        region: None,
        source_path: None,
        properties: None,
    }
}

fn sarif_type_only_dep_fields(
    dep: &TypeOnlyDependency,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/type-only-dependency",
        level,
        message: format!(
            "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
            dep.package_name
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, 1))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

fn sarif_test_only_dep_fields(
    dep: &TestOnlyDependency,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/test-only-dependency",
        level,
        message: format!(
            "Package '{}' is only imported by test files (consider moving to devDependencies)",
            dep.package_name
        ),
        uri: relative_uri(&dep.path, root),
        region: if dep.line > 0 {
            Some((dep.line, 1))
        } else {
            None
        },
        source_path: (dep.line > 0).then(|| dep.path.clone()),
        properties: None,
    }
}

fn sarif_unresolved_import_fields(
    import: &UnresolvedImport,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/unresolved-import",
        level,
        message: format!("Import '{}' could not be resolved", import.specifier),
        uri: relative_uri(&import.path, root),
        region: Some((import.line, import.col + 1)),
        source_path: Some(import.path.clone()),
        properties: None,
    }
}

fn sarif_circular_dep_fields(
    cycle: &CircularDependency,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let chain: Vec<String> = cycle.files.iter().map(|p| relative_uri(p, root)).collect();
    let mut display_chain = chain.clone();
    if let Some(first) = chain.first() {
        display_chain.push(first.clone());
    }
    let first_uri = chain.first().map_or_else(String::new, Clone::clone);
    let first_path = cycle.files.first().cloned();
    SarifFields {
        rule_id: "fallow/circular-dependency",
        level,
        message: format!(
            "Circular dependency{}: {}",
            if cycle.is_cross_package {
                " (cross-package)"
            } else {
                ""
            },
            display_chain.join(" \u{2192} ")
        ),
        uri: first_uri,
        region: if cycle.line > 0 {
            Some((cycle.line, cycle.col + 1))
        } else {
            None
        },
        source_path: (cycle.line > 0).then_some(first_path).flatten(),
        properties: None,
    }
}

fn sarif_boundary_violation_fields(
    violation: &BoundaryViolation,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let from_uri = relative_uri(&violation.from_path, root);
    let to_uri = relative_uri(&violation.to_path, root);
    SarifFields {
        rule_id: "fallow/boundary-violation",
        level,
        message: format!(
            "Import from zone '{}' to zone '{}' is not allowed ({})",
            violation.from_zone, violation.to_zone, to_uri,
        ),
        uri: from_uri,
        region: if violation.line > 0 {
            Some((violation.line, violation.col + 1))
        } else {
            None
        },
        source_path: (violation.line > 0).then(|| violation.from_path.clone()),
        properties: None,
    }
}

fn sarif_stale_suppression_fields(
    suppression: &StaleSuppression,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    SarifFields {
        rule_id: "fallow/stale-suppression",
        level,
        message: suppression.description(),
        uri: relative_uri(&suppression.path, root),
        region: Some((suppression.line, suppression.col + 1)),
        source_path: Some(suppression.path.clone()),
        properties: None,
    }
}

fn sarif_unused_catalog_entry_fields(
    entry: &fallow_core::results::UnusedCatalogEntry,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let message = if entry.catalog_name == "default" {
        format!(
            "Catalog entry '{}' is not referenced by any workspace package",
            entry.entry_name
        )
    } else {
        format!(
            "Catalog entry '{}' (catalog '{}') is not referenced by any workspace package",
            entry.entry_name, entry.catalog_name
        )
    };
    SarifFields {
        rule_id: "fallow/unused-catalog-entry",
        level,
        message,
        uri: relative_uri(&entry.path, root),
        region: Some((entry.line, 1)),
        source_path: Some(entry.path.clone()),
        properties: None,
    }
}

/// Unlisted deps fan out to one SARIF result per import site, so they do not
/// fit `push_sarif_results`. Keep the nested-loop shape in its own helper.
fn push_sarif_unlisted_deps(
    sarif_results: &mut Vec<serde_json::Value>,
    deps: &[UnlistedDependency],
    root: &Path,
    level: &'static str,
    snippets: &mut SourceSnippetCache,
) {
    for dep in deps {
        for site in &dep.imported_from {
            let uri = relative_uri(&site.path, root);
            let source_snippet = snippets.line(&site.path, site.line);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/unlisted-dependency",
                level,
                &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                &uri,
                Some((site.line, site.col + 1)),
                source_snippet.as_deref(),
            ));
        }
    }
}

/// Duplicate exports fan out to one SARIF result per location
/// (SARIF 2.1.0 section 3.27.12), so they do not fit `push_sarif_results`.
fn push_sarif_duplicate_exports(
    sarif_results: &mut Vec<serde_json::Value>,
    dups: &[DuplicateExport],
    root: &Path,
    level: &'static str,
    snippets: &mut SourceSnippetCache,
) {
    for dup in dups {
        for loc in &dup.locations {
            let uri = relative_uri(&loc.path, root);
            let source_snippet = snippets.line(&loc.path, loc.line);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/duplicate-export",
                level,
                &format!("Export '{}' appears in multiple modules", dup.export_name),
                &uri,
                Some((loc.line, loc.col + 1)),
                source_snippet.as_deref(),
            ));
        }
    }
}

/// Build the SARIF rules list from the current rules configuration.
fn build_sarif_rules(rules: &RulesConfig) -> Vec<serde_json::Value> {
    [
        (
            "fallow/unused-file",
            "File is not reachable from any entry point",
            rules.unused_files,
        ),
        (
            "fallow/unused-export",
            "Export is never imported",
            rules.unused_exports,
        ),
        (
            "fallow/unused-type",
            "Type export is never imported",
            rules.unused_types,
        ),
        (
            "fallow/private-type-leak",
            "Exported signature references a same-file private type",
            rules.private_type_leaks,
        ),
        (
            "fallow/unused-dependency",
            "Dependency listed but never imported",
            rules.unused_dependencies,
        ),
        (
            "fallow/unused-dev-dependency",
            "Dev dependency listed but never imported",
            rules.unused_dev_dependencies,
        ),
        (
            "fallow/unused-optional-dependency",
            "Optional dependency listed but never imported",
            rules.unused_optional_dependencies,
        ),
        (
            "fallow/type-only-dependency",
            "Production dependency only used via type-only imports",
            rules.type_only_dependencies,
        ),
        (
            "fallow/test-only-dependency",
            "Production dependency only imported by test files",
            rules.test_only_dependencies,
        ),
        (
            "fallow/unused-enum-member",
            "Enum member is never referenced",
            rules.unused_enum_members,
        ),
        (
            "fallow/unused-class-member",
            "Class member is never referenced",
            rules.unused_class_members,
        ),
        (
            "fallow/unresolved-import",
            "Import could not be resolved",
            rules.unresolved_imports,
        ),
        (
            "fallow/unlisted-dependency",
            "Dependency used but not in package.json",
            rules.unlisted_dependencies,
        ),
        (
            "fallow/duplicate-export",
            "Export name appears in multiple modules",
            rules.duplicate_exports,
        ),
        (
            "fallow/circular-dependency",
            "Circular dependency chain detected",
            rules.circular_dependencies,
        ),
        (
            "fallow/boundary-violation",
            "Import crosses an architecture boundary",
            rules.boundary_violation,
        ),
        (
            "fallow/stale-suppression",
            "Suppression comment or tag no longer matches any issue",
            rules.stale_suppressions,
        ),
        (
            "fallow/unused-catalog-entry",
            "pnpm catalog entry not referenced by any workspace package",
            rules.unused_catalog_entries,
        ),
    ]
    .into_iter()
    .map(|(id, description, rule_severity)| {
        sarif_rule(id, description, configured_sarif_level(rule_severity))
    })
    .collect()
}

#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "SARIF builds one flat result list across every analysis family"
)]
pub fn build_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> serde_json::Value {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    push_sarif_results(
        &mut sarif_results,
        &results.unused_files,
        &mut snippets,
        |f| sarif_unused_file_fields(f, root, severity_to_sarif_level(rules.unused_files)),
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_exports,
        &mut snippets,
        |e| {
            sarif_export_fields(
                e,
                root,
                "fallow/unused-export",
                severity_to_sarif_level(rules.unused_exports),
                "Export",
                "Re-export",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_types,
        &mut snippets,
        |e| {
            sarif_export_fields(
                e,
                root,
                "fallow/unused-type",
                severity_to_sarif_level(rules.unused_types),
                "Type export",
                "Type re-export",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.private_type_leaks,
        &mut snippets,
        |e| {
            sarif_private_type_leak_fields(
                e,
                root,
                severity_to_sarif_level(rules.private_type_leaks),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_dependencies,
        &mut snippets,
        |d| {
            sarif_dep_fields(
                d,
                root,
                "fallow/unused-dependency",
                severity_to_sarif_level(rules.unused_dependencies),
                "dependencies",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_dev_dependencies,
        &mut snippets,
        |d| {
            sarif_dep_fields(
                d,
                root,
                "fallow/unused-dev-dependency",
                severity_to_sarif_level(rules.unused_dev_dependencies),
                "devDependencies",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_optional_dependencies,
        &mut snippets,
        |d| {
            sarif_dep_fields(
                d,
                root,
                "fallow/unused-optional-dependency",
                severity_to_sarif_level(rules.unused_optional_dependencies),
                "optionalDependencies",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.type_only_dependencies,
        &mut snippets,
        |d| {
            sarif_type_only_dep_fields(
                d,
                root,
                severity_to_sarif_level(rules.type_only_dependencies),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.test_only_dependencies,
        &mut snippets,
        |d| {
            sarif_test_only_dep_fields(
                d,
                root,
                severity_to_sarif_level(rules.test_only_dependencies),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_enum_members,
        &mut snippets,
        |m| {
            sarif_member_fields(
                m,
                root,
                "fallow/unused-enum-member",
                severity_to_sarif_level(rules.unused_enum_members),
                "Enum",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_class_members,
        &mut snippets,
        |m| {
            sarif_member_fields(
                m,
                root,
                "fallow/unused-class-member",
                severity_to_sarif_level(rules.unused_class_members),
                "Class",
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unresolved_imports,
        &mut snippets,
        |i| {
            sarif_unresolved_import_fields(
                i,
                root,
                severity_to_sarif_level(rules.unresolved_imports),
            )
        },
    );
    if !results.unlisted_dependencies.is_empty() {
        push_sarif_unlisted_deps(
            &mut sarif_results,
            &results.unlisted_dependencies,
            root,
            severity_to_sarif_level(rules.unlisted_dependencies),
            &mut snippets,
        );
    }
    if !results.duplicate_exports.is_empty() {
        push_sarif_duplicate_exports(
            &mut sarif_results,
            &results.duplicate_exports,
            root,
            severity_to_sarif_level(rules.duplicate_exports),
            &mut snippets,
        );
    }
    push_sarif_results(
        &mut sarif_results,
        &results.circular_dependencies,
        &mut snippets,
        |c| {
            sarif_circular_dep_fields(
                c,
                root,
                severity_to_sarif_level(rules.circular_dependencies),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.boundary_violations,
        &mut snippets,
        |v| {
            sarif_boundary_violation_fields(
                v,
                root,
                severity_to_sarif_level(rules.boundary_violation),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.stale_suppressions,
        &mut snippets,
        |s| {
            sarif_stale_suppression_fields(
                s,
                root,
                severity_to_sarif_level(rules.stale_suppressions),
            )
        },
    );
    push_sarif_results(
        &mut sarif_results,
        &results.unused_catalog_entries,
        &mut snippets,
        |e| {
            sarif_unused_catalog_entry_fields(
                e,
                root,
                severity_to_sarif_level(rules.unused_catalog_entries),
            )
        },
    );

    serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": build_sarif_rules(rules)
                }
            },
            "results": sarif_results
        }]
    })
}

pub(super) fn print_sarif(results: &AnalysisResults, root: &Path, rules: &RulesConfig) -> ExitCode {
    let sarif = build_sarif(results, root, rules);
    emit_json(&sarif, "SARIF")
}

/// Print SARIF output with owner properties added to each result.
///
/// Calls `build_sarif` to produce the standard SARIF JSON, then post-processes
/// each result to add `"properties": { "owner": "@team" }` by resolving the
/// artifact location URI through the `OwnershipResolver`.
pub(super) fn print_grouped_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut sarif = build_sarif(results, root, rules);

    // Post-process each result to inject the owner property.
    if let Some(runs) = sarif.get_mut("runs").and_then(|r| r.as_array_mut()) {
        for run in runs {
            if let Some(results) = run.get_mut("results").and_then(|r| r.as_array_mut()) {
                for result in results {
                    let uri = result
                        .pointer("/locations/0/physicalLocation/artifactLocation/uri")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    // Decode percent-encoded brackets before ownership lookup
                    // (SARIF URIs encode `[`/`]` as `%5B`/`%5D`)
                    let decoded = uri.replace("%5B", "[").replace("%5D", "]");
                    let owner =
                        grouping::resolve_owner(Path::new(&decoded), Path::new(""), resolver);
                    let props = result
                        .as_object_mut()
                        .expect("SARIF result should be an object")
                        .entry("properties")
                        .or_insert_with(|| serde_json::json!({}));
                    props
                        .as_object_mut()
                        .expect("properties should be an object")
                        .insert("owner".to_string(), serde_json::Value::String(owner));
                }
            }
        }
    }

    emit_json(&sarif, "SARIF")
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
pub(super) fn print_duplication_sarif(report: &DuplicationReport, root: &Path) -> ExitCode {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    for (i, group) in report.clone_groups.iter().enumerate() {
        for instance in &group.instances {
            let uri = relative_uri(&instance.file, root);
            let source_snippet = snippets.line(&instance.file, instance.start_line as u32);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/code-duplication",
                "warning",
                &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                &uri,
                Some((instance.start_line as u32, (instance.start_col + 1) as u32)),
                source_snippet.as_deref(),
            ));
        }
    }

    let sarif = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": [sarif_rule("fallow/code-duplication", "Duplicated code block", "warning")]
                }
            },
            "results": sarif_results
        }]
    });

    emit_json(&sarif, "SARIF")
}

/// Print SARIF duplication output with a `properties.group` tag on every
/// result.
///
/// Each clone group is attributed to its largest owner (most instances; ties
/// broken alphabetically) via [`super::dupes_grouping::largest_owner`], and
/// every result emitted for that group's instances carries the same
/// `properties.group` value. This mirrors the health SARIF convention
/// (`print_grouped_health_sarif`) so consumers (GitHub Code Scanning, GitLab
/// Code Quality) can partition findings per team / package / directory
/// without re-resolving ownership.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
pub(super) fn print_grouped_duplication_sarif(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    for (i, group) in report.clone_groups.iter().enumerate() {
        // Compute the group's primary owner once. Every result emitted for
        // this group carries the same `properties.group` value (the GROUP'S
        // owner, not the per-instance owner).
        let primary_owner = super::dupes_grouping::largest_owner(group, root, resolver);
        for instance in &group.instances {
            let uri = relative_uri(&instance.file, root);
            let source_snippet = snippets.line(&instance.file, instance.start_line as u32);
            let mut result = sarif_result_with_snippet(
                "fallow/code-duplication",
                "warning",
                &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                &uri,
                Some((instance.start_line as u32, (instance.start_col + 1) as u32)),
                source_snippet.as_deref(),
            );
            let props = result
                .as_object_mut()
                .expect("SARIF result should be an object")
                .entry("properties")
                .or_insert_with(|| serde_json::json!({}));
            props
                .as_object_mut()
                .expect("properties should be an object")
                .insert(
                    "group".to_string(),
                    serde_json::Value::String(primary_owner.clone()),
                );
            sarif_results.push(result);
        }
    }

    let sarif = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": [sarif_rule("fallow/code-duplication", "Duplicated code block", "warning")]
                }
            },
            "results": sarif_results
        }]
    });

    emit_json(&sarif, "SARIF")
}

// ── Health SARIF output ────────────────────────────────────────────
// Note: file_scores are intentionally omitted from SARIF output.
// SARIF is designed for diagnostic results (issues/findings), not metric tables.
// File health scores are available in JSON, human, compact, and markdown formats.

#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "flat rules + results table: adding runtime-coverage rules pushed past the 150 line threshold but each section is a straightforward sequence of sarif_rule / sarif_result calls"
)]
pub fn build_health_sarif(
    report: &crate::health_types::HealthReport,
    root: &Path,
) -> serde_json::Value {
    use crate::health_types::ExceededThreshold;

    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    for finding in &report.findings {
        let uri = relative_uri(&finding.path, root);
        // When CRAP contributes alongside complexity, use the CRAP rule as the
        // most actionable identifier (CRAP combines complexity and coverage)
        // and surface all exceeded dimensions in the message.
        let (rule_id, message) = match finding.exceeded {
            ExceededThreshold::Cyclomatic => (
                "fallow/high-cyclomatic-complexity",
                format!(
                    "'{}' has cyclomatic complexity {} (threshold: {})",
                    finding.name, finding.cyclomatic, report.summary.max_cyclomatic_threshold,
                ),
            ),
            ExceededThreshold::Cognitive => (
                "fallow/high-cognitive-complexity",
                format!(
                    "'{}' has cognitive complexity {} (threshold: {})",
                    finding.name, finding.cognitive, report.summary.max_cognitive_threshold,
                ),
            ),
            ExceededThreshold::Both => (
                "fallow/high-complexity",
                format!(
                    "'{}' has cyclomatic complexity {} (threshold: {}) and cognitive complexity {} (threshold: {})",
                    finding.name,
                    finding.cyclomatic,
                    report.summary.max_cyclomatic_threshold,
                    finding.cognitive,
                    report.summary.max_cognitive_threshold,
                ),
            ),
            ExceededThreshold::Crap
            | ExceededThreshold::CyclomaticCrap
            | ExceededThreshold::CognitiveCrap
            | ExceededThreshold::All => {
                let crap = finding.crap.unwrap_or(0.0);
                let coverage = finding
                    .coverage_pct
                    .map(|pct| format!(", coverage {pct:.0}%"))
                    .unwrap_or_default();
                (
                    "fallow/high-crap-score",
                    format!(
                        "'{}' has CRAP score {:.1} (threshold: {:.1}, cyclomatic {}{})",
                        finding.name,
                        crap,
                        report.summary.max_crap_threshold,
                        finding.cyclomatic,
                        coverage,
                    ),
                )
            }
        };

        let level = match finding.severity {
            crate::health_types::FindingSeverity::Critical => "error",
            crate::health_types::FindingSeverity::High => "warning",
            crate::health_types::FindingSeverity::Moderate => "note",
        };
        let source_snippet = snippets.line(&finding.path, finding.line);
        sarif_results.push(sarif_result_with_snippet(
            rule_id,
            level,
            &message,
            &uri,
            Some((finding.line, finding.col + 1)),
            source_snippet.as_deref(),
        ));
    }

    if let Some(ref production) = report.runtime_coverage {
        append_runtime_coverage_sarif_results(&mut sarif_results, production, root, &mut snippets);
    }

    // Refactoring targets as SARIF results (warning level — advisory recommendations)
    for target in &report.targets {
        let uri = relative_uri(&target.path, root);
        let message = format!(
            "[{}] {} (priority: {:.1}, efficiency: {:.1}, effort: {}, confidence: {})",
            target.category.label(),
            target.recommendation,
            target.priority,
            target.efficiency,
            target.effort.label(),
            target.confidence.label(),
        );
        sarif_results.push(sarif_result(
            "fallow/refactoring-target",
            "warning",
            &message,
            &uri,
            None,
        ));
    }

    if let Some(ref gaps) = report.coverage_gaps {
        for item in &gaps.files {
            let uri = relative_uri(&item.path, root);
            let message = format!(
                "File is runtime-reachable but has no test dependency path ({} value export{})",
                item.value_export_count,
                if item.value_export_count == 1 {
                    ""
                } else {
                    "s"
                },
            );
            sarif_results.push(sarif_result(
                "fallow/untested-file",
                "warning",
                &message,
                &uri,
                None,
            ));
        }

        for item in &gaps.exports {
            let uri = relative_uri(&item.path, root);
            let message = format!(
                "Export '{}' is runtime-reachable but never referenced by test-reachable modules",
                item.export_name
            );
            let source_snippet = snippets.line(&item.path, item.line);
            sarif_results.push(sarif_result_with_snippet(
                "fallow/untested-export",
                "warning",
                &message,
                &uri,
                Some((item.line, item.col + 1)),
                source_snippet.as_deref(),
            ));
        }
    }

    let health_rules = vec![
        sarif_rule(
            "fallow/high-cyclomatic-complexity",
            "Function has high cyclomatic complexity",
            "note",
        ),
        sarif_rule(
            "fallow/high-cognitive-complexity",
            "Function has high cognitive complexity",
            "note",
        ),
        sarif_rule(
            "fallow/high-complexity",
            "Function exceeds both complexity thresholds",
            "note",
        ),
        sarif_rule(
            "fallow/high-crap-score",
            "Function has a high CRAP score (high complexity combined with low coverage)",
            "warning",
        ),
        sarif_rule(
            "fallow/refactoring-target",
            "File identified as a high-priority refactoring candidate",
            "warning",
        ),
        sarif_rule(
            "fallow/untested-file",
            "Runtime-reachable file has no test dependency path",
            "warning",
        ),
        sarif_rule(
            "fallow/untested-export",
            "Runtime-reachable export has no test dependency path",
            "warning",
        ),
        sarif_rule(
            "fallow/runtime-safe-to-delete",
            "Function is statically unused and was never invoked in production",
            "warning",
        ),
        sarif_rule(
            "fallow/runtime-review-required",
            "Function is statically used but was never invoked in production",
            "warning",
        ),
        sarif_rule(
            "fallow/runtime-low-traffic",
            "Function was invoked below the low-traffic threshold relative to total trace count",
            "note",
        ),
        sarif_rule(
            "fallow/runtime-coverage-unavailable",
            "Runtime coverage could not be resolved for this function",
            "note",
        ),
        sarif_rule(
            "fallow/runtime-coverage",
            "Runtime coverage finding",
            "note",
        ),
    ];

    serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": health_rules
                }
            },
            "results": sarif_results
        }]
    })
}

// Note: `production.hot_paths`, `production.signals`, and per-hot-path
// `end_line` are intentionally omitted from SARIF output. SARIF is
// designed for diagnostic results (issues a reviewer should act on),
// not for state observations. `hot-path-touched` is informational
// (PR-context heads-up that a touched function is on the hot path),
// not a finding to fix; surfacing it as a SARIF result would clutter
// Code Scanning's UI with non-actionable entries. JSON consumers that
// want the full picture read `runtime_coverage.signals[]` and
// `runtime_coverage.hot_paths[]` directly.
fn append_runtime_coverage_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    production: &crate::health_types::RuntimeCoverageReport,
    root: &Path,
    snippets: &mut SourceSnippetCache,
) {
    for finding in &production.findings {
        let uri = relative_uri(&finding.path, root);
        let rule_id = match finding.verdict {
            crate::health_types::RuntimeCoverageVerdict::SafeToDelete => {
                "fallow/runtime-safe-to-delete"
            }
            crate::health_types::RuntimeCoverageVerdict::ReviewRequired => {
                "fallow/runtime-review-required"
            }
            crate::health_types::RuntimeCoverageVerdict::LowTraffic => "fallow/runtime-low-traffic",
            crate::health_types::RuntimeCoverageVerdict::CoverageUnavailable => {
                "fallow/runtime-coverage-unavailable"
            }
            crate::health_types::RuntimeCoverageVerdict::Active
            | crate::health_types::RuntimeCoverageVerdict::Unknown => "fallow/runtime-coverage",
        };
        let level = match finding.verdict {
            crate::health_types::RuntimeCoverageVerdict::SafeToDelete
            | crate::health_types::RuntimeCoverageVerdict::ReviewRequired => "warning",
            _ => "note",
        };
        let invocations_hint = finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations"),
        );
        let message = format!(
            "'{}' runtime coverage verdict: {} ({})",
            finding.function,
            finding.verdict.human_label(),
            invocations_hint,
        );
        let source_snippet = snippets.line(&finding.path, finding.line);
        sarif_results.push(sarif_result_with_snippet(
            rule_id,
            level,
            &message,
            &uri,
            Some((finding.line, 1)),
            source_snippet.as_deref(),
        ));
    }
}

pub(super) fn print_health_sarif(
    report: &crate::health_types::HealthReport,
    root: &Path,
) -> ExitCode {
    let sarif = build_health_sarif(report, root);
    emit_json(&sarif, "SARIF")
}

/// Print health SARIF with a per-result `properties.group` tag.
///
/// Mirrors the dead-code grouped SARIF pattern (`print_grouped_sarif`):
/// build the standard SARIF first, then post-process each result to inject
/// the resolver-derived group key on `properties.group`. Consumers that read
/// SARIF (GitHub Code Scanning, GitLab Code Quality) can then partition
/// findings per team / package / directory without dropping out of the
/// SARIF pipeline. Each finding's URI is decoded (`%5B` -> `[`, `%5D` -> `]`)
/// before resolution, matching the dead-code behaviour for paths containing
/// brackets like Next.js dynamic routes.
pub(super) fn print_grouped_health_sarif(
    report: &crate::health_types::HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut sarif = build_health_sarif(report, root);

    if let Some(runs) = sarif.get_mut("runs").and_then(|r| r.as_array_mut()) {
        for run in runs {
            if let Some(results) = run.get_mut("results").and_then(|r| r.as_array_mut()) {
                for result in results {
                    let uri = result
                        .pointer("/locations/0/physicalLocation/artifactLocation/uri")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let decoded = uri.replace("%5B", "[").replace("%5D", "]");
                    let group =
                        grouping::resolve_owner(Path::new(&decoded), Path::new(""), resolver);
                    let props = result
                        .as_object_mut()
                        .expect("SARIF result should be an object")
                        .entry("properties")
                        .or_insert_with(|| serde_json::json!({}));
                    props
                        .as_object_mut()
                        .expect("properties should be an object")
                        .insert("group".to_string(), serde_json::Value::String(group));
                }
            }
        }
    }

    emit_json(&sarif, "SARIF")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use fallow_core::results::*;
    use std::path::PathBuf;

    #[test]
    fn sarif_has_required_top_level_fields() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        assert_eq!(
            sarif["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        assert_eq!(sarif["version"], "2.1.0");
        assert!(sarif["runs"].is_array());
    }

    #[test]
    fn sarif_has_tool_driver_info() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        let driver = &sarif["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "fallow");
        assert!(driver["version"].is_string());
        assert_eq!(
            driver["informationUri"],
            "https://github.com/fallow-rs/fallow"
        );
    }

    #[test]
    fn sarif_declares_all_rules() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("rules should be an array");
        assert_eq!(rules.len(), 18);

        let rule_ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert!(rule_ids.contains(&"fallow/unused-file"));
        assert!(rule_ids.contains(&"fallow/unused-export"));
        assert!(rule_ids.contains(&"fallow/unused-type"));
        assert!(rule_ids.contains(&"fallow/private-type-leak"));
        assert!(rule_ids.contains(&"fallow/unused-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-dev-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-optional-dependency"));
        assert!(rule_ids.contains(&"fallow/type-only-dependency"));
        assert!(rule_ids.contains(&"fallow/test-only-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-enum-member"));
        assert!(rule_ids.contains(&"fallow/unused-class-member"));
        assert!(rule_ids.contains(&"fallow/unresolved-import"));
        assert!(rule_ids.contains(&"fallow/unlisted-dependency"));
        assert!(rule_ids.contains(&"fallow/duplicate-export"));
        assert!(rule_ids.contains(&"fallow/circular-dependency"));
        assert!(rule_ids.contains(&"fallow/boundary-violation"));
        assert!(rule_ids.contains(&"fallow/unused-catalog-entry"));
    }

    #[test]
    fn sarif_empty_results_no_results_entries() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        let sarif_results = sarif["runs"][0]["results"]
            .as_array()
            .expect("results should be an array");
        assert!(sarif_results.is_empty());
    }

    #[test]
    fn sarif_unused_file_result() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry["ruleId"], "fallow/unused-file");
        // Default severity is "error" per RulesConfig::default()
        assert_eq!(entry["level"], "error");
        assert_eq!(
            entry["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/dead.ts"
        );
    }

    #[test]
    fn sarif_unused_export_includes_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unused-export");

        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 10);
        // SARIF columns are 1-based, code adds +1 to the 0-based col
        assert_eq!(region["startColumn"], 5);
    }

    #[test]
    fn sarif_unresolved_import_is_error_level() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing".to_string(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unresolved-import");
        assert_eq!(entry["level"], "error");
    }

    #[test]
    fn sarif_unlisted_dependency_points_to_import_site() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".to_string(),
            imported_from: vec![ImportSite {
                path: root.join("src/cli.ts"),
                line: 3,
                col: 0,
            }],
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unlisted-dependency");
        assert_eq!(entry["level"], "error");
        assert_eq!(
            entry["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/cli.ts"
        );
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 3);
        assert_eq!(region["startColumn"], 1);
    }

    #[test]
    fn sarif_dependency_issues_point_to_package_json() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        for entry in entries {
            assert_eq!(
                entry["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
                "package.json"
            );
        }
    }

    #[test]
    fn sarif_duplicate_export_emits_one_result_per_location() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/b.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        // One SARIF result per location, not one per DuplicateExport
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["ruleId"], "fallow/duplicate-export");
        assert_eq!(entries[1]["ruleId"], "fallow/duplicate-export");
        assert_eq!(
            entries[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/a.ts"
        );
        assert_eq!(
            entries[1]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/b.ts"
        );
    }

    #[test]
    fn sarif_all_issue_types_produce_results() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        // All issue types with one entry each; duplicate_exports has 2 locations => one extra SARIF result
        assert_eq!(entries.len(), results.total_issues() + 1);

        let rule_ids: Vec<&str> = entries
            .iter()
            .map(|e| e["ruleId"].as_str().unwrap())
            .collect();
        assert!(rule_ids.contains(&"fallow/unused-file"));
        assert!(rule_ids.contains(&"fallow/unused-export"));
        assert!(rule_ids.contains(&"fallow/unused-type"));
        assert!(rule_ids.contains(&"fallow/unused-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-dev-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-optional-dependency"));
        assert!(rule_ids.contains(&"fallow/type-only-dependency"));
        assert!(rule_ids.contains(&"fallow/test-only-dependency"));
        assert!(rule_ids.contains(&"fallow/unused-enum-member"));
        assert!(rule_ids.contains(&"fallow/unused-class-member"));
        assert!(rule_ids.contains(&"fallow/unresolved-import"));
        assert!(rule_ids.contains(&"fallow/unlisted-dependency"));
        assert!(rule_ids.contains(&"fallow/duplicate-export"));
    }

    #[test]
    fn sarif_serializes_to_valid_json() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let sarif = build_sarif(&results, &root, &RulesConfig::default());

        let json_str = serde_json::to_string_pretty(&sarif).expect("SARIF should serialize");
        let reparsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("SARIF output should be valid JSON");
        assert_eq!(reparsed, sarif);
    }

    #[test]
    fn sarif_file_write_produces_valid_sarif() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let json_str = serde_json::to_string_pretty(&sarif).expect("SARIF should serialize");

        let dir = std::env::temp_dir().join("fallow-test-sarif-file");
        let _ = std::fs::create_dir_all(&dir);
        let sarif_path = dir.join("results.sarif");
        std::fs::write(&sarif_path, &json_str).expect("should write SARIF file");

        let contents = std::fs::read_to_string(&sarif_path).expect("should read SARIF file");
        let parsed: serde_json::Value =
            serde_json::from_str(&contents).expect("file should contain valid JSON");

        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(
            parsed["$schema"],
            "https://json.schemastore.org/sarif-2.1.0.json"
        );
        let sarif_results = parsed["runs"][0]["results"]
            .as_array()
            .expect("results should be an array");
        assert!(!sarif_results.is_empty());

        // Clean up
        let _ = std::fs::remove_file(&sarif_path);
        let _ = std::fs::remove_dir(&dir);
    }

    // ── Health SARIF ──

    #[test]
    fn health_sarif_empty_no_results() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        assert_eq!(sarif["version"], "2.1.0");
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert!(results.is_empty());
        let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 12);
    }

    #[test]
    fn health_sarif_cyclomatic_only() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/utils.ts"),
                name: "parseExpression".to_string(),
                line: 42,
                col: 0,
                cyclomatic: 25,
                cognitive: 10,
                line_count: 80,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Cyclomatic,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/high-cyclomatic-complexity");
        assert_eq!(entry["level"], "warning");
        assert!(
            entry["message"]["text"]
                .as_str()
                .unwrap()
                .contains("cyclomatic complexity 25")
        );
        assert_eq!(
            entry["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/utils.ts"
        );
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 42);
        assert_eq!(region["startColumn"], 1);
    }

    #[test]
    fn health_sarif_cognitive_only() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/api.ts"),
                name: "handleRequest".to_string(),
                line: 10,
                col: 4,
                cyclomatic: 8,
                cognitive: 20,
                line_count: 40,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Cognitive,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 3,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/high-cognitive-complexity");
        assert!(
            entry["message"]["text"]
                .as_str()
                .unwrap()
                .contains("cognitive complexity 20")
        );
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startColumn"], 5); // col 4 + 1
    }

    #[test]
    fn health_sarif_both_thresholds() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/complex.ts"),
                name: "doEverything".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 30,
                cognitive: 45,
                line_count: 100,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/high-complexity");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.contains("cyclomatic complexity 30"));
        assert!(msg.contains("cognitive complexity 45"));
    }

    #[test]
    fn health_sarif_crap_only_emits_crap_rule() {
        // CRAP-only: cyclomatic + cognitive below their thresholds, CRAP at or
        // above the CRAP threshold. Rule must be `fallow/high-crap-score`.
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/untested.ts"),
                name: "risky".to_string(),
                line: 8,
                col: 0,
                cyclomatic: 10,
                cognitive: 10,
                line_count: 20,
                param_count: 1,
                exceeded: crate::health_types::ExceededThreshold::Crap,
                severity: crate::health_types::FindingSeverity::High,
                crap: Some(82.2),
                coverage_pct: Some(12.0),
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/high-crap-score");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.contains("CRAP score 82.2"), "msg: {msg}");
        assert!(msg.contains("coverage 12%"), "msg: {msg}");
    }

    #[test]
    fn health_sarif_cyclomatic_crap_uses_crap_rule() {
        // Cyclomatic + CRAP both exceeded. The CRAP-centric rule subsumes
        // the cyclomatic breach; only one SARIF result is emitted.
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/hot.ts"),
                name: "branchy".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 67,
                cognitive: 12,
                line_count: 80,
                param_count: 1,
                exceeded: crate::health_types::ExceededThreshold::CyclomaticCrap,
                severity: crate::health_types::FindingSeverity::Critical,
                crap: Some(182.0),
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let sarif = build_health_sarif(&report, &root);
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(
            results.len(),
            1,
            "CyclomaticCrap should emit a single SARIF result under the CRAP rule"
        );
        assert_eq!(results[0]["ruleId"], "fallow/high-crap-score");
        let msg = results[0]["message"]["text"].as_str().unwrap();
        assert!(msg.contains("CRAP score 182"), "msg: {msg}");
        // coverage_pct absent => no coverage suffix
        assert!(!msg.contains("coverage"), "msg: {msg}");
    }

    // ── Severity mapping ──

    #[test]
    fn severity_to_sarif_level_error() {
        assert_eq!(severity_to_sarif_level(Severity::Error), "error");
    }

    #[test]
    fn severity_to_sarif_level_warn() {
        assert_eq!(severity_to_sarif_level(Severity::Warn), "warning");
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn severity_to_sarif_level_off() {
        let _ = severity_to_sarif_level(Severity::Off);
    }

    // ── Re-export properties ──

    #[test]
    fn sarif_re_export_has_properties() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExported".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["properties"]["is_re_export"], true);
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.starts_with("Re-export"));
    }

    #[test]
    fn sarif_non_re_export_has_no_properties() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "foo".to_string(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert!(entry.get("properties").is_none());
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.starts_with("Export"));
    }

    // ── Type re-export ──

    #[test]
    fn sarif_type_re_export_message() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_types.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "MyType".to_string(),
            is_type_only: true,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unused-type");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.starts_with("Type re-export"));
        assert_eq!(entry["properties"]["is_re_export"], true);
    }

    // ── Dependency line == 0 skips region ──

    #[test]
    fn sarif_dependency_line_zero_skips_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 0,
            used_in_workspaces: Vec::new(),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let phys = &entry["locations"][0]["physicalLocation"];
        assert!(phys.get("region").is_none());
    }

    #[test]
    fn sarif_dependency_line_nonzero_has_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 7,
            used_in_workspaces: Vec::new(),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 7);
        assert_eq!(region["startColumn"], 1);
    }

    // ── Type-only dependency line == 0 skips region ──

    #[test]
    fn sarif_type_only_dep_line_zero_skips_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 0,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let phys = &entry["locations"][0]["physicalLocation"];
        assert!(phys.get("region").is_none());
    }

    // ── Circular dependency line == 0 skips region ──

    #[test]
    fn sarif_circular_dep_line_zero_skips_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
            length: 2,
            line: 0,
            col: 0,
            is_cross_package: false,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let phys = &entry["locations"][0]["physicalLocation"];
        assert!(phys.get("region").is_none());
    }

    #[test]
    fn sarif_circular_dep_line_nonzero_has_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
            length: 2,
            line: 5,
            col: 2,
            is_cross_package: false,
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 5);
        assert_eq!(region["startColumn"], 3);
    }

    // ── Unused optional dependency ──

    #[test]
    fn sarif_unused_optional_dependency_result() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unused-optional-dependency");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.contains("optionalDependencies"));
    }

    // ── Enum and class member SARIF messages ──

    #[test]
    fn sarif_enum_member_message_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(fallow_core::results::UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Color".to_string(),
                member_name: "Purple".to_string(),
                kind: fallow_core::extract::MemberKind::EnumMember,
                line: 5,
                col: 2,
            });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unused-enum-member");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.contains("Enum member 'Color.Purple'"));
        let region = &entry["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startColumn"], 3); // col 2 + 1
    }

    #[test]
    fn sarif_class_member_message_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(fallow_core::results::UnusedMember {
                path: root.join("src/service.ts"),
                parent_name: "API".to_string(),
                member_name: "fetch".to_string(),
                kind: fallow_core::extract::MemberKind::ClassMethod,
                line: 10,
                col: 4,
            });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["ruleId"], "fallow/unused-class-member");
        let msg = entry["message"]["text"].as_str().unwrap();
        assert!(msg.contains("Class member 'API.fetch'"));
    }

    // ── Duplication SARIF ──

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test line/col values are trivially small"
    )]
    fn duplication_sarif_structure() {
        use fallow_core::duplicates::*;

        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 1,
                        end_line: 10,
                        start_col: 0,
                        end_col: 0,
                        fragment: String::new(),
                    },
                    CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 5,
                        end_line: 14,
                        start_col: 2,
                        end_col: 0,
                        fragment: String::new(),
                    },
                ],
                token_count: 50,
                line_count: 10,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };

        let sarif = serde_json::json!({
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "version": "2.1.0",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "fallow",
                        "version": env!("CARGO_PKG_VERSION"),
                        "informationUri": "https://github.com/fallow-rs/fallow",
                        "rules": [sarif_rule("fallow/code-duplication", "Duplicated code block", "warning")]
                    }
                },
                "results": []
            }]
        });
        // Just verify the function doesn't panic and produces expected structure
        let _ = sarif;

        // Test the actual build path through print_duplication_sarif internals
        let mut sarif_results = Vec::new();
        for (i, group) in report.clone_groups.iter().enumerate() {
            for instance in &group.instances {
                sarif_results.push(sarif_result(
                    "fallow/code-duplication",
                    "warning",
                    &format!(
                        "Code clone group {} ({} lines, {} instances)",
                        i + 1,
                        group.line_count,
                        group.instances.len()
                    ),
                    &super::super::relative_uri(&instance.file, &root),
                    Some((instance.start_line as u32, (instance.start_col + 1) as u32)),
                ));
            }
        }
        assert_eq!(sarif_results.len(), 2);
        assert_eq!(sarif_results[0]["ruleId"], "fallow/code-duplication");
        assert!(
            sarif_results[0]["message"]["text"]
                .as_str()
                .unwrap()
                .contains("10 lines")
        );
        let region0 = &sarif_results[0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region0["startLine"], 1);
        assert_eq!(region0["startColumn"], 1); // start_col 0 + 1
        let region1 = &sarif_results[1]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region1["startLine"], 5);
        assert_eq!(region1["startColumn"], 3); // start_col 2 + 1
    }

    // ── sarif_rule fallback (unknown rule ID) ──

    #[test]
    fn sarif_rule_known_id_has_full_description() {
        let rule = sarif_rule("fallow/unused-file", "fallback text", "error");
        assert!(rule.get("fullDescription").is_some());
        assert!(rule.get("helpUri").is_some());
    }

    #[test]
    fn sarif_rule_unknown_id_uses_fallback() {
        let rule = sarif_rule("fallow/nonexistent", "fallback text", "warning");
        assert_eq!(rule["shortDescription"]["text"], "fallback text");
        assert!(rule.get("fullDescription").is_none());
        assert!(rule.get("helpUri").is_none());
        assert_eq!(rule["defaultConfiguration"]["level"], "warning");
    }

    // ── sarif_result without region ──

    #[test]
    fn sarif_result_no_region_omits_region_key() {
        let result = sarif_result("rule/test", "error", "test msg", "src/file.ts", None);
        let phys = &result["locations"][0]["physicalLocation"];
        assert!(phys.get("region").is_none());
        assert_eq!(phys["artifactLocation"]["uri"], "src/file.ts");
    }

    #[test]
    fn sarif_result_with_region_includes_region() {
        let result = sarif_result(
            "rule/test",
            "error",
            "test msg",
            "src/file.ts",
            Some((10, 5)),
        );
        let region = &result["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 10);
        assert_eq!(region["startColumn"], 5);
    }

    #[test]
    fn sarif_partial_fingerprint_ignores_rendered_message() {
        let a = sarif_result(
            "rule/test",
            "error",
            "first message",
            "src/file.ts",
            Some((10, 5)),
        );
        let b = sarif_result(
            "rule/test",
            "error",
            "rewritten message",
            "src/file.ts",
            Some((10, 5)),
        );
        assert_eq!(
            a["partialFingerprints"][fingerprint::FINGERPRINT_KEY],
            b["partialFingerprints"][fingerprint::FINGERPRINT_KEY]
        );
    }

    // ── Health SARIF refactoring targets ──

    #[test]
    fn health_sarif_includes_refactoring_targets() {
        use crate::health_types::*;

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            targets: vec![RefactoringTarget {
                path: root.join("src/complex.ts"),
                priority: 85.0,
                efficiency: 42.5,
                recommendation: "Split high-impact file".into(),
                category: RecommendationCategory::SplitHighImpact,
                effort: EffortEstimate::Medium,
                confidence: Confidence::High,
                factors: vec![],
                evidence: None,
            }],
            ..Default::default()
        };

        let sarif = build_health_sarif(&report, &root);
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["ruleId"], "fallow/refactoring-target");
        assert_eq!(entries[0]["level"], "warning");
        let msg = entries[0]["message"]["text"].as_str().unwrap();
        assert!(msg.contains("high impact"));
        assert!(msg.contains("Split high-impact file"));
        assert!(msg.contains("42.5"));
    }

    #[test]
    fn health_sarif_includes_coverage_gaps() {
        use crate::health_types::*;

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 2,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 1,
                },
                files: vec![UntestedFile {
                    path: root.join("src/app.ts"),
                    value_export_count: 2,
                }],
                exports: vec![UntestedExport {
                    path: root.join("src/app.ts"),
                    export_name: "loader".into(),
                    line: 12,
                    col: 4,
                }],
            }),
            ..Default::default()
        };

        let sarif = build_health_sarif(&report, &root);
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["ruleId"], "fallow/untested-file");
        assert_eq!(
            entries[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/app.ts"
        );
        assert!(
            entries[0]["message"]["text"]
                .as_str()
                .unwrap()
                .contains("2 value exports")
        );
        assert_eq!(entries[1]["ruleId"], "fallow/untested-export");
        assert_eq!(
            entries[1]["locations"][0]["physicalLocation"]["region"]["startLine"],
            12
        );
        assert_eq!(
            entries[1]["locations"][0]["physicalLocation"]["region"]["startColumn"],
            5
        );
    }

    // ── Health SARIF rules include fullDescription from explain module ──

    #[test]
    fn health_sarif_rules_have_full_descriptions() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport::default();
        let sarif = build_health_sarif(&report, &root);
        let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        for rule in rules {
            let id = rule["id"].as_str().unwrap();
            assert!(
                rule.get("fullDescription").is_some(),
                "health rule {id} should have fullDescription"
            );
            assert!(
                rule.get("helpUri").is_some(),
                "health rule {id} should have helpUri"
            );
        }
    }

    // ── Warn severity propagates correctly ──

    #[test]
    fn sarif_warn_severity_produces_warning_level() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });

        let rules = RulesConfig {
            unused_files: Severity::Warn,
            ..RulesConfig::default()
        };

        let sarif = build_sarif(&results, &root, &rules);
        let entry = &sarif["runs"][0]["results"][0];
        assert_eq!(entry["level"], "warning");
    }

    // ── Unused file has no region ──

    #[test]
    fn sarif_unused_file_has_no_region() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entry = &sarif["runs"][0]["results"][0];
        let phys = &entry["locations"][0]["physicalLocation"];
        assert!(phys.get("region").is_none());
    }

    // ── Multiple unlisted deps with multiple import sites ──

    #[test]
    fn sarif_unlisted_dep_multiple_import_sites() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "dotenv".to_string(),
            imported_from: vec![
                ImportSite {
                    path: root.join("src/a.ts"),
                    line: 1,
                    col: 0,
                },
                ImportSite {
                    path: root.join("src/b.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        // One SARIF result per import site
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/a.ts"
        );
        assert_eq!(
            entries[1]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "src/b.ts"
        );
    }

    // ── Empty unlisted dep (no import sites) produces zero results ──

    #[test]
    fn sarif_unlisted_dep_no_import_sites() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "phantom".to_string(),
            imported_from: vec![],
        });

        let sarif = build_sarif(&results, &root, &RulesConfig::default());
        let entries = sarif["runs"][0]["results"].as_array().unwrap();
        // No import sites => no SARIF results for this unlisted dep
        assert!(entries.is_empty());
    }
}
