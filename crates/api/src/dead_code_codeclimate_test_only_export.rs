//! CodeClimate issues for test-only exports.

use super::*;

pub(super) fn push_test_only_export_issues(
    issues: &mut Vec<CodeClimateIssue>,
    exports: &[fallow_types::output_dead_code::TestOnlyExportFinding],
    root: &Path,
    severity: Severity,
) {
    for entry in exports {
        let level = finding_codeclimate(entry, severity);
        let export = &entry.export;
        let path = cc_path(&export.path, root);
        let fp =
            codeclimate_fingerprint_hash(&["fallow/test-only-export", &path, &export.export_name]);
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/test-only-export",
                description: &export.message(),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: Some(export.line),
                fingerprint: &fp,
            },
        ));
    }
}
