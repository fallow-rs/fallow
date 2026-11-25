//! Line-level suppression of test-only exports.

use crate::{
    diagnostics::test_only_exports::test_only_export_diagnostic, position::PositionMapper,
};
use fallow_api::EditorAnalysisResults as AnalysisResults;
use ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use std::path::Path;

/// Offer a suppression comment above the export line only for a saved,
/// current analysed source. The request boundary supplies the source only
/// when the captured document version still matches.
#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes uses std HashMap"
)]
#[must_use]
pub fn build_suppress_test_only_export_actions(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor: &Range,
    live_source: &str,
) -> Vec<CodeActionOrCommand> {
    if results.test_only_exports.is_empty() {
        return Vec::new();
    }
    if crate::position::read_document_text(file_path).is_none_or(|saved| saved != live_source) {
        return Vec::new();
    }
    let file_lines: Vec<_> = live_source.lines().collect();
    let mut mapper = PositionMapper::default();
    results
        .test_only_exports
        .iter()
        .filter_map(|finding| {
            let export = &finding.export;
            let line = export.line.saturating_sub(1);
            if export.path != file_path || line < cursor.start.line || line > cursor.end.line {
                return None;
            }
            let content = file_lines.get(line as usize)?;
            if !content.contains(export.export_name.as_str()) {
                return None;
            }
            let diagnostic = test_only_export_diagnostic(finding, &mut mapper);
            let indent: String = content.chars().take_while(|c| c.is_whitespace()).collect();
            let position = Position { line, character: 0 };
            let edit = TextEdit {
                range: Range {
                    start: position,
                    end: position,
                },
                new_text: format!("{indent}// fallow-ignore-next-line test-only-export\n"),
            };
            let changes = std::collections::HashMap::from([(uri.clone(), vec![edit])]);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Keep '{}' as an export for tests (suppress on this line)",
                    export.export_name
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
