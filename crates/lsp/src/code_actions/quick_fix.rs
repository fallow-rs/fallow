#[expect(
    clippy::disallowed_types,
    reason = "serde JSON deserialization produces std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;

#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use tower_lsp::lsp_types::*;

use fallow_core::results::AnalysisResults;

use crate::diagnostics::FIRST_LINE_RANGE;

/// Build quick-fix code actions for unused exports (remove the `export` keyword).
#[expect(
    clippy::disallowed_types,
    reason = "serde JSON deserialization produces std HashMap"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "identifier/indent lengths are bounded by source size"
)]
pub fn build_remove_export_actions(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Url,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for (exports, msg_prefix) in [
        (&results.unused_exports, "Export"),
        (&results.unused_types, "Type export"),
    ] {
        for export in exports {
            if export.path != file_path {
                continue;
            }

            // export.line is a 1-based line number; convert to 0-based for LSP
            let export_line = export.line.saturating_sub(1);

            // Check if this diagnostic is in the requested range
            if export_line < cursor_range.start.line || export_line > cursor_range.end.line {
                continue;
            }

            // Determine the export prefix to remove by inspecting the line content
            let line_content = file_lines.get(export_line as usize).copied().unwrap_or("");
            let trimmed = line_content.trim_start();
            let indent_len = line_content.len() - trimmed.len();

            let prefix_to_remove = if trimmed.starts_with("export default ") {
                Some("export default ")
            } else if trimmed.starts_with("export ") {
                // Handles: export const, export function, export class, export type,
                // export interface, export enum, export abstract, export async,
                // export let, export var, etc.
                Some("export ")
            } else {
                None
            };

            let Some(prefix) = prefix_to_remove else {
                continue;
            };

            let title = format!("Remove unused export `{}`", export.export_name);
            let mut changes = HashMap::new();

            // Create a text edit that removes the export keyword prefix
            let edit = TextEdit {
                range: Range {
                    start: Position {
                        line: export_line,
                        character: indent_len as u32,
                    },
                    end: Position {
                        line: export_line,
                        character: (indent_len + prefix.len()) as u32,
                    },
                },
                new_text: String::new(),
            };

            changes.insert(uri.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                diagnostics: Some(vec![Diagnostic {
                    range: Range {
                        start: Position {
                            line: export_line,
                            character: export.col,
                        },
                        end: Position {
                            line: export_line,
                            character: export.col + export.export_name.len() as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::HINT),
                    source: Some("fallow".to_string()),
                    message: format!("{msg_prefix} '{}' is unused", export.export_name),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Default::default()
                }]),
                ..Default::default()
            }));
        }
    }

    actions
}

