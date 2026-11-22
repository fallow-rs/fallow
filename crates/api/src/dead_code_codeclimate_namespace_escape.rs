//! CodeClimate issues for namespace escapes.

use super::*;

pub(super) fn push_namespace_escape_issues(
    issues: &mut Vec<CodeClimateIssue>,
    escapes: &[fallow_types::output_dead_code::NamespaceEscapeFinding],
    root: &Path,
    severity: Severity,
) {
    for entry in escapes {
        let level = finding_codeclimate(entry, severity);
        let escape = &entry.escape;
        let path = cc_path(&escape.path, root);
        let module = cc_path(&escape.module, root);
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/namespace-escape",
            &path,
            &escape.local_name,
            &module,
        ]);
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/namespace-escape",
                description: &escape.message(&module),
                severity: level,
                category: "Clarity",
                path: &path,
                begin_line: Some(escape.line),
                fingerprint: &fp,
            },
        ));
    }
}
