use std::path::Path;
use std::process::ExitCode;

use fallow_config::{RulesConfig, Severity};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{AnalysisResults, PrivateTypeLeak};

use super::ci::{fingerprint, severity};
use super::grouping::{self, OwnershipResolver};
use super::{emit_json, normalize_uri, relative_path};
use crate::health_types::{ExceededThreshold, HealthReport};

/// Map fallow severity to CodeClimate severity.
fn severity_to_codeclimate(s: Severity) -> &'static str {
    severity::codeclimate_severity(s)
}

/// Compute a relative path string with forward-slash normalization.
///
/// Uses `normalize_uri` to ensure forward slashes on all platforms
/// and percent-encode brackets for Next.js dynamic routes.
fn cc_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

/// Compute a deterministic fingerprint hash from key fields.
///
/// Uses FNV-1a (64-bit) for guaranteed cross-version stability.
/// `DefaultHasher` is explicitly not specified across Rust versions.
fn fingerprint_hash(parts: &[&str]) -> String {
    fingerprint::fingerprint_hash(parts)
}

/// Build a single CodeClimate issue object.
fn cc_issue(
    check_name: &str,
    description: &str,
    severity: &str,
    category: &str,
    path: &str,
    begin_line: Option<u32>,
    fingerprint: &str,
) -> serde_json::Value {
    let lines = begin_line.map_or_else(
        || serde_json::json!({ "begin": 1 }),
        |line| serde_json::json!({ "begin": line }),
    );

    serde_json::json!({
        "type": "issue",
        "check_name": check_name,
        "description": description,
        "categories": [category],
        "severity": severity,
        "fingerprint": fingerprint,
        "location": {
            "path": path,
            "lines": lines
        }
    })
}

