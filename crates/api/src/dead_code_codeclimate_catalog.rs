//! CodeClimate issues for catalog entries, catalog references and dependency
//! overrides.

use super::*;

pub(super) fn push_unresolved_catalog_reference_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for finding in findings {
        let level = finding_codeclimate(finding, severity);
        let finding_id = finding.finding_id.as_deref();
        let introduced = finding.introduced;
        let finding = &finding.reference;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
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
        issues.push(dead_code_issue(
            finding_id,
            introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unresolved-catalog-reference",
                description: &description,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(finding.line),
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_misconfigured_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for finding in findings {
        let level = finding_codeclimate(finding, severity);
        let finding_id = finding.finding_id.as_deref();
        let introduced = finding.introduced;
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/misconfigured-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let description = format!(
            "Override `{}` -> `{}` is malformed: {}",
            finding.raw_key,
            finding.raw_value,
            finding.reason.describe(),
        );
        issues.push(dead_code_issue(
            finding_id,
            introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/misconfigured-dependency-override",
                description: &description,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(finding.line),
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_unused_catalog_entry_issues(
    issues: &mut Vec<CodeClimateIssue>,
    entries: &[fallow_types::output_dead_code::UnusedCatalogEntryFinding],
    root: &Path,
    severity: Severity,
) {
    if entries.is_empty() {
        return;
    }
    for entry in entries {
        let level = finding_codeclimate(entry, severity);
        let finding_id = entry.finding_id.as_deref();
        let introduced = entry.introduced;
        let entry = &entry.entry;
        let path = cc_path(&entry.path, root);
        let line_str = entry.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
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
        issues.push(dead_code_issue(
            finding_id,
            introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unused-catalog-entry",
                description: &description,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(entry.line),
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_empty_catalog_group_issues(
    issues: &mut Vec<CodeClimateIssue>,
    groups: &[fallow_types::output_dead_code::EmptyCatalogGroupFinding],
    root: &Path,
    severity: Severity,
) {
    if groups.is_empty() {
        return;
    }
    for group in groups {
        let level = finding_codeclimate(group, severity);
        let finding_id = group.finding_id.as_deref();
        let introduced = group.introduced;
        let group = &group.group;
        let path = cc_path(&group.path, root);
        let line_str = group.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/empty-catalog-group",
            &path,
            &line_str,
            &group.catalog_name,
        ]);
        issues.push(dead_code_issue(
            finding_id,
            introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/empty-catalog-group",
                description: &format!("Catalog group '{}' has no entries", group.catalog_name),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(group.line),
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_unused_dependency_override_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedDependencyOverrideFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for finding in findings {
        let level = finding_codeclimate(finding, severity);
        let finding_id = finding.finding_id.as_deref();
        let introduced = finding.introduced;
        let finding = &finding.entry;
        let path = cc_path(&finding.path, root);
        let line_str = finding.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/unused-dependency-override",
            &path,
            &line_str,
            finding.source.as_label(),
            &finding.raw_key,
        ]);
        let mut description = format!(
            "Override `{}` forces version `{}` but `{}` is not declared by any workspace package or resolved in the lockfile",
            finding.raw_key, finding.version_range, finding.target_package,
        );
        if let Some(hint) = &finding.hint {
            use std::fmt::Write as _;
            let _ = write!(description, " ({hint})");
        }
        issues.push(dead_code_issue(
            finding_id,
            introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unused-dependency-override",
                description: &description,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(finding.line),
                fingerprint: &fp,
            },
        ));
    }
}
