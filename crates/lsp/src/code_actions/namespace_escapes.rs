//! Line-level suppression of namespace escapes.

use crate::{
    diagnostics::namespace_escapes::namespace_escape_diagnostic, position::PositionMapper,
};
use fallow_api::EditorAnalysisResults as AnalysisResults;
use ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use std::path::Path;

/// Offer a suppression comment above the import line only for a saved,
/// current analysed source. The request boundary supplies the source only
/// when the captured document version still matches.
#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes uses std HashMap"
)]
#[must_use]
pub fn build_suppress_namespace_escape_actions(
    results: &AnalysisResults,
    root: &Path,
    file_path: &Path,
    uri: &Uri,
    cursor: &Range,
    live_source: &str,
) -> Vec<CodeActionOrCommand> {
    if results.namespace_escapes.is_empty() {
        return Vec::new();
    }
    if crate::position::read_document_text(file_path).is_none_or(|saved| saved != live_source) {
        return Vec::new();
    }
    let file_lines: Vec<_> = live_source.lines().collect();
    let mut mapper = PositionMapper::default();
    results
        .namespace_escapes
        .iter()
        .filter_map(|finding| {
            let escape = &finding.escape;
            let line = escape.line.saturating_sub(1);
            if escape.path != file_path || line < cursor.start.line || line > cursor.end.line {
                return None;
            }
            let content = file_lines.get(line as usize)?;
            if !content.contains(escape.local_name.as_str()) {
                return None;
            }
            let diagnostic = namespace_escape_diagnostic(finding, root, &mut mapper);
            let indent: String = content.chars().take_while(|c| c.is_whitespace()).collect();
            let position = Position { line, character: 0 };
            let edit = TextEdit {
                range: Range {
                    start: position,
                    end: position,
                },
                new_text: format!("{indent}// fallow-ignore-next-line namespace-escape\n"),
            };
            let changes = std::collections::HashMap::from([(uri.clone(), vec![edit])]);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Keep namespace '{}' as a whole object (suppress on this line)",
                    escape.local_name
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
        NamespaceEscape, NamespaceEscapeFinding, NamespaceEscapeReason,
    };

    fn escape(path: std::path::PathBuf, module: std::path::PathBuf) -> NamespaceEscapeFinding {
        NamespaceEscapeFinding::with_actions(NamespaceEscape {
            path,
            line: 2,
            col: 9,
            local_name: "Formats".to_string(),
            module,
            hidden_exports: 2,
            reason: NamespaceEscapeReason::WholeObjectUse,
        })
    }

    #[test]
    fn offers_a_comment_above_the_import_line() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("app.ts");
        let source = "// header\n  import * as Formats from './formats';\n";
        std::fs::write(&path, source).expect("source");
        let mut results = AnalysisResults::default();
        results
            .namespace_escapes
            .push(escape(path.clone(), project.path().join("formats.ts")));
        let uri = Uri::from_file_path(&path).expect("uri");
        let cursor = Range {
            start: Position {
                line: 1,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 0,
            },
        };
        let actions = build_suppress_namespace_escape_actions(
            &results,
            project.path(),
            &path,
            &uri,
            &cursor,
            source,
        );
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
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(
            edits[0].new_text,
            "  // fallow-ignore-next-line namespace-escape\n"
        );
        let diagnostics = action.diagnostics.as_ref().expect("diagnostics");
        assert_eq!(
            diagnostics[0].message,
            "namespace Formats is used as a whole value; 2 exports of formats.ts are not checked"
        );

        let stale = build_suppress_namespace_escape_actions(
            &results,
            project.path(),
            &path,
            &uri,
            &cursor,
            "changed",
        );
        assert!(stale.is_empty());
    }

    #[test]
    fn offers_nothing_outside_the_cursor_lines() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("app.ts");
        let source = "// header\nimport * as Formats from './formats';\n";
        std::fs::write(&path, source).expect("source");
        let mut results = AnalysisResults::default();
        results
            .namespace_escapes
            .push(escape(path.clone(), project.path().join("formats.ts")));
        let uri = Uri::from_file_path(&path).expect("uri");
        let actions = build_suppress_namespace_escape_actions(
            &results,
            project.path(),
            &path,
            &uri,
            &Range::default(),
            source,
        );
        assert!(actions.is_empty());
    }
}