/// Push CodeClimate issues for unused dependencies with a shared structure.
fn push_dep_cc_issues(
    issues: &mut Vec<serde_json::Value>,
    deps: &[fallow_core::results::UnusedDependency],
    root: &Path,
    rule_id: &str,
    location_label: &str,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dep in deps {
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&[rule_id, &dep.package_name]);
        let workspace_context = if dep.used_in_workspaces.is_empty() {
            String::new()
        } else {
            let workspaces = dep
                .used_in_workspaces
                .iter()
                .map(|path| cc_path(path, root))
                .collect::<Vec<_>>()
                .join(", ");
            format!("; imported in other workspaces: {workspaces}")
        };
        issues.push(cc_issue(
            rule_id,
            &format!(
                "Package '{}' is in {location_label} but never imported{workspace_context}",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

fn push_unused_file_issues(
    issues: &mut Vec<serde_json::Value>,
    files: &[fallow_core::results::UnusedFile],
    root: &Path,
    severity: Severity,
) {
    if files.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for file in files {
        let path = cc_path(&file.path, root);
        let fp = fingerprint_hash(&["fallow/unused-file", &path]);
        issues.push(cc_issue(
            "fallow/unused-file",
            "File is not reachable from any entry point",
            level,
            "Bug Risk",
            &path,
            None,
            &fp,
        ));
    }
}

/// Push CodeClimate issues for unused exports or unused types.
///
/// `direct_label` / `re_export_label` let the same helper produce the right
/// prose for both `unused-export` (Export / Re-export) and `unused-type`
/// (Type export / Type re-export) rule ids.
fn push_unused_export_issues(
    issues: &mut Vec<serde_json::Value>,
    exports: &[fallow_core::results::UnusedExport],
    root: &Path,
    rule_id: &str,
    direct_label: &str,
    re_export_label: &str,
    severity: Severity,
) {
    if exports.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for export in exports {
        let path = cc_path(&export.path, root);
        let kind = if export.is_re_export {
            re_export_label
        } else {
            direct_label
        };
        let line_str = export.line.to_string();
        let fp = fingerprint_hash(&[rule_id, &path, &line_str, &export.export_name]);
        issues.push(cc_issue(
            rule_id,
            &format!(
                "{kind} '{}' is never imported by other modules",
                export.export_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(export.line),
            &fp,
        ));
    }
}

fn push_private_type_leak_issues(
    issues: &mut Vec<serde_json::Value>,
    leaks: &[PrivateTypeLeak],
    root: &Path,
    severity: Severity,
) {
    if leaks.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for leak in leaks {
        let path = cc_path(&leak.path, root);
        let line_str = leak.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/private-type-leak",
            &path,
            &line_str,
            &leak.export_name,
            &leak.type_name,
        ]);
        issues.push(cc_issue(
            "fallow/private-type-leak",
            &format!(
                "Export '{}' references private type '{}'",
                leak.export_name, leak.type_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(leak.line),
            &fp,
        ));
    }
}

fn push_type_only_dep_issues(
    issues: &mut Vec<serde_json::Value>,
    deps: &[fallow_core::results::TypeOnlyDependency],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dep in deps {
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/type-only-dependency", &dep.package_name]);
        issues.push(cc_issue(
            "fallow/type-only-dependency",
            &format!(
                "Package '{}' is only imported via type-only imports (consider moving to devDependencies)",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

fn push_test_only_dep_issues(
    issues: &mut Vec<serde_json::Value>,
    deps: &[fallow_core::results::TestOnlyDependency],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dep in deps {
        let path = cc_path(&dep.path, root);
        let line = if dep.line > 0 { Some(dep.line) } else { None };
        let fp = fingerprint_hash(&["fallow/test-only-dependency", &dep.package_name]);
        issues.push(cc_issue(
            "fallow/test-only-dependency",
            &format!(
                "Package '{}' is only imported by test files (consider moving to devDependencies)",
                dep.package_name
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

/// Push CodeClimate issues for unused enum or class members.
///
/// `entity_label` is `"Enum"` or `"Class"` so the rendered description reads
/// "Enum member ..." or "Class member ..." accordingly.
fn push_unused_member_issues(
    issues: &mut Vec<serde_json::Value>,
    members: &[fallow_core::results::UnusedMember],
    root: &Path,
    rule_id: &str,
    entity_label: &str,
    severity: Severity,
) {
    if members.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for member in members {
        let path = cc_path(&member.path, root);
        let line_str = member.line.to_string();
        let fp = fingerprint_hash(&[
            rule_id,
            &path,
            &line_str,
            &member.parent_name,
            &member.member_name,
        ]);
        issues.push(cc_issue(
            rule_id,
            &format!(
                "{entity_label} member '{}.{}' is never referenced",
                member.parent_name, member.member_name
            ),
            level,
            "Bug Risk",
            &path,
            Some(member.line),
            &fp,
        ));
    }
}

fn push_unresolved_import_issues(
    issues: &mut Vec<serde_json::Value>,
    imports: &[fallow_core::results::UnresolvedImport],
    root: &Path,
    severity: Severity,
) {
    if imports.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for import in imports {
        let path = cc_path(&import.path, root);
        let line_str = import.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-import",
            &path,
            &line_str,
            &import.specifier,
        ]);
        issues.push(cc_issue(
            "fallow/unresolved-import",
            &format!("Import '{}' could not be resolved", import.specifier),
            level,
            "Bug Risk",
            &path,
            Some(import.line),
            &fp,
        ));
    }
}

fn push_unlisted_dep_issues(
    issues: &mut Vec<serde_json::Value>,
    deps: &[fallow_core::results::UnlistedDependency],
    root: &Path,
    severity: Severity,
) {
    if deps.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dep in deps {
        for site in &dep.imported_from {
            let path = cc_path(&site.path, root);
            let line_str = site.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/unlisted-dependency",
                &path,
                &line_str,
                &dep.package_name,
            ]);
            issues.push(cc_issue(
                "fallow/unlisted-dependency",
                &format!(
                    "Package '{}' is imported but not listed in package.json",
                    dep.package_name
                ),
                level,
                "Bug Risk",
                &path,
                Some(site.line),
                &fp,
            ));
        }
    }
}

fn push_duplicate_export_issues(
    issues: &mut Vec<serde_json::Value>,
    dups: &[fallow_core::results::DuplicateExport],
    root: &Path,
    severity: Severity,
) {
    if dups.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for dup in dups {
        for loc in &dup.locations {
            let path = cc_path(&loc.path, root);
            let line_str = loc.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/duplicate-export",
                &path,
                &line_str,
                &dup.export_name,
            ]);
            issues.push(cc_issue(
                "fallow/duplicate-export",
                &format!("Export '{}' appears in multiple modules", dup.export_name),
                level,
                "Bug Risk",
                &path,
                Some(loc.line),
                &fp,
            ));
        }
    }
}

fn push_circular_dep_issues(
    issues: &mut Vec<serde_json::Value>,
    cycles: &[fallow_core::results::CircularDependency],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for cycle in cycles {
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let fp = fingerprint_hash(&["fallow/circular-dependency", &chain_str]);
        let line = if cycle.line > 0 {
            Some(cycle.line)
        } else {
            None
        };
        issues.push(cc_issue(
            "fallow/circular-dependency",
            &format!(
                "Circular dependency{}: {}",
                if cycle.is_cross_package {
                    " (cross-package)"
                } else {
                    ""
                },
                chain.join(" \u{2192} ")
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

fn push_boundary_violation_issues(
    issues: &mut Vec<serde_json::Value>,
    violations: &[fallow_core::results::BoundaryViolation],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for v in violations {
        let path = cc_path(&v.from_path, root);
        let to = cc_path(&v.to_path, root);
        let fp = fingerprint_hash(&["fallow/boundary-violation", &path, &to]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        issues.push(cc_issue(
            "fallow/boundary-violation",
            &format!(
                "Boundary violation: {} -> {} ({} -> {})",
                path, to, v.from_zone, v.to_zone
            ),
            level,
            "Bug Risk",
            &path,
            line,
            &fp,
        ));
    }
}

fn push_stale_suppression_issues(
    issues: &mut Vec<serde_json::Value>,
    suppressions: &[fallow_core::results::StaleSuppression],
    root: &Path,
    severity: Severity,
) {
    if suppressions.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for s in suppressions {
        let path = cc_path(&s.path, root);
        let line_str = s.line.to_string();
        let fp = fingerprint_hash(&["fallow/stale-suppression", &path, &line_str]);
        issues.push(cc_issue(
            "fallow/stale-suppression",
            &s.description(),
            level,
            "Bug Risk",
            &path,
            Some(s.line),
            &fp,
        ));
    }
}

fn push_unused_catalog_entry_issues(
    issues: &mut Vec<serde_json::Value>,
    entries: &[fallow_core::results::UnusedCatalogEntry],
    root: &Path,
    severity: Severity,
) {
    if entries.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for entry in entries {
        let path = cc_path(&entry.path, root);
        let line_str = entry.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unused-catalog-entry",
            &path,
            &line_str,
            &entry.catalog_name,
            &entry.entry_name,
        ]);
        let description = if entry.catalog_name == "default" {
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
        issues.push(cc_issue(
            "fallow/unused-catalog-entry",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(entry.line),
            &fp,
        ));
    }
}

fn push_unresolved_catalog_reference_issues(
    issues: &mut Vec<serde_json::Value>,
    findings: &[fallow_core::results::UnresolvedCatalogReference],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_codeclimate(severity);
    for finding in findings {
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[
            "fallow/unresolved-catalog-reference",
            &path,
            &line_str,
            &finding.catalog_name,
            &finding.entry_name,
        ]);
        let catalog_phrase = if finding.catalog_name == "default" {
            "the default catalog".to_string()
        } else {
            format!("catalog '{}'", finding.catalog_name)
        };
        let mut description = format!(
            "Package '{}' is referenced via `catalog:{}` but {} does not declare it; `pnpm install` will fail",
            finding.entry_name,
            if finding.catalog_name == "default" {
                ""
            } else {
                finding.catalog_name.as_str()
            },
            catalog_phrase,
        );
        if !finding.available_in_catalogs.is_empty() {
            use std::fmt::Write as _;
            let _ = write!(
                description,
                " (available in: {})",
                finding.available_in_catalogs.join(", ")
            );
        }
        issues.push(cc_issue(
            "fallow/unresolved-catalog-reference",
            &description,
            level,
            "Bug Risk",
            &path,
            Some(finding.line),
            &fp,
        ));
    }
}

/// Build CodeClimate JSON array from dead-code analysis results.
#[must_use]
pub fn build_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> serde_json::Value {
    let mut issues = Vec::new();

    push_unused_file_issues(&mut issues, &results.unused_files, root, rules.unused_files);
    push_unused_export_issues(
        &mut issues,
        &results.unused_exports,
        root,
        "fallow/unused-export",
        "Export",
        "Re-export",
        rules.unused_exports,
    );
    push_unused_export_issues(
        &mut issues,
        &results.unused_types,
        root,
        "fallow/unused-type",
        "Type export",
        "Type re-export",
        rules.unused_types,
    );
    push_private_type_leak_issues(
        &mut issues,
        &results.private_type_leaks,
        root,
        rules.private_type_leaks,
    );
    push_dep_cc_issues(
        &mut issues,
        &results.unused_dependencies,
        root,
        "fallow/unused-dependency",
        "dependencies",
        rules.unused_dependencies,
    );
    push_dep_cc_issues(
        &mut issues,
        &results.unused_dev_dependencies,
        root,
        "fallow/unused-dev-dependency",
        "devDependencies",
        rules.unused_dev_dependencies,
    );
    push_dep_cc_issues(
        &mut issues,
        &results.unused_optional_dependencies,
        root,
        "fallow/unused-optional-dependency",
        "optionalDependencies",
        rules.unused_optional_dependencies,
    );
    push_type_only_dep_issues(
        &mut issues,
        &results.type_only_dependencies,
        root,
        rules.type_only_dependencies,
    );
    push_test_only_dep_issues(
        &mut issues,
        &results.test_only_dependencies,
        root,
        rules.test_only_dependencies,
    );
    push_unused_member_issues(
        &mut issues,
        &results.unused_enum_members,
        root,
        "fallow/unused-enum-member",
        "Enum",
        rules.unused_enum_members,
    );
    push_unused_member_issues(
        &mut issues,
        &results.unused_class_members,
        root,
        "fallow/unused-class-member",
        "Class",
        rules.unused_class_members,
    );
    push_unresolved_import_issues(
        &mut issues,
        &results.unresolved_imports,
        root,
        rules.unresolved_imports,
    );
    push_unlisted_dep_issues(
        &mut issues,
        &results.unlisted_dependencies,
        root,
        rules.unlisted_dependencies,
    );
    push_duplicate_export_issues(
        &mut issues,
        &results.duplicate_exports,
        root,
        rules.duplicate_exports,
    );
    push_circular_dep_issues(
        &mut issues,
        &results.circular_dependencies,
        root,
        rules.circular_dependencies,
    );
    push_boundary_violation_issues(
        &mut issues,
        &results.boundary_violations,
        root,
        rules.boundary_violation,
    );
    push_stale_suppression_issues(
        &mut issues,
        &results.stale_suppressions,
        root,
        rules.stale_suppressions,
    );
    push_unused_catalog_entry_issues(
        &mut issues,
        &results.unused_catalog_entries,
        root,
        rules.unused_catalog_entries,
    );
    push_unresolved_catalog_reference_issues(
        &mut issues,
        &results.unresolved_catalog_references,
        root,
        rules.unresolved_catalog_references,
    );

    serde_json::Value::Array(issues)
}

/// Print dead-code analysis results in CodeClimate format.
pub(super) fn print_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> ExitCode {
    let value = build_codeclimate(results, root, rules);
    emit_json(&value, "CodeClimate")
}

/// Print CodeClimate output with owner properties added to each issue.
///
/// Calls `build_codeclimate` to produce the standard CodeClimate JSON array,
/// then post-processes each entry to add `"owner": "@team"` by resolving the
/// issue's location path through the `OwnershipResolver`.
pub(super) fn print_grouped_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut value = build_codeclimate(results, root, rules);

    if let Some(issues) = value.as_array_mut() {
        for issue in issues {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let owner = grouping::resolve_owner(Path::new(path), Path::new(""), resolver);
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("owner".to_string(), serde_json::Value::String(owner));
        }
    }

    emit_json(&value, "CodeClimate")
}

/// Build CodeClimate JSON array from health/complexity analysis results.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "CRAP adds a fourth exceeded-threshold branch plus its description; splitting the dispatch table would fragment the mapping."
)]
pub fn build_health_codeclimate(report: &HealthReport, root: &Path) -> serde_json::Value {
    let mut issues = Vec::new();

    let cyc_t = report.summary.max_cyclomatic_threshold;
    let cog_t = report.summary.max_cognitive_threshold;
    let crap_t = report.summary.max_crap_threshold;

    for finding in &report.findings {
        let path = cc_path(&finding.path, root);
        let description = match finding.exceeded {
            ExceededThreshold::Both => format!(
                "'{}' has cyclomatic complexity {} (threshold: {}) and cognitive complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, cyc_t, finding.cognitive, cog_t
            ),
            ExceededThreshold::Cyclomatic => format!(
                "'{}' has cyclomatic complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, cyc_t
            ),
            ExceededThreshold::Cognitive => format!(
                "'{}' has cognitive complexity {} (threshold: {})",
                finding.name, finding.cognitive, cog_t
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
                format!(
                    "'{}' has CRAP score {crap:.1} (threshold: {crap_t:.1}, cyclomatic {}{coverage})",
                    finding.name, finding.cyclomatic,
                )
            }
        };
        let check_name = match finding.exceeded {
            ExceededThreshold::Both => "fallow/high-complexity",
            ExceededThreshold::Cyclomatic => "fallow/high-cyclomatic-complexity",
            ExceededThreshold::Cognitive => "fallow/high-cognitive-complexity",
            ExceededThreshold::Crap
            | ExceededThreshold::CyclomaticCrap
            | ExceededThreshold::CognitiveCrap
            | ExceededThreshold::All => "fallow/high-crap-score",
        };
        // Map finding severity to CodeClimate severity levels
        let severity = match finding.severity {
            crate::health_types::FindingSeverity::Critical => "critical",
            crate::health_types::FindingSeverity::High => "major",
            crate::health_types::FindingSeverity::Moderate => "minor",
        };
        let line_str = finding.line.to_string();
        let fp = fingerprint_hash(&[check_name, &path, &line_str, &finding.name]);
        issues.push(cc_issue(
            check_name,
            &description,
            severity,
            "Complexity",
            &path,
            Some(finding.line),
            &fp,
        ));
    }

    // Note: `production.hot_paths` and `production.signals` are
    // intentionally omitted from CodeClimate output. CodeClimate / GitLab
    // Code Quality issues are actionable findings; the
    // `hot-path-touched` signal is a PR-review heads-up and the
    // `signals[]` array is a programmatic decomposition of the verdict.
    // JSON consumers that need the full surface read those fields
    // directly from the JSON output.
    if let Some(ref production) = report.runtime_coverage {
        for finding in &production.findings {
            let path = cc_path(&finding.path, root);
            let check_name = match finding.verdict {
                crate::health_types::RuntimeCoverageVerdict::SafeToDelete => {
                    "fallow/runtime-safe-to-delete"
                }
                crate::health_types::RuntimeCoverageVerdict::ReviewRequired => {
                    "fallow/runtime-review-required"
                }
                crate::health_types::RuntimeCoverageVerdict::LowTraffic => {
                    "fallow/runtime-low-traffic"
                }
                crate::health_types::RuntimeCoverageVerdict::CoverageUnavailable => {
                    "fallow/runtime-coverage-unavailable"
                }
                crate::health_types::RuntimeCoverageVerdict::Active
                | crate::health_types::RuntimeCoverageVerdict::Unknown => "fallow/runtime-coverage",
            };
            let invocations_hint = finding.invocations.map_or_else(
                || "untracked".to_owned(),
                |hits| format!("{hits} invocations"),
            );
            let description = format!(
                "'{}' runtime coverage verdict: {} ({})",
                finding.function,
                finding.verdict.human_label(),
                invocations_hint,
            );
            // GitLab Code Quality renders MR inline annotations only for
            // blocker/critical/major/minor. Any non-cold verdict collapses to
            // "minor" — "info" is schema-valid but silently dropped from MR
            // annotations.
            let severity = match finding.verdict {
                crate::health_types::RuntimeCoverageVerdict::SafeToDelete => "critical",
                crate::health_types::RuntimeCoverageVerdict::ReviewRequired => "major",
                _ => "minor",
            };
            let fp = fingerprint_hash(&[
                check_name,
                &path,
                &finding.line.to_string(),
                &finding.function,
            ]);
            issues.push(cc_issue(
                check_name,
                &description,
                severity,
                // CodeClimate/GitLab Code Quality allows a fixed category set:
                // Bug Risk | Clarity | Compatibility | Complexity | Duplication
                // | Performance | Security | Style. Production-coverage
                // findings are a dead-code signal, so use "Bug Risk" — same
                // category used by static dead-code issues elsewhere.
                "Bug Risk",
                &path,
                Some(finding.line),
                &fp,
            ));
        }
    }

    if let Some(ref gaps) = report.coverage_gaps {
        for item in &gaps.files {
            let path = cc_path(&item.path, root);
            let description = format!(
                "File is runtime-reachable but has no test dependency path ({} value export{})",
                item.value_export_count,
                if item.value_export_count == 1 {
                    ""
                } else {
                    "s"
                },
            );
            let fp = fingerprint_hash(&["fallow/untested-file", &path]);
            issues.push(cc_issue(
                "fallow/untested-file",
                &description,
                "minor",
                "Coverage",
                &path,
                None,
                &fp,
            ));
        }

        for item in &gaps.exports {
            let path = cc_path(&item.path, root);
            let description = format!(
                "Export '{}' is runtime-reachable but never referenced by test-reachable modules",
                item.export_name
            );
            let line_str = item.line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/untested-export",
                &path,
                &line_str,
                &item.export_name,
            ]);
            issues.push(cc_issue(
                "fallow/untested-export",
                &description,
                "minor",
                "Coverage",
                &path,
                Some(item.line),
                &fp,
            ));
        }
    }

    serde_json::Value::Array(issues)
}

/// Print health analysis results in CodeClimate format.
pub(super) fn print_health_codeclimate(report: &HealthReport, root: &Path) -> ExitCode {
    let value = build_health_codeclimate(report, root);
    emit_json(&value, "CodeClimate")
}

/// Print health CodeClimate output with a per-issue `group` field.
///
/// Mirrors the dead-code grouped CodeClimate pattern
/// (`print_grouped_codeclimate`): build the standard payload first, then
/// post-process each issue to attach a `group` key derived from the
/// `OwnershipResolver`. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package without re-parsing the
/// project structure.
pub(super) fn print_grouped_health_codeclimate(
    report: &HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut value = build_health_codeclimate(report, root);

    if let Some(issues) = value.as_array_mut() {
        for issue in issues {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let group = grouping::resolve_owner(Path::new(path), Path::new(""), resolver);
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("group".to_string(), serde_json::Value::String(group));
        }
    }

    emit_json(&value, "CodeClimate")
}

/// Build CodeClimate JSON array from duplication analysis results.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "line numbers are bounded by source size"
)]
pub fn build_duplication_codeclimate(report: &DuplicationReport, root: &Path) -> serde_json::Value {
    let mut issues = Vec::new();

    for (i, group) in report.clone_groups.iter().enumerate() {
        // Content-based fingerprint: hash token_count + line_count + first 64 chars of fragment
        // This is stable across runs regardless of group ordering.
        let token_str = group.token_count.to_string();
        let line_count_str = group.line_count.to_string();
        let fragment_prefix: String = group
            .instances
            .first()
            .map(|inst| inst.fragment.chars().take(64).collect())
            .unwrap_or_default();

        for instance in &group.instances {
            let path = cc_path(&instance.file, root);
            let start_str = instance.start_line.to_string();
            let fp = fingerprint_hash(&[
                "fallow/code-duplication",
                &path,
                &start_str,
                &token_str,
                &line_count_str,
                &fragment_prefix,
            ]);
            issues.push(cc_issue(
                "fallow/code-duplication",
                &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                "minor",
                "Duplication",
                &path,
                Some(instance.start_line as u32),
                &fp,
            ));
        }
    }

    serde_json::Value::Array(issues)
}

/// Print duplication analysis results in CodeClimate format.
pub(super) fn print_duplication_codeclimate(report: &DuplicationReport, root: &Path) -> ExitCode {
    let value = build_duplication_codeclimate(report, root);
    emit_json(&value, "CodeClimate")
}

/// Print duplication CodeClimate output with a per-issue `group` field.
///
/// Mirrors [`print_grouped_health_codeclimate`]: each clone group is attributed
/// to its largest owner ([`super::dupes_grouping::largest_owner`]) and every
/// CodeClimate issue emitted for that clone group's instances carries the same
/// top-level `group` key. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package / directory without
/// re-parsing the project structure.
pub(super) fn print_grouped_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut value = build_duplication_codeclimate(report, root);

    // Build a flat lookup from each instance path -> primary owner. Every
    // instance of a clone group inherits the group's largest-owner key.
    use rustc_hash::FxHashMap;
    let mut path_to_owner: FxHashMap<String, String> = FxHashMap::default();
    for group in &report.clone_groups {
        let owner = super::dupes_grouping::largest_owner(group, root, resolver);
        for instance in &group.instances {
            let path = cc_path(&instance.file, root);
            path_to_owner.insert(path, owner.clone());
        }
    }

    if let Some(issues) = value.as_array_mut() {
        for issue in issues {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let group = path_to_owner
                .get(&path)
                .cloned()
                .unwrap_or_else(|| crate::codeowners::UNOWNED_LABEL.to_string());
            issue
                .as_object_mut()
                .expect("CodeClimate issue should be an object")
                .insert("group".to_string(), serde_json::Value::String(group));
        }
    }

    emit_json(&value, "CodeClimate")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use fallow_config::RulesConfig;
    use fallow_core::results::*;
    use std::path::PathBuf;

    /// Compute graduated severity for health findings based on threshold ratio.
    /// Kept for unit test coverage of the original CodeClimate severity model.
    fn health_severity(value: u16, threshold: u16) -> &'static str {
        if threshold == 0 {
            return "minor";
        }
        let ratio = f64::from(value) / f64::from(threshold);
        if ratio > 2.5 {
            "critical"
        } else if ratio > 1.5 {
            "major"
        } else {
            "minor"
        }
    }

    #[test]
    fn codeclimate_empty_results_produces_empty_array() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let arr = output.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn codeclimate_produces_array_of_issues() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        assert!(output.is_array());
        let arr = output.as_array().unwrap();
        // Should have at least one issue per type
        assert!(!arr.is_empty());
    }

    #[test]
    fn codeclimate_issue_has_required_fields() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let issue = &output.as_array().unwrap()[0];

        assert_eq!(issue["type"], "issue");
        assert_eq!(issue["check_name"], "fallow/unused-file");
        assert!(issue["description"].is_string());
        assert!(issue["categories"].is_array());
        assert!(issue["severity"].is_string());
        assert!(issue["fingerprint"].is_string());
        assert!(issue["location"].is_object());
        assert!(issue["location"]["path"].is_string());
        assert!(issue["location"]["lines"].is_object());
    }

    #[test]
    fn codeclimate_unused_file_severity_follows_rules() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });

        // Error severity -> major
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        assert_eq!(output[0]["severity"], "major");

        // Warn severity -> minor
        let rules = RulesConfig {
            unused_files: Severity::Warn,
            ..RulesConfig::default()
        };
        let output = build_codeclimate(&results, &root, &rules);
        assert_eq!(output[0]["severity"], "minor");
    }

    #[test]
    fn codeclimate_unused_export_has_line_number() {
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
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let issue = &output[0];
        assert_eq!(issue["location"]["lines"]["begin"], 10);
    }

    #[test]
    fn codeclimate_unused_file_line_defaults_to_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let issue = &output[0];
        assert_eq!(issue["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn codeclimate_paths_are_relative() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/deep/nested/file.ts"),
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let path = output[0]["location"]["path"].as_str().unwrap();
        assert_eq!(path, "src/deep/nested/file.ts");
        assert!(!path.starts_with("/project"));
    }

    #[test]
    fn codeclimate_re_export_label_in_description() {
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
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Re-export"));
    }

    #[test]
    fn codeclimate_unlisted_dep_one_issue_per_import_site() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".to_string(),
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
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let arr = output.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["check_name"], "fallow/unlisted-dependency");
        assert_eq!(arr[1]["check_name"], "fallow/unlisted-dependency");
    }

    #[test]
    fn codeclimate_duplicate_export_one_issue_per_location() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 10,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/b.ts"),
                    line: 20,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/c.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let arr = output.as_array().unwrap();
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn codeclimate_circular_dep_emits_chain_in_description() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
            length: 2,
            line: 3,
            col: 0,
            is_cross_package: false,
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("Circular dependency"));
        assert!(desc.contains("src/a.ts"));
        assert!(desc.contains("src/b.ts"));
    }

    #[test]
    fn codeclimate_fingerprints_are_deterministic() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output1 = build_codeclimate(&results, &root, &rules);
        let output2 = build_codeclimate(&results, &root, &rules);

        let fps1: Vec<&str> = output1
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        let fps2: Vec<&str> = output2
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        assert_eq!(fps1, fps2);
    }

    #[test]
    fn codeclimate_fingerprints_are_unique() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);

        let mut fps: Vec<&str> = output
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap())
            .collect();
        let original_len = fps.len();
        fps.sort_unstable();
        fps.dedup();
        assert_eq!(fps.len(), original_len, "fingerprints should be unique");
    }

    #[test]
    fn codeclimate_type_only_dep_has_correct_check_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        assert_eq!(output[0]["check_name"], "fallow/type-only-dependency");
        let desc = output[0]["description"].as_str().unwrap();
        assert!(desc.contains("zod"));
        assert!(desc.contains("type-only"));
    }

    #[test]
    fn codeclimate_dep_with_zero_line_omits_line_number() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 0,
            used_in_workspaces: Vec::new(),
        });
        let rules = RulesConfig::default();
        let output = build_codeclimate(&results, &root, &rules);
        // Line 0 -> begin defaults to 1
        assert_eq!(output[0]["location"]["lines"]["begin"], 1);
    }

    // ── fingerprint_hash tests ─────────────────────────────────────

    #[test]
    fn fingerprint_hash_different_inputs_differ() {
        let h1 = fingerprint_hash(&["a", "b"]);
        let h2 = fingerprint_hash(&["a", "c"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_order_matters() {
        let h1 = fingerprint_hash(&["a", "b"]);
        let h2 = fingerprint_hash(&["b", "a"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_separator_prevents_collision() {
        // "ab" + "c" should differ from "a" + "bc"
        let h1 = fingerprint_hash(&["ab", "c"]);
        let h2 = fingerprint_hash(&["a", "bc"]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn fingerprint_hash_is_16_hex_chars() {
        let h = fingerprint_hash(&["test"]);
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── severity_to_codeclimate ─────────────────────────────────────

    #[test]
    fn severity_error_maps_to_major() {
        assert_eq!(severity_to_codeclimate(Severity::Error), "major");
    }

    #[test]
    fn severity_warn_maps_to_minor() {
        assert_eq!(severity_to_codeclimate(Severity::Warn), "minor");
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn severity_off_maps_to_minor() {
        let _ = severity_to_codeclimate(Severity::Off);
    }

    // ── health_severity ─────────────────────────────────────────────

    #[test]
    fn health_severity_zero_threshold_returns_minor() {
        assert_eq!(health_severity(100, 0), "minor");
    }

    #[test]
    fn health_severity_at_threshold_returns_minor() {
        assert_eq!(health_severity(10, 10), "minor");
    }

    #[test]
    fn health_severity_1_5x_threshold_returns_minor() {
        assert_eq!(health_severity(15, 10), "minor");
    }

    #[test]
    fn health_severity_above_1_5x_returns_major() {
        assert_eq!(health_severity(16, 10), "major");
    }

    #[test]
    fn health_severity_at_2_5x_returns_major() {
        assert_eq!(health_severity(25, 10), "major");
    }

    #[test]
    fn health_severity_above_2_5x_returns_critical() {
        assert_eq!(health_severity(26, 10), "critical");
    }

    #[test]
    fn health_codeclimate_includes_coverage_gaps() {
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

        let output = build_health_codeclimate(&report, &root);
        let issues = output.as_array().unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0]["check_name"], "fallow/untested-file");
        assert_eq!(issues[0]["categories"][0], "Coverage");
        assert_eq!(issues[0]["location"]["path"], "src/app.ts");
        assert_eq!(issues[1]["check_name"], "fallow/untested-export");
        assert_eq!(issues[1]["location"]["lines"]["begin"], 12);
        assert!(
            issues[1]["description"]
                .as_str()
                .unwrap()
                .contains("loader")
        );
    }

    #[test]
    fn health_codeclimate_crap_only_uses_crap_check_name() {
        use crate::health_types::{FindingSeverity, HealthFinding, HealthReport, HealthSummary};
        let root = PathBuf::from("/project");
        let report = HealthReport {
            findings: vec![HealthFinding {
                path: root.join("src/untested.ts"),
                name: "risky".to_string(),
                line: 7,
                col: 0,
                cyclomatic: 10,
                cognitive: 10,
                line_count: 20,
                param_count: 1,
                exceeded: crate::health_types::ExceededThreshold::Crap,
                severity: FindingSeverity::High,
                crap: Some(60.0),
                coverage_pct: Some(25.0),
                coverage_tier: None,
            }],
            summary: HealthSummary {
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = build_health_codeclimate(&report, &root);
        let issues = json.as_array().unwrap();
        assert_eq!(issues[0]["check_name"], "fallow/high-crap-score");
        assert_eq!(issues[0]["severity"], "major");
        let description = issues[0]["description"].as_str().unwrap();
        assert!(description.contains("CRAP score"), "desc: {description}");
        assert!(description.contains("coverage 25%"), "desc: {description}");
    }
}
