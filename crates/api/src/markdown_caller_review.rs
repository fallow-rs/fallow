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
            // A string value comes from project source, so it renders as
            // a code span, never as markdown.
            let summary = match &parameter.value {
                Some(value) => {
                    format!("every call passes {}", markdown_code_span(&value.display()))
                }
                None => parameter.summary(),
            };
            vec![format!(
                "- {} in {}: {} ({} calls). Callers: {}",
                markdown_code_span(&format!(
                    "{}({})",
                    parameter.function_name,
                    parameter.label()
                )),
                markdown_code_span(&format!("{}:{}", rel(&parameter.path), parameter.line)),
                summary,
                parameter.call_site_count,
                callers
            )]
        },
    );
}

#[cfg(test)]
mod tests {
    use fallow_types::results::{
        ConstantParameter, ConstantParameterFinding, ConstantParameterReason,
        ConstantParameterValue, ConstantParameterValueKind,
    };

    use super::*;

    #[test]
    fn a_string_value_renders_as_a_code_span() {
        let mut results = AnalysisResults::default();
        results
            .constant_parameters
            .push(ConstantParameterFinding::with_actions(ConstantParameter {
                path: "src/a.ts".into(),
                function_name: "f".into(),
                parameter_name: "p".into(),
                option_key: None,
                parameter_index: 0,
                line: 1,
                col: 11,
                reason: ConstantParameterReason::IdenticalArgument,
                evidence: None,
                value: Some(ConstantParameterValue {
                    kind: ConstantParameterValueKind::String,
                    text: "<img src=x> `tick`".into(),
                }),
                optional: false,
                has_default: false,
                condition_reads: 0,
                call_site_count: 3,
                inspected_call_sites: vec![],
                explanation: String::new(),
            }));
        let mut out = String::new();
        push_markdown_caller_review_sections(&mut out, &results, &|path| {
            path.display().to_string()
        });
        let expected = markdown_code_span("\"<img src=x> `tick`\"");
        assert!(
            out.contains(&format!("every call passes {expected}")),
            "{out}"
        );
    }
}
