//! Manual review diagnostics for parameters that each visible call passes in
//! one way.

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_api::editor_results::ConstantParameterFinding;
use fallow_types::output_dead_code::EffectiveSeverity;
use ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Uri,
};
use rustc_hash::FxHashMap;

use super::{doc_link_for_code, finding_data};
use crate::position::{PositionMapper, line_range_from_byte_col};

pub fn push_constant_parameter_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.constant_parameters {
        if let Some(diagnostic) = constant_parameter_diagnostic(finding, mapper)
            && let Some(uri) = Uri::from_file_path(&finding.parameter.path)
        {
            map.entry(uri).or_default().push(diagnostic);
        }
    }
}

pub fn constant_parameter_diagnostic(
    finding: &ConstantParameterFinding,
    mapper: &mut PositionMapper,
) -> Option<Diagnostic> {
    let parameter = &finding.parameter;
    Uri::from_file_path(&parameter.path)?;
    let line = parameter.line.saturating_sub(1);
    let (start, end) = mapper.utf16_col_span(
        &parameter.path,
        line,
        parameter.col,
        &parameter.parameter_name,
    );
    let related = parameter
        .inspected_call_sites
        .iter()
        .filter_map(|call| {
            Some(DiagnosticRelatedInformation {
                location: Location {
                    uri: Uri::from_file_path(&call.path)?,
                    range: line_range_from_byte_col(
                        mapper,
                        &call.path,
                        call.line.saturating_sub(1),
                        call.col,
                    ),
                },
                message: format!("Inspected call of '{}'", parameter.function_name),
            })
        })
        .collect();
    Some(Diagnostic {
        range: Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        },
        severity: Some(match finding.effective_severity {
            Some(EffectiveSeverity::Error) => DiagnosticSeverity::ERROR,
            _ => DiagnosticSeverity::WARNING,
        }),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String("constant-parameter".to_string())),
        code_description: doc_link_for_code("constant-parameter"),
        message: format!(
            "Parameter '{}' of '{}': {} ({} calls). {}",
            parameter.label(),
            parameter.function_name,
            parameter.summary(),
            parameter.call_site_count,
            parameter.explanation
        ),
        related_information: Some(related),
        data: finding_data(finding.finding_id.as_deref()),
        ..Default::default()
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use fallow_api::editor_results::{
        ConstantParameter, ConstantParameterCallSite, ConstantParameterReason,
        ConstantParameterValue, ConstantParameterValueKind,
    };

    pub fn candidate(path: std::path::PathBuf) -> ConstantParameterFinding {
        let mut finding = ConstantParameterFinding::with_actions(ConstantParameter {
            path,
            function_name: "format".to_string(),
            parameter_name: "compact".to_string(),
            option_key: None,
            parameter_index: 1,
            line: 1,
            col: 23,
            reason: ConstantParameterReason::IdenticalArgument,
            evidence: None,
            value: Some(ConstantParameterValue {
                kind: ConstantParameterValueKind::Boolean,
                text: "true".to_string(),
            }),
            optional: false,
            has_default: false,
            condition_reads: 1,
            call_site_count: 3,
            inspected_call_sites: vec![],
            explanation: "Each visible call passes the same value for this parameter.".to_string(),
        });
        finding.finding_id = Some("dc1:constant-parameter:0123456789abcdef".to_string());
        finding
    }

    #[test]
    fn diagnostic_names_the_value_and_lists_the_calls() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("format.ts");
        let caller = project.path().join("main.ts");
        std::fs::write(&path, "export function format(compact: boolean) {}\n").expect("source");
        std::fs::write(&caller, "/*😀*/format(true);\n").expect("caller");
        let mut finding = candidate(path);
        finding.parameter.inspected_call_sites = vec![ConstantParameterCallSite {
            path: caller,
            line: 1,
            col: 8,
        }];
        let diagnostic = constant_parameter_diagnostic(&finding, &mut PositionMapper::default())
            .expect("diagnostic");
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String("constant-parameter".to_string()))
        );
        assert_eq!(diagnostic.range.start.character, 23);
        assert_eq!(diagnostic.range.end.character, 30);
        assert!(
            diagnostic
                .message
                .contains("every call passes true (3 calls)")
        );
        let related = diagnostic.related_information.expect("related");
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].location.range.start.character, 6);
    }
}
