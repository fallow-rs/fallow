//! Manual review diagnostics for optional inputs absent from inspected callers.

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_types::output_dead_code::{AbsentComponentPropFinding, EffectiveSeverity};
use ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Uri,
};
use rustc_hash::FxHashMap;

use super::{doc_link_for_code, finding_data};
use crate::position::{PositionMapper, line_range_from_byte_col};

pub fn push_absent_prop_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.absent_component_props {
        if let Some(diagnostic) = absent_prop_diagnostic(finding, mapper)
            && let Some(uri) = Uri::from_file_path(&finding.prop.path)
        {
            map.entry(uri).or_default().push(diagnostic);
        }
    }
}

pub fn absent_prop_diagnostic(
    finding: &AbsentComponentPropFinding,
    mapper: &mut PositionMapper,
) -> Option<Diagnostic> {
    let prop = &finding.prop;
    Uri::from_file_path(&prop.path)?;
    let line = prop.line.saturating_sub(1);
    let (start, end) = mapper.utf16_col_span(&prop.path, line, prop.col, &prop.prop_name);
    let related = prop
        .inspected_call_sites
        .iter()
        .filter_map(|caller| {
            Some(DiagnosticRelatedInformation {
                location: Location {
                    uri: Uri::from_file_path(&caller.path)?,
                    range: line_range_from_byte_col(
                        mapper,
                        &caller.path,
                        caller.line.saturating_sub(1),
                        caller.col,
                    ),
                },
                message: format!(
                    "Known reachable caller inspected for '{}.{}'",
                    prop.component_name, prop.prop_name
                ),
            })
        })
        .collect();
    let default = if prop.has_default {
        " A declaration default is present."
    } else {
        ""
    };
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
        code: Some(NumberOrString::String("absent-component-prop".to_string())),
        code_description: doc_link_for_code("absent-component-prop"),
        message: format!(
            "Review optional prop '{}.{}' ({}). {}{default}",
            prop.component_name, prop.prop_name, prop.framework, prop.explanation
        ),
        related_information: Some(related),
        data: finding_data(finding.finding_id.as_deref()),
        ..Default::default()
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use fallow_types::results::{AbsentComponentProp, ComponentPropCallSite};

    pub fn candidate(path: std::path::PathBuf) -> AbsentComponentPropFinding {
        let mut finding = AbsentComponentPropFinding::with_actions(AbsentComponentProp {
            path,
            component_name: "Card".to_string(),
            prop_name: "highlight".to_string(),
            framework: "react".to_string(),
            line: 1,
            col: 4,
            has_default: true,
            inspected_call_sites: vec![],
            explanation: "Known reachable callers do not supply this optional prop. Review defaults and API intent before changing the component; static analysis does not prove runtime unreachability.".to_string(),
        });
        finding.finding_id = Some("dc1:absent-component-prop:0123456789abcdef".to_string());
        finding
    }

    #[test]
    fn candidate_has_manual_semantics_and_utf16_caller_locations() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("Card.tsx");
        let caller = project.path().join("App.tsx");
        std::fs::write(&path, "type Props = {\n/*😀*/highlight?: boolean\n};\n")
            .expect("declaration");
        std::fs::write(&caller, "const view = <>{/*😀*/}<Card /></>;\n").expect("caller");
        let mut finding = candidate(path);
        finding.prop.line = 2;
        finding.prop.col = 8;
        finding
            .prop
            .inspected_call_sites
            .push(ComponentPropCallSite {
                path: caller.clone(),
                line: 1,
                col: 25,
            });
        let diagnostic =
            absent_prop_diagnostic(&finding, &mut PositionMapper::default()).expect("diagnostic");
        assert_eq!(diagnostic.range.start.character, 6);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
        assert!(diagnostic.tags.is_none());
        assert!(
            diagnostic
                .message
                .contains("static analysis does not prove runtime unreachability")
        );
        assert!(diagnostic.message.contains("default"));
        assert_eq!(
            diagnostic.data.expect("identity")["findingId"],
            finding.finding_id.expect("id")
        );
        let related = diagnostic.related_information.expect("caller evidence");
        assert_eq!(related.len(), 1);
        assert_eq!(
            related[0].location.uri,
            Uri::from_file_path(caller).expect("uri")
        );
        assert_eq!(related[0].location.range.start.character, 23);
        assert!(related[0].message.contains("inspected"));
    }

    #[test]
    fn configured_error_remains_an_error_without_unnecessary_tag() {
        let mut finding = candidate(std::env::temp_dir().join("Card.tsx"));
        finding.effective_severity = Some(EffectiveSeverity::Error);
        let diagnostic =
            absent_prop_diagnostic(&finding, &mut PositionMapper::default()).expect("diagnostic");
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert!(diagnostic.tags.is_none());
    }
}
