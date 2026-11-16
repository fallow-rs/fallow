//! CodeClimate issues for duplicate exports.

use super::*;

pub(super) fn push_duplicate_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    dups: &[fallow_types::output_dead_code::DuplicateExportFinding],
    root: &Path,
    severity: Severity,
) {
    if dups.is_empty() {
        return;
    }
    for dup in dups {
        let level = finding_codeclimate(dup, severity);
        // The finding id of a duplicate export holds the set of its paths, so
        // one more or one less location gives the finding a new id. Each
        // issue is one location, so its fingerprint uses the part of the id
        // that stays: the rule and the export name. The location discriminator
        // adds the path. A saved finding without an id keeps the legacy
        // fingerprint.
        let location_identity = dup
            .finding_id
            .is_some()
            .then(|| dead_code_finding_id("duplicate-export", &[&dup.export.export_name]));
        let finding_id = location_identity.as_deref();
        let introduced = dup.introduced;
        let dup = &dup.export;
        let locations: Vec<(String, u32, u32)> = dup
            .locations
            .iter()
            .map(|loc| (cc_path(&loc.path, root), loc.line, loc.col))
            .collect();
        let discriminators = location_discriminators(&locations);
        for ((path, line, _), discriminator) in locations.iter().zip(&discriminators) {
            let line_str = line.to_string();
            let discriminator: Vec<&str> = discriminator.iter().map(String::as_str).collect();
            let fp = codeclimate_fingerprint_hash(&[
                "fallow/duplicate-export",
                path,
                &line_str,
                &dup.export_name,
            ]);
            issues.push(dead_code_issue(
                finding_id,
                introduced,
                &discriminator,
                CodeClimateIssueInput {
                    check_name: "fallow/duplicate-export",
                    description: &format!(
                        "Export '{}' appears in multiple modules",
                        dup.export_name
                    ),
                    severity: level,
                    category: "Bug Risk",
                    path,
                    begin_line: Some(*line),
                    fingerprint: &fp,
                },
            ));
        }
    }
}