/// Build quick-fix code actions for unused pnpm catalog entries
/// (delete the line range from `pnpm-workspace.yaml`).
///
/// Only emits an action when the finding's `hardcoded_consumers` list is
/// empty. Workspace packages that still pin a hardcoded version would
/// break on the next `pnpm install` if the catalog entry were removed,
/// so the user must migrate consumers to the `catalog:` protocol first.
///
/// The LSP diagnostic carries only the entry's start line, not the end.
/// The deletion range is computed from the YAML source on disk by
/// scanning forward for lines whose indent is strictly greater than the
/// entry line's indent (covers object-form entries such as
/// `react:\n    specifier: ^18.2.0`).
///
/// The `root` parameter is required because `UnusedCatalogEntry.path` is
/// stored project-root-relative; `Url::from_file_path` would silently
/// reject the relative path and the action would never appear.
///
/// `file_lines` is the caller-supplied content of `pnpm-workspace.yaml`
/// (in-memory document text when available, otherwise on-disk content).
/// Passing the buffer in mirrors `build_remove_export_actions` and keeps
/// the deletion range consistent with what the user actually sees in
/// their editor, even when there are unsaved edits to the YAML file.
/// Empty `file_lines` short-circuits the function with no actions.
#[expect(
    clippy::disallowed_types,
    reason = "WorkspaceEdit.changes is typed as std::collections::HashMap by tower-lsp"
)]
pub fn build_remove_catalog_entry_actions(
    results: &AnalysisResults,
    root: &Path,
    uri: &Url,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    if file_lines.is_empty() {
        return actions;
    }

    for entry in &results.unused_catalog_entries {
        // Skip entries with hardcoded consumers: the consumer-side
        // migration must happen first.
        if !entry.hardcoded_consumers.is_empty() {
            continue;
        }

        let Ok(entry_uri) = Url::from_file_path(root.join(&entry.path)) else {
            continue;
        };
        if entry_uri != *uri {
            continue;
        }

        let entry_line = entry.line.saturating_sub(1);
        if entry_line < cursor_range.start.line || entry_line > cursor_range.end.line {
            continue;
        }

        let start_idx = entry_line as usize;
        if start_idx >= file_lines.len() {
            continue;
        }
        let end_idx = compute_catalog_deletion_end(file_lines, start_idx);
        // Sanity-check the line really matches the reported entry name
        // BEFORE producing a destructive edit. The file may have been
        // edited since `fallow check` ran, and pnpm catalogs commonly
        // contain sibling entries whose names overlap by substring
        // (`react` and `react-native`, `lodash` and `lodash-es`,
        // `core-js` and `core-js-bundle`). A loose `contains` check
        // would happily delete the wrong line in those cases. Anchor
        // the match to the start of the trimmed line in either the
        // unquoted (`react:`), double-quoted (`"@scope/foo":`), or
        // single-quoted (`'react':`) key form.
        if !line_matches_catalog_key(file_lines[start_idx], &entry.entry_name) {
            continue;
        }

        let title = if entry.catalog_name == "default" {
            format!("Remove unused catalog entry `{}`", entry.entry_name)
        } else {
            format!(
                "Remove unused catalog entry `{}` from `{}`",
                entry.entry_name, entry.catalog_name
            )
        };

        let mut changes = HashMap::new();
        // Single TextEdit removing the line range [start_idx, end_idx).
        // The replacement text is empty. Using start of start_idx to
        // start of end_idx absorbs the newline at the boundary so we
        // don't leave a blank line behind.
        let mut edits = vec![TextEdit {
            range: Range {
                start: Position {
                    line: start_idx as u32,
                    character: 0,
                },
                end: Position {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "line index is bounded by source size"
                    )]
                    line: end_idx as u32,
                    character: 0,
                },
            },
            new_text: String::new(),
        }];

        // If removing this entry empties its parent catalog
        // (`catalog:` or `catalogs.<name>:`), append a second TextEdit
        // that rewrites the parent header to `key: {}`. Bare `key:` in
        // YAML parses as null which pnpm rejects with
        // "Cannot convert undefined or null to object" at install time.
        // Verified against pnpm 10.33.4.
        if let Some(parent_edit) =
            build_parent_rewrite_edit(file_lines, start_idx, end_idx, &entry.catalog_name)
        {
            edits.push(parent_edit);
        }
        changes.insert(uri.clone(), edits);

        // Reconstruct the published diagnostic so VS Code's
        // "Fix all in file" source action can tie this edit back to the
        // existing diagnostic. The message wording matches
        // `crates/lsp/src/diagnostics/unused.rs:261-271` (default vs
        // named-catalog variants).
        let diagnostic_message = if entry.catalog_name == "default" {
            format!(
                "Unused catalog entry: '{}' is not referenced by any workspace package",
                entry.entry_name
            )
        } else {
            format!(
                "Unused catalog entry: '{}' in catalog '{}' is not referenced by any workspace package",
                entry.entry_name, entry.catalog_name
            )
        };

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            diagnostics: Some(vec![Diagnostic {
                range: Range {
                    start: Position {
                        line: entry_line,
                        character: 0,
                    },
                    end: Position {
                        line: entry_line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-catalog-entry".to_string())),
                message: diagnostic_message,
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                ..Default::default()
            }]),
            ..Default::default()
        }));
    }

    actions
}

