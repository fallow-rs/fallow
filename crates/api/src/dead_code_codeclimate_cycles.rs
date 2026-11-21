//! CodeClimate issues for circular dependencies, re-export cycles and
//! package cycles.

use fallow_output::{CodeClimateLines, CodeClimateLocation};

use super::*;

pub(super) fn push_circular_dep_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::CircularDependencyFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    for entry in cycles {
        let level = finding_codeclimate(entry, severity);
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let fp = codeclimate_fingerprint_hash(&["fallow/circular-dependency", &chain_str]);
        let line = if cycle.line > 0 {
            Some(cycle.line)
        } else {
            None
        };
        let mut issue = dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/circular-dependency",
                description: &format!(
                    "Circular dependency{}: {}",
                    if cycle.is_cross_package {
                        " (cross-package)"
                    } else {
                        ""
                    },
                    chain.join(" \u{2192} ")
                ),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: line,
                fingerprint: &fp,
            },
        );
        // The imports of the other member files. A change to any member file
        // keeps the cycle in a diff-scoped analysis, and the PR summary needs
        // these files to keep it too.
        issue.other_locations = cycle
            .edges
            .iter()
            .skip(1)
            .map(|edge| point_location(cc_path(&edge.path, root), edge.line))
            .collect();
        issues.push(issue);
    }
}

/// A location on one line. A file without a known line (an edge of an old
/// baseline, or a re-export cycle member) points at line 1: the location
/// names the file, which the diff filter of the PR summary needs.
fn point_location(path: String, line: u32) -> CodeClimateLocation {
    CodeClimateLocation {
        path,
        lines: CodeClimateLines {
            begin: line.max(1),
            end: None,
        },
    }
}

pub(super) fn push_re_export_cycle_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::ReExportCycleFinding],
    root: &Path,
    severity: Severity,
) {
    if cycles.is_empty() {
        return;
    }
    for entry in cycles {
        let level = finding_codeclimate(entry, severity);
        let cycle = &entry.cycle;
        let Some(first) = cycle.files.first() else {
            continue;
        };
        let path = cc_path(first, root);
        let chain: Vec<String> = cycle.files.iter().map(|f| cc_path(f, root)).collect();
        let chain_str = chain.join(":");
        let kind_token = match cycle.kind {
            fallow_types::results::ReExportCycleKind::SelfLoop => "self-loop",
            fallow_types::results::ReExportCycleKind::MultiNode => "multi-node",
        };
        let kind_tag = match cycle.kind {
            fallow_types::results::ReExportCycleKind::SelfLoop => " (self-loop)",
            fallow_types::results::ReExportCycleKind::MultiNode => "",
        };
        let fp = codeclimate_fingerprint_hash(&["fallow/re-export-cycle", kind_token, &chain_str]);
        let mut issue = dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/re-export-cycle",
                description: &format!("Re-export cycle{}: {}", kind_tag, chain.join(" <-> ")),
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: (cycle.line > 0).then_some(cycle.line),
                fingerprint: &fp,
            },
        );
        issue.other_locations = chain
            .iter()
            .skip(1)
            .map(|member| point_location(member.clone(), 0))
            .collect();
        issues.push(issue);
    }
}

pub(super) fn push_package_cycle_issues(
    issues: &mut Vec<CodeClimateIssue>,
    cycles: &[fallow_types::output_dead_code::PackageCycleFinding],
    root: &Path,
    severity: Severity,
) {
    for entry in cycles {
        let level = finding_codeclimate(entry, severity);
        let cycle = &entry.cycle;
        let Some(anchor) = cycle.edges.first() else {
            continue;
        };
        let path = cc_path(&anchor.path, root);
        let packages = cycle.packages.join(":");
        let fp = codeclimate_fingerprint_hash(&["fallow/package-cycle", &packages]);
        let note = if cycle.group_truncated {
            format!(
                " ({})",
                fallow_types::results::PackageCycle::GROUP_TRUNCATED_NOTE
            )
        } else {
            String::new()
        };
        let mut issue = build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/package-cycle",
            description: &format!("Package cycle: {}{note}", cycle.chain(" \u{2192} ")),
            severity: level,
            category: "Bug Risk",
            path: &path,
            begin_line: (anchor.line > 0).then_some(anchor.line),
            fingerprint: &fp,
        });
        issue.audit = entry
            .introduced
            .map(|AuditIntroduced(introduced)| audit_review_context(introduced));
        issue.other_locations = cycle
            .edges
            .iter()
            .skip(1)
            .map(|edge| point_location(cc_path(&edge.path, root), edge.line))
            .collect();
        issues.push(issue);
    }
}
