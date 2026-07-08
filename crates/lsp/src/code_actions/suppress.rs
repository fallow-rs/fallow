//! Suppress code actions for security candidates (issue #891).
//!
//! Security findings are CANDIDATES, not confirmed bugs, so the actions are
//! framed as "dismiss a candidate", never "fix" / "ignore" / "silence". Both
//! actions are ADDITIVE (they insert a `// fallow-ignore-*` comment), unlike the
//! destructive remove/delete actions in `quick_fix.rs`, so re-validation only
//! confirms the anchor line still exists rather than re-parsing a declaration.
//!
//! The default (first) action is line-level, the conservative scope: it
//! dismisses exactly the candidate the user looked at. A file-level action
//! follows, clearly labeled, deduped to one per kind.

#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes is a std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;

#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use ls_types::*;
use rustc_hash::FxHashSet;

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_api::editor_results::SecurityFindingKind;

use crate::diagnostics::security::{security_diagnostic, security_label, security_token};
use crate::position::PositionMapper;

/// Build suppress code actions for security candidates under the cursor.
///
/// A file-level `// fallow-ignore-file <token>` dismissal is offered once per
/// distinct kind (both kinds honor file-level suppression). A line-level
/// `// fallow-ignore-next-line <token>` dismissal is offered ONLY for
/// `TaintedSink`: the `ClientServerLeak` detector honors only file-level
/// suppression (`analyze/security/mod.rs`), so a line-level marker would be a
/// dead no-op for it (the squiggle would reappear on the next analysis pass).
/// `TaintedSink` honors both (`analyze/security/tainted_sink.rs`).
#[derive(Clone, Copy)]
pub struct SuppressSecurityActionInput<'a> {
    pub results: &'a AnalysisResults,
    pub file_path: &'a Path,
    pub uri: &'a Uri,
    pub cursor_range: &'a Range,
    pub file_lines: &'a [&'a str],
}

impl<'a> SuppressSecurityActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        file_path: &'a Path,
        uri: &'a Uri,
        cursor_range: &'a Range,
        file_lines: &'a [&'a str],
    ) -> Self {
        Self {
            results,
            file_path,
            uri,
            cursor_range,
            file_lines,
        }
    }
}

pub fn build_suppress_security_actions(
    input: SuppressSecurityActionInput<'_>,
) -> Vec<CodeActionOrCommand> {
    let SuppressSecurityActionInput {
        results,
        file_path,
        uri,
        cursor_range,
        file_lines,
    } = input;
    let mut actions = Vec::new();
    let mut file_level_tokens: FxHashSet<&'static str> = FxHashSet::default();
    let mut position_mapper = PositionMapper::default();

    for finding in &results.security_findings {
        if finding.path != file_path {
            continue;
        }

        let finding_line = finding.line.saturating_sub(1);
        if finding_line < cursor_range.start.line || finding_line > cursor_range.end.line {
            continue;
        }

        // Additive insert: only confirm the anchor line still exists in the
        // live buffer (the destructive-edit re-validation in `quick_fix.rs` is
        // not needed because we never overwrite user text).
        let Some(line_content) = file_lines.get(finding_line as usize).copied() else {
            continue;
        };

        let token = security_token(finding.kind);
        let label = security_label(finding);
        let linked = security_diagnostic(finding, &mut position_mapper);

        // Line-level: only for TaintedSink (the only kind whose detector honors
        // line-level suppression). Match the anchor's indentation so the
        // inserted comment sits flush with the code it suppresses.
        if matches!(finding.kind, SecurityFindingKind::TaintedSink) {
            let indent: String = line_content
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            actions.push(suppress_action(
                format!("Dismiss this security candidate on this line ({label})"),
                uri,
                insert_before(
                    finding_line,
                    format!("{indent}// fallow-ignore-next-line {token}\n"),
                ),
                linked.clone(),
            ));
        }

        // File-level: one per distinct kind, inserted at the top of the file.
        // Both kinds honor file-level suppression.
        if file_level_tokens.insert(token) {
            actions.push(suppress_action(
                format!("Dismiss this security candidate type in this file ({label})"),
                uri,
                insert_before(0, format!("// fallow-ignore-file {token}\n")),
                linked,
            ));
        }
    }

    actions
}

#[cfg(test)]
fn build_suppress_security_actions_for_test(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    build_suppress_security_actions(SuppressSecurityActionInput::new(
        results,
        file_path,
        uri,
        cursor_range,
        file_lines,
    ))
}

/// A zero-width insertion `TextEdit` at the start of `line`.
fn insert_before(line: u32, new_text: String) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position { line, character: 0 },
            end: Position { line, character: 0 },
        },
        new_text,
    }
}