/// Anchored match for a catalog key on its declared line. Handles
/// unquoted (`react:`), double-quoted (`"@scope/foo":`), and single-quoted
/// (`'react':`) key forms; rejects substring matches in values or in
/// sibling entries whose names share a prefix (`react` vs `react-native`).
fn line_matches_catalog_key(line: &str, entry_name: &str) -> bool {
    let trimmed = line.trim_start();
    // Unquoted: `react:` or `react: ^18.2.0` or `react :` (rare).
    if let Some(rest) = trimmed.strip_prefix(entry_name)
        && rest.trim_start().starts_with(':')
    {
        return true;
    }
    // Double-quoted: `"@scope/foo": ...`
    if let Some(rest) = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_prefix(entry_name))
        && (rest.starts_with("\":") || rest.starts_with("\" :"))
    {
        return true;
    }
    // Single-quoted: `'react': ...`
    if let Some(rest) = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_prefix(entry_name))
        && (rest.starts_with("':") || rest.starts_with("' :"))
    {
        return true;
    }
    false
}

/// Compute the end line index (exclusive) for a catalog entry whose key
/// sits on `start_idx`. Mirrors the algorithm in
/// `crates/cli/src/fix/catalog.rs::compute_deletion_range` so the LSP
/// preview matches what `fallow fix` would write.
fn compute_catalog_deletion_end(lines: &[&str], start_idx: usize) -> usize {
    let entry_indent = lines[start_idx].bytes().take_while(|&b| b == b' ').count();
    let mut end_idx = start_idx + 1;
    while end_idx < lines.len() {
        let line = lines[end_idx];
        if line.trim().is_empty() {
            break;
        }
        let indent = line.bytes().take_while(|&b| b == b' ').count();
        if indent <= entry_indent {
            break;
        }
        end_idx += 1;
    }
    end_idx
}

/// Build a TextEdit that rewrites the parent catalog header to `key: {}`
/// when removing the line range `[start_idx, end_idx)` would leave it
/// with no children. Returns `None` if siblings remain or no parent is
/// found. Mirrors the CLI fix module's `rewrite_empty_catalog_parents`.
fn build_parent_rewrite_edit(
    lines: &[&str],
    start_idx: usize,
    end_idx: usize,
    catalog_name: &str,
) -> Option<TextEdit> {
    let parent_idx = find_parent_header_idx(lines, start_idx, catalog_name)?;
    if parent_has_other_children(lines, parent_idx, start_idx, end_idx) {
        return None;
    }
    let header = lines[parent_idx];
    let trimmed_end = header.trim_end();
    let new_text = format!("{trimmed_end} {{}}");
    Some(TextEdit {
        range: Range {
            start: Position {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "line index is bounded by source size"
                )]
                line: parent_idx as u32,
                character: 0,
            },
            end: Position {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "line index is bounded by source size"
                )]
                line: parent_idx as u32,
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "header length is bounded by source size"
                )]
                character: header.len() as u32,
            },
        },
        new_text,
    })
}

/// Walk backwards from a catalog entry line to find its parent header
/// line index. For default-catalog entries the parent is the line
/// starting with `catalog:`; for named-catalog entries the parent is
/// the indented `<name>:` line under `catalogs:`.
fn find_parent_header_idx(lines: &[&str], entry_idx: usize, catalog_name: &str) -> Option<usize> {
    if entry_idx >= lines.len() {
        return None;
    }
    let entry_indent = lines[entry_idx].bytes().take_while(|&b| b == b' ').count();
    for idx in (0..entry_idx).rev() {
        let line = lines[idx];
        let stripped = line.trim_end();
        let content = stripped.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = stripped.bytes().take_while(|&b| b == b' ').count();
        if indent >= entry_indent {
            continue;
        }
        if catalog_name == "default" {
            return content.starts_with("catalog:").then_some(idx);
        }
        let key = content
            .trim_start_matches(['"', '\''])
            .split([':', '"', '\''])
            .next()
            .unwrap_or("");
        return (key == catalog_name).then_some(idx);
    }
    None
}

/// Return true if the parent at `parent_idx` has at least one child
/// line that is NOT inside the deletion range `[del_start, del_end)`.
/// Comments and blank lines are not counted as children.
fn parent_has_other_children(
    lines: &[&str],
    parent_idx: usize,
    del_start: usize,
    del_end: usize,
) -> bool {
    let parent_indent = lines[parent_idx].bytes().take_while(|&b| b == b' ').count();
    for (idx, line) in lines.iter().enumerate().skip(parent_idx + 1) {
        let stripped = line.trim_end();
        let content = stripped.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let indent = stripped.bytes().take_while(|&b| b == b' ').count();
        if indent <= parent_indent {
            return false;
        }
        // This is a child of the parent. Is it inside the deletion range?
        if idx < del_start || idx >= del_end {
            return true;
        }
    }
    false
}

