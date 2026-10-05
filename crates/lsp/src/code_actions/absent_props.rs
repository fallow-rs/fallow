//! Additive declaration-line dismissal of absent optional prop candidates.

use crate::{diagnostics::absent_props::absent_prop_diagnostic, position::PositionMapper};
use fallow_api::EditorAnalysisResults as AnalysisResults;
use ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use std::path::Path;

/// Offer a declaration-line comment only for a saved, current analysed source.
///
/// The request boundary supplies source only when the captured document version
/// still matches. An unsaved buffer must wait for save and analysis.
#[expect(
    clippy::disallowed_types,
    reason = "ls_types WorkspaceEdit.changes uses std HashMap"
)]
#[must_use]
pub fn build_suppress_absent_prop_actions(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor: &Range,
    live_source: &str,
) -> Vec<CodeActionOrCommand> {
    if results.absent_component_props.is_empty() {
        return Vec::new();
    }
    if !std::fs::read_to_string(file_path).is_ok_and(|saved| saved == live_source) {
        return Vec::new();
    }
    let file_lines: Vec<_> = live_source.lines().collect();
    let mut mapper = PositionMapper::default();
    results
        .absent_component_props
        .iter()
        .filter_map(|finding| {
            let prop = &finding.prop;
            let line = prop.line.saturating_sub(1);
            if prop.path != file_path || line < cursor.start.line || line > cursor.end.line {
                return None;
            }
            let content = file_lines.get(line as usize)?;
            let start = usize::try_from(prop.col).ok()?;
            let end = start.checked_add(prop.prop_name.len())?;
            if content.get(start..end)? != prop.prop_name
                || !code_region(file_path, &file_lines, line as usize)
            {
                return None;
            }
            let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
            if content
                .get(..start)?
                .chars()
                .next_back()
                .is_some_and(is_ident)
                || content.get(end..)?.chars().next().is_some_and(is_ident)
            {
                return None;
            }
            let diagnostic = absent_prop_diagnostic(finding, &mut mapper)?;
            let indent: String = content.chars().take_while(|c| c.is_whitespace()).collect();
            let position = Position { line, character: 0 };
            let edit = TextEdit {
                range: Range {
                    start: position,
                    end: position,
                },
                new_text: format!("{indent}// fallow-ignore-next-line absent-component-prop\n"),
            };
            let changes = std::collections::HashMap::from([(uri.clone(), vec![edit])]);
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Dismiss optional prop review on this line ({}.{})",
                    prop.component_name, prop.prop_name
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

fn code_region(path: &Path, lines: &[&str], anchor: usize) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("vue" | "svelte") => {
            let mut in_script = false;
            for (index, line) in lines.iter().enumerate().take(anchor + 1) {
                if line.contains("<script") {
                    if index == anchor {
                        return false;
                    }
                    in_script = line.contains('>');
                }
                if line.contains("</script") {
                    in_script = false;
                }
            }
            in_script
        }
        Some("astro") => {
            lines.first().is_some_and(|line| line.trim() == "---")
                && anchor > 0
                && !lines
                    .iter()
                    .take(anchor + 1)
                    .skip(1)
                    .any(|line| line.trim() == "---")
        }
        Some("js" | "jsx" | "ts" | "tsx" | "mts" | "cts" | "mjs" | "cjs" | "gts" | "gjs") => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::{
        output_dead_code::AbsentComponentPropFinding, results::AbsentComponentProp,
    };

    fn results(path: &Path, line: u32, col: u32) -> AnalysisResults {
        let mut results = AnalysisResults::default();
        results
            .absent_component_props
            .push(AbsentComponentPropFinding::with_actions(
                AbsentComponentProp {
                    path: path.to_path_buf(),
                    component_name: "Card".to_string(),
                    prop_name: "highlight".to_string(),
                    framework: "react".to_string(),
                    line,
                    col,
                    has_default: true,
                    inspected_call_sites: vec![],
                    explanation: "Review callers and defaults.".to_string(),
                },
            ));
        results
    }

    #[test]
    fn candidate_offers_only_additive_line_suppression_in_supported_code_regions() {
        for (extension, source, line) in [
            ("tsx", "type Props = {\n  highlight?: boolean\n};\n", 2),
            (
                "vue",
                "<script setup lang=\"ts\">\ntype Props = {\n  highlight?: boolean\n};\n</script>\n",
                3,
            ),
            (
                "svelte",
                "<script lang=\"ts\">\ntype Props = {\n  highlight?: boolean\n};\n</script>\n",
                3,
            ),
            (
                "astro",
                "---\ninterface Props {\n  highlight?: boolean\n}\n---\n",
                3,
            ),
            ("gts", "interface Args {\n  highlight?: boolean\n}\n", 2),
            ("ts", "class Card {\n  highlight?: boolean\n}\n", 2),
        ] {
            let project = tempfile::tempdir().expect("project");
            let path = project.path().join(format!("Card.{extension}"));
            std::fs::write(&path, source).expect("saved declaration");
            let uri = Uri::from_file_path(&path).expect("uri");
            let results = results(&path, line, 2);
            let range = Range {
                start: Position {
                    line: line - 1,
                    character: 2,
                },
                end: Position {
                    line: line - 1,
                    character: 11,
                },
            };
            let actions = build_suppress_absent_prop_actions(&results, &path, &uri, &range, source);
            assert_eq!(actions.len(), 1, "{extension}");
            let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
                panic!("action")
            };
            let edits = &action
                .edit
                .as_ref()
                .expect("edit")
                .changes
                .as_ref()
                .expect("changes")[&uri];
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].range.start, edits[0].range.end);
            assert_eq!(
                edits[0].new_text,
                "  // fallow-ignore-next-line absent-component-prop\n"
            );
            assert!(!action.title.contains("Remove"));
        }
    }

    #[test]
    fn suppression_links_utf16_range_from_saved_valid_source() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("Card.tsx");
        let source = "type Props = { /*😀*/ highlight?: boolean };\n";
        std::fs::write(&path, source).expect("saved valid declaration");
        let uri = Uri::from_file_path(&path).expect("uri");
        let col = u32::try_from(source.find("highlight").expect("property")).expect("col");
        let results = results(&path, 1, col);
        let start = u32::try_from(source[..col as usize].encode_utf16().count()).expect("utf16");
        let range = Range {
            start: Position {
                line: 0,
                character: start,
            },
            end: Position {
                line: 0,
                character: start + 9,
            },
        };
        let actions = build_suppress_absent_prop_actions(&results, &path, &uri, &range, source);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("action")
        };
        assert_eq!(
            action.diagnostics.as_ref().expect("linked diagnostic")[0].range,
            range
        );
    }

    #[test]
    fn changed_same_name_strings_comments_and_sfc_regions_do_not_offer_actions() {
        for (extension, saved, live, line, col) in [
            (
                "tsx",
                "type Props = {\n  highlight?: boolean\n};\n",
                "const description = `\n  highlight\n`;\n",
                2,
                2,
            ),
            (
                "tsx",
                "type Props = {\n  highlight?: boolean\n};\n",
                "/*\n  highlight\n*/\n",
                2,
                2,
            ),
            (
                "vue",
                "<script lang='ts'>\n  highlight?: boolean\n</script>\n",
                "<template>\n  highlight\n</template>\n",
                2,
                2,
            ),
            (
                "svelte",
                "<script lang='ts'>\n  highlight?: boolean\n</script>\n",
                "<div>\n  highlight\n</div>\n",
                2,
                2,
            ),
            (
                "astro",
                "---\n  highlight?: boolean\n---\n",
                "<div>\n  highlight\n</div>\n",
                2,
                2,
            ),
        ] {
            let project = tempfile::tempdir().expect("project");
            let path = project.path().join(format!("Card.{extension}"));
            std::fs::write(&path, saved).expect("saved declaration");
            let uri = Uri::from_file_path(&path).expect("uri");
            let results = results(&path, line, col);
            let range = Range {
                start: Position {
                    line: line - 1,
                    character: 0,
                },
                end: Position {
                    line: line - 1,
                    character: 20,
                },
            };
            assert!(
                build_suppress_absent_prop_actions(&results, &path, &uri, &range, live).is_empty(),
                "{extension}"
            );
        }
    }

    #[test]
    fn missing_saved_source_has_no_suppression_action() {
        let project = tempfile::tempdir().expect("project");
        let path = project.path().join("Card.tsx");
        let uri = Uri::from_file_path(&path).expect("uri");
        let results = results(&path, 2, 4);
        assert!(
            build_suppress_absent_prop_actions(
                &results,
                &path,
                &uri,
                &Range {
                    start: Position {
                        line: 1,
                        character: 4
                    },
                    end: Position {
                        line: 1,
                        character: 13
                    },
                },
                "type Props = {\n    highlight?:boolean\n};"
            )
            .is_empty()
        );
    }
}
