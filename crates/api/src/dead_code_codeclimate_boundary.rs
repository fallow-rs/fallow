//! CodeClimate issues for boundary violations.

use super::*;

pub(super) fn push_boundary_violation_issues(
    issues: &mut Vec<CodeClimateIssue>,
    violations: &[fallow_types::output_dead_code::BoundaryViolationFinding],
    root: &Path,
    severity: Severity,
) {
    if violations.is_empty() {
        return;
    }
    for entry in violations {
        let level = finding_codeclimate(entry, severity);
        let v = &entry.violation;
        let path = cc_path(&v.from_path, root);
        let to = cc_path(&v.to_path, root);
        let fp = codeclimate_fingerprint_hash(&["fallow/boundary-violation", &path, &to]);
        let line = if v.line > 0 { Some(v.line) } else { None };
        let via = v
            .via_path
            .as_ref()
            .map_or_else(String::new, |via| format!(", via {}", cc_path(via, root)));
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/boundary-violation",
                description: &v
                    .internal_file_message(&|path| cc_path(path, root))
                    .map_or_else(
                        || {
                            format!(
                                "Boundary violation: {} -> {} ({} -> {}{})",
                                path, to, v.from_zone, v.to_zone, via
                            )
                        },
                        |message| format!("Boundary violation: {path}: {message}{via}"),
                    ),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: line,
                fingerprint: &fp,
            },
        ));
    }
}