/// Build quick-fix code actions for unused files (delete the file).
pub fn build_delete_file_actions(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Url,
    cursor_range: &Range,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for file in &results.unused_files {
        if file.path != file_path {
            continue;
        }

        // The diagnostic is at line 0, col 0 — check if the request range overlaps
        if cursor_range.start.line > 0 {
            continue;
        }

        let title = "Delete this unused file".to_string();

        let delete_file_op = DocumentChangeOperation::Op(ResourceOp::Delete(DeleteFile {
            uri: uri.clone(),
            options: Some(DeleteFileOptions {
                recursive: Some(false),
                ignore_if_not_exists: Some(true),
                annotation_id: None,
            }),
        }));

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Operations(vec![delete_file_op])),
                ..Default::default()
            }),
            diagnostics: Some(vec![Diagnostic {
                range: FIRST_LINE_RANGE,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-file".to_string())),
                message: "File is not reachable from any entry point".to_string(),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                ..Default::default()
            }]),
            ..Default::default()
        }));
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_core::results::{UnusedExport, UnusedFile};

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn make_range(start_line: u32, end_line: u32) -> Range {
        Range {
            start: Position {
                line: start_line,
                character: 0,
            },
            end: Position {
                line: end_line,
                character: 0,
            },
        }
    }

    fn make_unused_export(path: &Path, name: &str, line: u32, col: u32) -> UnusedExport {
        UnusedExport {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col,
            span_start: 0,
            is_re_export: false,
        }
    }

    fn unwrap_code_action(action: &CodeActionOrCommand) -> &CodeAction {
        match action {
            CodeActionOrCommand::CodeAction(ca) => ca,
            CodeActionOrCommand::Command(_) => panic!("expected CodeAction, got Command"),
        }
    }

    // -----------------------------------------------------------------------
    // build_remove_export_actions
    // -----------------------------------------------------------------------

    #[test]
    fn no_export_actions_when_results_empty() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Url::from_file_path(&file).unwrap();
        let results = AnalysisResults::default();
        let lines = vec!["export const foo = 1;"];

        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_export_actions_for_different_file() {
        let root = test_root();
        let file_a = root.join("a.ts");
        let file_b = root.join("b.ts");
        let uri_b = Url::from_file_path(&file_b).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file_a, "foo", 1, 7));

        let lines = vec!["export const foo = 1;"];
        let actions =
            build_remove_export_actions(&results, &file_b, &uri_b, &make_range(0, 10), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_export_actions_when_cursor_outside_export_line() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // Export on 1-based line 5 => 0-based line 4
        results
            .unused_exports
            .push(make_unused_export(&file, "bar", 5, 7));

        let lines = vec!["line0", "line1", "line2", "line3", "export const bar = 2;"];
        // Cursor on lines 0-2, export is on line 4
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 2), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn generates_action_for_unused_export_const() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 13));

        let lines = vec!["export const foo = 42;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        assert_eq!(ca.title, "Remove unused export `foo`");
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));

        // The edit should remove "export " (7 chars starting at column 0)
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 7); // "export " = 7 chars
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn generates_action_for_export_default() {
        let root = test_root();
        let file = root.join("component.tsx");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "default", 1, 0));

        let lines = vec!["export default function App() {}"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        // "export default " = 15 chars
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 15);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn preserves_indentation_in_edit_range() {
        let root = test_root();
        let file = root.join("nested.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // Export on 1-based line 2 => 0-based line 1
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 2, 11));

        let lines = vec![
            "namespace Ns {",
            "    export function helper() {}", // 4 spaces indent
            "}",
        ];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(1, 1), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        // Edit should start at column 4 (after indent) and remove "export " (7 chars)
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.start.character, 4);
        assert_eq!(edits[0].range.end.character, 11); // 4 + 7
    }

    #[test]
    fn handles_type_exports() {
        let root = test_root();
        let file = root.join("types.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_types.push(UnusedExport {
            path: file.clone(),
            export_name: "MyType".to_string(),
            is_type_only: true,
            line: 1,
            col: 12,
            span_start: 0,
            is_re_export: false,
        });

        let lines = vec!["export type MyType = string;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        // Check the diagnostic message uses "Type export" prefix
        let diags = ca.diagnostics.as_ref().unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "Type export 'MyType' is unused");
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(diags[0].source, Some("fallow".to_string()));
        assert_eq!(diags[0].tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn combines_unused_exports_and_unused_types() {
        let root = test_root();
        let file = root.join("mixed.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 13));
        results.unused_types.push(UnusedExport {
            path: file.clone(),
            export_name: "Bar".to_string(),
            is_type_only: true,
            line: 2,
            col: 12,
            span_start: 0,
            is_re_export: false,
        });

        let lines = vec!["export const foo = 1;", "export type Bar = string;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 1), &lines);

        assert_eq!(actions.len(), 2);

        let ca0 = unwrap_code_action(&actions[0]);
        let ca1 = unwrap_code_action(&actions[1]);

        assert_eq!(ca0.title, "Remove unused export `foo`");
        assert_eq!(ca1.title, "Remove unused export `Bar`");

        // Verify message prefixes differ
        let diag0 = &ca0.diagnostics.as_ref().unwrap()[0];
        let diag1 = &ca1.diagnostics.as_ref().unwrap()[0];
        assert!(diag0.message.starts_with("Export "));
        assert!(diag1.message.starts_with("Type export "));
    }

    #[test]
    fn skips_line_without_export_prefix() {
        let root = test_root();
        let file = root.join("odd.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // The result says line 1 has an unused export, but the actual line content
        // doesn't start with "export" (e.g., re-export or corrupted data)
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 0));

        let lines = vec!["const foo = 1;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "Should skip exports where line doesn't start with 'export'"
        );
    }

    #[test]
    fn handles_export_on_line_0_saturating_sub() {
        let root = test_root();
        let file = root.join("edge.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // line=0 is unusual (lines are 1-based), but saturating_sub(1) handles it
        // gracefully by producing 0-based line 0 (same as line=1 would)
        results
            .unused_exports
            .push(make_unused_export(&file, "x", 0, 7));

        let lines = vec!["export const x = 1;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        // saturating_sub(0, 1) = 0, so it maps to line 0 which is in range
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn multiple_exports_same_file_all_in_range() {
        let root = test_root();
        let file = root.join("multi.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "a", 1, 7));
        results
            .unused_exports
            .push(make_unused_export(&file, "b", 2, 7));
        results
            .unused_exports
            .push(make_unused_export(&file, "c", 3, 7));

        let lines = vec![
            "export function a() {}",
            "export function b() {}",
            "export function c() {}",
        ];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 2), &lines);

        assert_eq!(actions.len(), 3);
        for action in &actions {
            let ca = unwrap_code_action(action);
            assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
        }
    }

    #[test]
    fn cursor_range_filters_subset_of_exports() {
        let root = test_root();
        let file = root.join("filter.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "a", 1, 7));
        results
            .unused_exports
            .push(make_unused_export(&file, "b", 3, 7));
        results
            .unused_exports
            .push(make_unused_export(&file, "c", 5, 7));

        let lines = vec![
            "export const a = 1;",
            "const used = true;",
            "export const b = 2;",
            "const also_used = false;",
            "export const c = 3;",
        ];
        // Cursor covers only line 2 (0-based), which is 1-based line 3 => export "b"
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(2, 2), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        assert_eq!(ca.title, "Remove unused export `b`");
    }

    #[test]
    fn diagnostic_range_matches_export_name_span() {
        let root = test_root();
        let file = root.join("span.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // col=13 means "export const " (13 chars), name "myLongExport" is 12 chars
        results
            .unused_exports
            .push(make_unused_export(&file, "myLongExport", 1, 13));

        let lines = vec!["export const myLongExport = 42;"];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let diag = &ca.diagnostics.as_ref().unwrap()[0];

        assert_eq!(diag.range.start.line, 0);
        assert_eq!(diag.range.start.character, 13);
        assert_eq!(diag.range.end.line, 0);
        assert_eq!(diag.range.end.character, 25); // 13 + 12
    }

    #[test]
    fn handles_empty_file_lines() {
        let root = test_root();
        let file = root.join("empty.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "x", 1, 0));

        // No lines at all — the get() call returns None, unwrap_or("")
        let lines: Vec<&str> = vec![];
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);
        // Empty string doesn't start with "export", so no action
        assert!(actions.is_empty());
    }

    #[test]
    fn handles_tab_indentation() {
        let root = test_root();
        let file = root.join("tabs.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "val", 1, 0));

        let lines = vec!["\t\texport const val = 1;"]; // 2 tabs of indent
        let actions = build_remove_export_actions(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        // 2 bytes of tab indent + "export " (7 chars) = columns 2..9
        assert_eq!(edits[0].range.start.character, 2);
        assert_eq!(edits[0].range.end.character, 9);
    }

    // -----------------------------------------------------------------------
    // build_delete_file_actions
    // -----------------------------------------------------------------------

    #[test]
    fn no_delete_actions_when_no_unused_files() {
        let root = test_root();
        let file = root.join("used.ts");
        let uri = Url::from_file_path(&file).unwrap();
        let results = AnalysisResults::default();

        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 10));
        assert!(actions.is_empty());
    }

    #[test]
    fn no_delete_action_for_different_file() {
        let root = test_root();
        let file_a = root.join("a.ts");
        let file_b = root.join("b.ts");
        let uri_b = Url::from_file_path(&file_b).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file_a });

        let actions = build_delete_file_actions(&results, &file_b, &uri_b, &make_range(0, 10));
        assert!(actions.is_empty());
    }

    #[test]
    fn no_delete_action_when_cursor_not_at_line_0() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });

        // Cursor starts at line 1, but diagnostic is at line 0
        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(1, 5));
        assert!(actions.is_empty());
    }

    #[test]
    fn generates_delete_action_when_cursor_at_line_0() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });

        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 0));

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        assert_eq!(ca.title, "Delete this unused file");
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    }

    #[test]
    fn delete_action_uses_document_changes_with_delete_op() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });

        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 0));
        let ca = unwrap_code_action(&actions[0]);

        let doc_changes = ca.edit.as_ref().unwrap().document_changes.as_ref().unwrap();

        match doc_changes {
            DocumentChanges::Operations(ops) => {
                assert_eq!(ops.len(), 1);
                match &ops[0] {
                    DocumentChangeOperation::Op(ResourceOp::Delete(del)) => {
                        assert_eq!(del.uri, uri);
                        let opts = del.options.as_ref().unwrap();
                        assert_eq!(opts.recursive, Some(false));
                        assert_eq!(opts.ignore_if_not_exists, Some(true));
                    }
                    other => panic!("expected Delete op, got: {other:?}"),
                }
            }
            other @ DocumentChanges::Edits(_) => panic!("expected Operations, got: {other:?}"),
        }
    }

    #[test]
    fn delete_action_diagnostic_has_correct_properties() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });

        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 0));
        let ca = unwrap_code_action(&actions[0]);

        let diags = ca.diagnostics.as_ref().unwrap();
        assert_eq!(diags.len(), 1);
        let diag = &diags[0];

        assert_eq!(diag.range, FIRST_LINE_RANGE);
        assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diag.source, Some("fallow".to_string()));
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("unused-file".to_string()))
        );
        assert_eq!(diag.message, "File is not reachable from any entry point");
        assert_eq!(diag.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn delete_action_with_cursor_spanning_line_0() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });

        // Cursor from line 0 to line 50 — should still trigger because start.line == 0
        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 50));
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn multiple_unused_files_same_path_produces_multiple_actions() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        // Unlikely in practice, but tests that the loop iterates all entries
        results.unused_files.push(UnusedFile { path: file.clone() });
        results.unused_files.push(UnusedFile { path: file.clone() });

        let actions = build_delete_file_actions(&results, &file, &uri, &make_range(0, 0));
        assert_eq!(actions.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Integration: both functions together on same file
    // -----------------------------------------------------------------------

    #[test]
    fn unused_file_and_unused_export_in_same_file() {
        let root = test_root();
        let file = root.join("orphan.ts");
        let uri = Url::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile { path: file.clone() });
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 1, 16));

        let lines = vec!["export function helper() {}"];
        let cursor = make_range(0, 0);

        let export_actions = build_remove_export_actions(&results, &file, &uri, &cursor, &lines);
        let delete_actions = build_delete_file_actions(&results, &file, &uri, &cursor);

        // Both produce independent actions
        assert_eq!(export_actions.len(), 1);
        assert_eq!(delete_actions.len(), 1);

        // They are different action types
        let export_ca = unwrap_code_action(&export_actions[0]);
        let delete_ca = unwrap_code_action(&delete_actions[0]);
        assert!(export_ca.title.contains("Remove unused export"));
        assert!(delete_ca.title.contains("Delete"));
    }

    // -----------------------------------------------------------------------
    // build_remove_catalog_entry_actions
    // -----------------------------------------------------------------------

    use fallow_core::results::UnusedCatalogEntry;

    fn make_catalog_entry(
        name: &str,
        catalog: &str,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntry {
        UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: consumers,
        }
    }

    fn workspace_yaml_uri(dir: &tempfile::TempDir) -> Url {
        // We need a URI that matches `Url::from_file_path(root.join("pnpm-workspace.yaml"))`
        // exactly. The file does NOT need to exist on disk because the LSP
        // handler now reads from `file_lines` passed in by the caller.
        Url::from_file_path(dir.path().join("pnpm-workspace.yaml")).unwrap()
    }

    #[test]
    fn no_catalog_action_when_no_findings() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);
        let results = AnalysisResults::default();
        let lines = vec!["catalog:", "  is-even: ^1.0.0"];

        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(0, 100),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_uses_root_join_for_relative_path() {
        // Regression test for issue #329: when the LSP emitter forgets
        // to `root.join(&entry.path)`, `Url::from_file_path` rejects the
        // relative path and the action silently never appears.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 2, vec![]));

        let lines = vec!["catalog:", "  is-even: ^1.0.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        assert!(ca.title.contains("is-even"));
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    }

    #[test]
    fn catalog_action_skipped_when_hardcoded_consumers_present() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results.unused_catalog_entries.push(make_catalog_entry(
            "react",
            "default",
            2,
            vec![PathBuf::from("apps/web/package.json")],
        ));

        let lines = vec!["catalog:", "  react: ^18.2.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        // The action must NOT appear: removing the catalog entry while a
        // consumer still pins a hardcoded version would break install.
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_deletes_object_form_range() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "default", 2, vec![]));

        let lines = vec![
            "catalog:",
            "  react:",
            "    specifier: ^18.2.0",
            "    publishConfig: {}",
            "  is-even: ^1.0.0",
        ];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        // start line 1 (`react:`), end line 4 (`is-even:` line). Range
        // covers the 3-line object block: react: + 2 nested.
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.end.line, 4);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn catalog_action_bails_when_line_does_not_match_entry_name() {
        // File has been edited since `fallow check` ran; the reported
        // line no longer holds the catalog entry. Don't blindly delete
        // unrelated content.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 2, vec![]));

        let lines = vec!["catalog:", "  different-entry: ^2.0.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_bails_when_entry_name_is_substring_of_sibling() {
        // Regression test for the rust-reviewer's BLOCK on the initial
        // implementation: pnpm catalogs commonly hold sibling entries
        // whose names share a prefix (`react` and `react-native`,
        // `lodash` and `lodash-es`, `core-js` and `core-js-bundle`). If
        // the file has been edited since `fallow check` ran and the
        // reported line now holds a DIFFERENT entry whose name starts
        // with the same prefix, the action must NOT fire.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        // Reported finding: `react` on line 2.
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "default", 2, vec![]));

        // But the actual file now has `react-native` on line 2.
        let lines = vec!["catalog:", "  react-native: ^0.73.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert!(
            actions.is_empty(),
            "must NOT delete `react-native` when the finding reports `react`"
        );
    }

    #[test]
    fn catalog_action_bails_when_entry_name_appears_only_in_value() {
        // Defensive: an entry whose value happens to contain the entry
        // name as a substring shouldn't accidentally match. Vanishingly
        // rare in real YAML but cheap to defend against.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "default", 2, vec![]));

        // The actual line is a different key whose VALUE mentions react.
        let lines = vec!["catalog:", "  description: react fork"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_handles_quoted_key_forms() {
        // Scoped packages and other quoted keys are valid YAML. The
        // sanity check accepts both quote styles.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("@scope/foo", "default", 2, vec![]));

        let lines = vec!["catalog:", "  \"@scope/foo\": ^1.0.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn catalog_action_outside_cursor_range_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        // is-even sits on 1-based line 3 (0-based line 2)
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 3, vec![]));

        let lines = vec!["catalog:", "  is-odd: ^1.0.0", "  is-even: ^1.0.0"];
        // Cursor on lines 0-1; action's diagnostic is on line 2.
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(0, 1),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_named_catalog_uses_matching_diagnostic_message() {
        // Regression test for the LSP-reviewer C3 concern: the action's
        // reconstructed diagnostic message must mirror the published
        // diagnostic's named-catalog phrasing so "Fix all in file"
        // grouping does not get confused.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "react17", 3, vec![]));

        let lines = vec!["catalogs:", "  react17:", "    react: ^17.0.2"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(2, 2),
            &lines,
        );
        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let diag_msg = &ca.diagnostics.as_ref().unwrap()[0].message;
        assert!(
            diag_msg.contains("in catalog 'react17'"),
            "named-catalog diagnostic must say `in catalog 'react17'`, got: {diag_msg}"
        );
    }

    #[test]
    fn catalog_action_emits_parent_rewrite_when_emptying_named_catalog() {
        // Regression: pnpm rejects bare `react17:` (null value). When the
        // last entry of a named catalog is removed, the action must emit
        // a second TextEdit rewriting the parent to `react17: {}`.
        // Reproduces the issue-329 fixture scenario Codex caught.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "react17", 3, vec![]));

        let lines = vec!["catalogs:", "  react17:", "    react: ^17.0.2"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(2, 2),
            &lines,
        );

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(
            edits.len(),
            2,
            "must emit deletion + parent rewrite, got {} edits",
            edits.len()
        );
        // First edit removes the entry line.
        assert_eq!(edits[0].range.start.line, 2);
        assert_eq!(edits[0].new_text, "");
        // Second edit rewrites the parent header.
        assert_eq!(edits[1].range.start.line, 1);
        assert_eq!(edits[1].new_text, "  react17: {}");
    }

    #[test]
    fn catalog_action_no_parent_rewrite_when_siblings_remain() {
        // When other entries stay in the catalog, the parent rewrite
        // must NOT fire.
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "react17", 3, vec![]));

        let lines = vec![
            "catalogs:",
            "  react17:",
            "    react: ^17.0.2",
            "    react-dom: ^17.0.2",
        ];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(2, 2),
            &lines,
        );

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(
            edits.len(),
            1,
            "react-dom remains so parent stays populated"
        );
    }

    #[test]
    fn catalog_action_emits_parent_rewrite_for_default_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 2, vec![]));

        let lines = vec!["catalog:", "  is-even: ^1.0.0"];
        let actions = build_remove_catalog_entry_actions(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let edits = ca
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .get(&uri)
            .unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].new_text, "catalog: {}");
    }

    #[test]
    fn compute_catalog_deletion_end_scalar() {
        let lines: Vec<&str> = "catalog:\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n"
            .split('\n')
            .collect();
        // start at index 1 (`  is-even: ^1.0.0`)
        assert_eq!(compute_catalog_deletion_end(&lines, 1), 2);
    }

    #[test]
    fn compute_catalog_deletion_end_object_form() {
        let content = "catalog:\n  react:\n    specifier: ^18.2.0\n    publishConfig: {}\n  is-even: ^1.0.0\n";
        let lines: Vec<&str> = content.split('\n').collect();
        // start at index 1 (`  react:`); should consume 3 more lines.
        assert_eq!(compute_catalog_deletion_end(&lines, 1), 4);
    }
}
