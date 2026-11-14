//! CodeClimate issues for findings that list the inspected callers.

use std::path::Path;

use fallow_config::Severity;
use fallow_output::{CodeClimateIssue, CodeClimateIssueInput, codeclimate_fingerprint_hash};

use super::{cc_path, dead_code_issue, finding_codeclimate};

pub(super) fn push_absent_component_prop_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::AbsentComponentPropFinding],
    root: &Path,
    severity: Severity,
) {
    for finding in findings {
        let prop = &finding.prop;
        let path = cc_path(&prop.path, root);
        let fingerprint = codeclimate_fingerprint_hash(&[
            "fallow/absent-component-prop",
            &path,
            &prop.component_name,
            &prop.prop_name,
        ]);
        let description = format!(
            "Optional prop '{}.{}' is absent from inspected callers. {}",
            prop.component_name, prop.prop_name, prop.explanation
        );
        issues.push(dead_code_issue(
            finding.finding_id.as_deref(),
            finding.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/absent-component-prop",
                description: &description,
                severity: finding_codeclimate(finding, severity),
                category: "Clarity",
                path: &path,
                begin_line: Some(prop.line),
                fingerprint: &fingerprint,
            },
        ));
    }
}

pub(super) fn push_constant_parameter_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::results::ConstantParameterFinding],
    root: &Path,
    severity: Severity,
) {
    for finding in findings {
        let parameter = &finding.parameter;
        let path = cc_path(&parameter.path, root);
        let fingerprint = codeclimate_fingerprint_hash(&[
            "fallow/constant-parameter",
            &path,
            &parameter.function_name,
            &parameter.parameter_name,
        ]);
        let description = format!(
            "Parameter '{}' of '{}': {}. {}",
            parameter.label(),
            parameter.function_name,
            parameter.summary(),
            parameter.explanation
        );
        issues.push(dead_code_issue(
            finding.finding_id.as_deref(),
            finding.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/constant-parameter",
                description: &description,
                severity: finding_codeclimate(finding, severity),
                category: "Clarity",
                path: &path,
                begin_line: Some(parameter.line),
                fingerprint: &fingerprint,
            },
        ));
    }
}
