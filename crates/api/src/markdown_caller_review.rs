//! Markdown sections for findings that list the inspected callers.

use std::fmt::Write as _;
use std::path::Path;

use fallow_output::markdown_code_span;
use fallow_types::results::AnalysisResults;

use super::markdown_section;

/// The most callers that one markdown line names.
const MAX_LISTED_CALLERS: usize = 3;

pub(super) fn push_markdown_caller_review_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
    markdown_section(
        out,
        &results.absent_component_props,
        "Absent optional component props",
        |finding| {
            let prop = &finding.prop;
            let callers = prop
                .inspected_call_sites
                .iter()
                .map(|site| markdown_code_span(&format!("{}:{}", rel(&site.path), site.line)))
                .collect::<Vec<_>>()
                .join(", ");
            vec![format!(
                "- {} in {} ({}; default: {}). {} Inspected callers: {}",
                markdown_code_span(&format!("{}.{}", prop.component_name, prop.prop_name)),
                markdown_code_span(&format!("{}:{}", rel(&prop.path), prop.line)),
                prop.framework,
                if prop.has_default {
                    "present"
                } else {
                    "absent"
                },
                prop.explanation,
                callers
            )]
        },
    );
    markdown_section(
        out,
        &results.constant_parameters,
        "Constant parameters",
        |finding| {
            let parameter = &finding.parameter;
            let mut callers = parameter
                .inspected_call_sites
                .iter()
                .take(MAX_LISTED_CALLERS)
                .map(|site| markdown_code_span(&format!("{}:{}", rel(&site.path), site.line)))
                .collect::<Vec<_>>()
                .join(", ");
            let listed = parameter.inspected_call_sites.len().min(MAX_LISTED_CALLERS);
            let more = (parameter.call_site_count as usize).saturating_sub(listed);
            if more > 0 {
                let _ = write!(callers, " (+{more} more)");
            }
            vec![format!(
                "- {} in {}: {} ({} calls). Callers: {}",
                markdown_code_span(&format!(
                    "{}({})",
                    parameter.function_name, parameter.parameter_name
                )),
                markdown_code_span(&format!("{}:{}", rel(&parameter.path), parameter.line)),
                parameter.summary(),
                parameter.call_site_count,
                callers
            )]
        },
    );
}
