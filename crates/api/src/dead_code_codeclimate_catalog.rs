//! CodeClimate issues for catalog references and dependency overrides.

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
