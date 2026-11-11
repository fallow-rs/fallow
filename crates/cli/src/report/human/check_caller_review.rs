//! Human sections for findings that list the inspected callers.

use super::*;
use crate::report::Level;
use fallow_types::results::ConstantParameterFinding;
use std::fmt::Write as _;

/// The most callers that one constant parameter line names.
const MAX_LISTED_CALLERS: usize = 3;

pub(super) fn push_caller_review_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.absent_component_props,
        title: "Absent optional component props",
        level: if results
            .absent_component_props
            .iter()
            .any(|finding| finding.effective_severity == Some(EffectiveSeverity::Error))
        {
            crate::report::Level::Error
        } else {
            crate::report::Level::Warn
        },
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |finding: &AbsentComponentPropFinding| finding.prop.path.as_path(),
        format_detail: &|finding: &AbsentComponentPropFinding| {
            let prop = &finding.prop;
            let callers = prop
                .inspected_call_sites
                .iter()
                .map(|site| format!("{}:{}", format_display_path(&site.path, root), site.line))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                ":{} {}.{} (default: {})\n    {}\n    Inspected callers: {}",
                prop.line,
                prop.component_name,
                prop.prop_name,
                if prop.has_default {
                    "present"
                } else {
                    "absent"
                },
                prop.explanation,
                callers
            )
        },
    });
    push_constant_parameter_section(lines, results, root);
}

fn caller_review_level(severities: impl Iterator<Item = Option<EffectiveSeverity>>) -> Level {
    let mut severities = severities;
    if severities.any(|severity| severity == Some(EffectiveSeverity::Error)) {
        Level::Error
    } else {
        Level::Warn
    }
}

/// Constant parameters, most condition reads first: a constant that a
/// condition tests points at a branch that cannot run.
fn push_constant_parameter_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
) {
    let mut items: Vec<ConstantParameterFinding> = results.constant_parameters.clone();
    items.sort_by(|a, b| {
        b.parameter
            .condition_reads
            .cmp(&a.parameter.condition_reads)
            .then(
                b.parameter
                    .call_site_count
                    .cmp(&a.parameter.call_site_count),
            )
            .then(a.parameter.path.cmp(&b.parameter.path))
            .then(a.parameter.line.cmp(&b.parameter.line))
    });
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &items,
        title: "Constant parameters",
        level: caller_review_level(items.iter().map(|finding| finding.effective_severity)),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |finding: &ConstantParameterFinding| finding.parameter.path.as_path(),
        format_detail: &|finding: &ConstantParameterFinding| {
            format_constant_parameter(finding, root)
        },
    });
}

fn format_constant_parameter(finding: &ConstantParameterFinding, root: &Path) -> String {
    let parameter = &finding.parameter;
    let reads = match parameter.condition_reads {
        0 => String::new(),
        1 => ", 1 condition read".to_string(),
        count => format!(", {count} condition reads"),
    };
    let mut callers = parameter
        .inspected_call_sites
        .iter()
        .take(MAX_LISTED_CALLERS)
        .map(|site| format!("{}:{}", format_display_path(&site.path, root), site.line))
        .collect::<Vec<_>>()
        .join(", ");
    let listed = parameter.inspected_call_sites.len().min(MAX_LISTED_CALLERS);
    let more = (parameter.call_site_count as usize).saturating_sub(listed);
    if more > 0 {
        let _ = write!(callers, " (+{more} more)");
    }
    format!(
        ":{} {}({}): {} ({} calls{reads})\n    Callers: {}",
        parameter.line,
        parameter.function_name,
        parameter.label(),
        parameter.summary(),
        parameter.call_site_count,
        callers
    )
}