/// Wrap a single insertion edit + its linked diagnostic into a quick-fix action.
#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes is a std HashMap"
)]
fn suppress_action(
    title: String,
    uri: &Uri,
    edit: TextEdit,
    linked: Diagnostic,
) -> CodeActionOrCommand {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![linked]),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_api::editor_results::{SecurityFinding, SecurityFindingKind, SecuritySeverity};

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn sink(path: PathBuf, line: u32) -> SecurityFinding {
        SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path,
            line,
            col: 2,
            evidence: "sink".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: None,
            runtime: None,
        }
    }

    fn leak(path: PathBuf, line: u32) -> SecurityFinding {
        SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path,
            line,
            col: 0,
            evidence: "leak".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: None,
            runtime: None,
        }
    }

    fn action_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
        actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(action) => action.title.clone(),
                CodeActionOrCommand::Command(cmd) => cmd.title.clone(),
            })
            .collect()
    }

    fn first_edit_text(action: &CodeActionOrCommand) -> String {
        let CodeActionOrCommand::CodeAction(action) = action else {
            panic!("expected a CodeAction");
        };
        let changes = action
            .edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .expect("edit has changes");
        changes.values().next().unwrap()[0].new_text.clone()
    }

    #[test]
    fn offers_line_and_file_suppress_for_candidate_under_cursor() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let uri = Uri::from_file_path(&path).unwrap();
        let mut results = AnalysisResults::default();
        results.security_findings.push(sink(path.clone(), 3));
        let file_lines = vec!["line1", "line2", "  doRender();", "line4"];
        let cursor = Range {
            start: Position {
                line: 2,
                character: 0,
            },
            end: Position {
                line: 2,
                character: 0,
            },
        };

        let actions =
            build_suppress_security_actions_for_test(&results, &path, &uri, &cursor, &file_lines);
        let titles = action_titles(&actions);
        assert_eq!(titles.len(), 2);
        assert!(titles[0].contains("Dismiss this security candidate on this line"));
        assert!(titles[1].contains("Dismiss this security candidate type in this file"));
        // Indentation is matched on the line-level insert.
        assert_eq!(
            first_edit_text(&actions[0]),
            "  // fallow-ignore-next-line security-sink\n"
        );
        assert_eq!(
            first_edit_text(&actions[1]),
            "// fallow-ignore-file security-sink\n"
        );
    }

    #[test]
    fn file_level_action_deduped_per_kind() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let uri = Uri::from_file_path(&path).unwrap();
        let mut results = AnalysisResults::default();
        results.security_findings.push(sink(path.clone(), 2));
        results.security_findings.push(sink(path.clone(), 3));
        let file_lines = vec!["a", "b", "c", "d"];
        let cursor = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 10,
                character: 0,
            },
        };

        let actions =
            build_suppress_security_actions_for_test(&results, &path, &uri, &cursor, &file_lines);
        // Two line-level (one per finding) + ONE file-level (deduped).
        let file_level = action_titles(&actions)
            .iter()
            .filter(|t| t.contains("type in this file"))
            .count();
        assert_eq!(file_level, 1);
        assert_eq!(actions.len(), 3);
    }

    #[test]
    fn client_server_leak_offers_only_file_level() {
        // ClientServerLeak honors only file-level suppression, so a line-level
        // dismiss would be a dead no-op. Only the file-level action is offered.
        let root = test_root();
        let path = root.join("src/leak.ts");
        let uri = Uri::from_file_path(&path).unwrap();
        let mut results = AnalysisResults::default();
        results.security_findings.push(leak(path.clone(), 1));
        let file_lines = vec!["export { SECRET } from './server';", "b"];
        let cursor = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 0,
            },
        };

        let actions =
            build_suppress_security_actions_for_test(&results, &path, &uri, &cursor, &file_lines);
        let titles = action_titles(&actions);
        assert_eq!(titles.len(), 1);
        assert!(titles[0].contains("type in this file"));
        assert!(!titles.iter().any(|t| t.contains("on this line")));
        assert_eq!(
            first_edit_text(&actions[0]),
            "// fallow-ignore-file security-client-server-leak\n"
        );
    }

    #[test]
    fn no_actions_outside_cursor_range() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let uri = Uri::from_file_path(&path).unwrap();
        let mut results = AnalysisResults::default();
        results.security_findings.push(sink(path.clone(), 50));
        let file_lines = vec!["a", "b", "c"];
        let cursor = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 0,
            },
        };

        let actions =
            build_suppress_security_actions_for_test(&results, &path, &uri, &cursor, &file_lines);
        assert!(actions.is_empty());
    }
}
