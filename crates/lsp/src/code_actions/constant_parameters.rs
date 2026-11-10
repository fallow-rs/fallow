//! Line-level dismissal of constant parameter candidates.

use crate::{
    diagnostics::constant_parameters::constant_parameter_diagnostic, position::PositionMapper,
};
use fallow_api::EditorAnalysisResults as AnalysisResults;
use ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use std::path::Path;

/// Offer a comment above the parameter only for a saved, current analysed
/// source. The request boundary supplies the source only when the captured
/// document version still matches.
#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes uses std HashMap"
)]
#[must_use]
pub fn build_suppress_constant_parameter_actions(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor: &Range,
    live_source: &str,
) -> Vec<CodeActionOrCommand> {
    if results.constant_parameters.is_empty() {
        return Vec::new();
    }
    if crate::position::read_document_text(file_path).is_none_or(|saved| saved != live_source) {
        return Vec::new();
    }
    let file_lines: Vec<_> = live_source.lines().collect();
    let mut mapper = PositionMapper::default();
    results
        .constant_parameters
        .iter()
        .filter_map(|finding| {
            let parameter = &finding.parameter;
            let line = parameter.line.saturating_sub(1);
            if parameter.path != file_path || line < cursor.start.line || line > cursor.end.line {
                return None;
            }
            let content = file_lines.get(line as usize)?;
            let start = usize::try_from(parameter.col).ok()?;
            let end = start.checked_add(parameter.parameter_name.len())?;
            if content.get(start..end)? != parameter.parameter_name {
                return None;
            }
            let diagnostic = constant_parameter_diagnostic(finding, &mut mapper)?;
            let indent: String = content.chars().take_while(|c| c.is_whitespace()).collect();
            let position = Position { line, character: 0 };
            let edit = TextEdit {
                range: Range {
                    start: position,
                    end: position,
                },
                new_text: format!("{indent}// fallow-ignore-next-line constant-parameter\n"),
            };
            let changes = std::collections::HashMap::from([(uri.clone(), vec![edit])]);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Keep parameter '{}' of '{}' (suppress on this line)",
                    parameter.parameter_name, parameter.function_name
                ),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_api::editor_results::{
        ConstantParameter, ConstantParameterFinding, ConstantParameterReason,
    };

    fn candidate(path: std::path::PathBuf) -> ConstantParameterFinding {
        ConstantParameterFinding::with_actions(ConstantParameter {
            path,
            function_name: "format".to_string(),
            parameter_name: "compact".to_string(),
            parameter_index: 0,
            line: 1,
            col: 0,
            reason: ConstantParameterReason::IdenticalArgument,
            value: None,
            optional: false,
            has_default: false,
            condition_reads: 0,
            call_site_count: 3,
            inspected_call_sites: vec![],
            explanation: String::new(),
        })
    }

    #[test]
    fn offers_a_comment_above_the_parameter_line() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("format.ts");
        let source = "  export function format(compact: boolean) {}\n";
        std::fs::write(&path, source).expect("source");
        let mut finding = candidate(path.clone());
        finding.parameter.col = 25;
        let mut results = AnalysisResults::default();
        results.constant_parameters.push(finding);
        let uri = Uri::from_file_path(&path).expect("uri");
        let cursor = Range::default();
        let actions =
            build_suppress_constant_parameter_actions(&results, &path, &uri, &cursor, source);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("code action");
        };
        let edits = &action
            .edit
            .as_ref()
            .expect("edit")
            .changes
            .as_ref()
            .expect("changes")[&uri];
        assert_eq!(
            edits[0].new_text,
            "  // fallow-ignore-next-line constant-parameter\n"
        );

        let stale =
            build_suppress_constant_parameter_actions(&results, &path, &uri, &cursor, "changed");
        assert!(stale.is_empty());
    }
}
