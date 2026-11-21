#[expect(
    clippy::disallowed_types,
    reason = "serde JSON deserialization produces std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;

use ls_types::*;

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_config::CatalogPrecedingCommentPolicy;
use fallow_config::pnpm_catalog_edit::{
    CatalogEditPlan, CatalogRemoval, CatalogRemovalOutcome, plan_catalog_edits,
};
use fallow_types::catalog_source::is_pnpm_catalog_source;
use fallow_types::output_dead_code::MutationEvidence;

use crate::diagnostics::FIRST_LINE_RANGE;
use crate::position::PositionMapper;
use fallow_api::source_edits::SourceEditIndex;

/// Typed input for unused export quick-fix code actions.
#[derive(Clone, Copy)]
pub struct RemoveExportActionInput<'a> {
    pub results: &'a AnalysisResults,
    pub file_path: &'a Path,
    pub uri: &'a Uri,
    pub cursor_range: &'a Range,
    /// The live text of the document.
    pub source: &'a str,
}

impl<'a> RemoveExportActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        file_path: &'a Path,
        uri: &'a Uri,
        cursor_range: &'a Range,
        source: &'a str,
    ) -> Self {
        Self {
            results,
            file_path,
            uri,
            cursor_range,
            source,
        }
    }
}

/// Build quick-fix code actions for unused exports (remove the `export` keyword).
///
/// A finding the run lacks the evidence for offers NO action here. The quick
/// fix builds its own `WorkspaceEdit` in process rather than shelling out to
/// `fallow fix`, so the CLI-side withholding protects nothing in the editor,
/// and the editor is where a single keystroke applies the edit. Withholding is
/// the consistent choice: `fallow fix` declines the same removal for the same
/// reason. The finding is still reported, and its diagnostic names the caveat
/// so the absent quick fix reads as a stated limit rather than a missing
/// feature.
///
/// The edit comes from the syntax spans of the live document text. The
/// action is offered only for a statement that the parser finds, and only
/// when each name that its `export` keyword exports is an unused finding. A
/// document that does not parse gets no action.
pub fn build_remove_export_actions(input: RemoveExportActionInput<'_>) -> Vec<CodeActionOrCommand> {
    let RemoveExportActionInput {
        results,
        file_path,
        uri,
        cursor_range,
        source,
    } = input;
    let in_file = |export: &&fallow_api::editor_results::UnusedExport| export.path == file_path;
    let unused: Vec<(&fallow_api::editor_results::UnusedExport, &str)> = results
        .unused_exports
        .iter()
        .filter(|f| f.may_auto_apply_mutation())
        .map(|f| (&f.export, "Export"))
        .chain(
            results
                .unused_types
                .iter()
                .filter(|f| f.may_auto_apply_mutation())
                .map(|f| (&f.export, "Type export")),
        )
        .filter(|(export, _)| in_file(export))
        .collect();
    let candidates: Vec<&(&fallow_api::editor_results::UnusedExport, &str)> = unused
        .iter()
        .filter(|(export, _)| {
            let line = export.line.saturating_sub(1);
            cursor_range.start.line <= line && line <= cursor_range.end.line
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let Some(index) = SourceEditIndex::parse(file_path, source) else {
        return Vec::new();
    };
    candidates
        .into_iter()
        .filter_map(|(export, msg_prefix)| {
            remove_export_action(&index, &unused, export, msg_prefix, uri, source)
        })
        .collect()
}

#[cfg(test)]
fn build_remove_export_actions_for_test(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    let source = file_lines.join("\n");
    build_remove_export_actions(RemoveExportActionInput::new(
        results,
        file_path,
        uri,
        cursor_range,
        &source,
    ))
}

#[expect(
    clippy::disallowed_types,
    reason = "serde JSON deserialization produces std HashMap"
)]
fn remove_export_action(
    index: &SourceEditIndex,
    unused: &[(&fallow_api::editor_results::UnusedExport, &str)],
    export: &fallow_api::editor_results::UnusedExport,
    msg_prefix: &str,
    uri: &Uri,
    source: &str,
) -> Option<CodeActionOrCommand> {
    let statement = index
        .export_statement(&export.export_name, export.line, export.col)
        .ok()?;
    let (first, last) = index.line_range(statement.start, statement.end);
    let every_name_unused = statement.names.iter().all(|name| {
        unused.iter().any(|(finding, _)| {
            finding.export_name == *name
                && (first..=last).contains(&(finding.line.saturating_sub(1) as usize))
        })
    });
    if !every_name_unused {
        return None;
    }
    let edit = statement.keyword_edit()?;
    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: lsp_position(source, edit.start)?,
                end: lsp_position(source, edit.end)?,
            },
            new_text: String::new(),
        }],
    );

    let export_line = export.line.saturating_sub(1);
    let mut position_mapper = PositionMapper::default();
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Remove unused export `{}`", export.export_name),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![remove_export_diagnostic(
            export,
            msg_prefix,
            export_line,
            &mut position_mapper,
        )]),
        ..Default::default()
    }))
}

/// The LSP position of byte `offset` of `source`: the line counts `\n`, and
/// the character counts UTF-16 units from the start of the line.
fn lsp_position(source: &str, offset: u32) -> Option<Position> {
    let before = source.get(..offset as usize)?;
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    Some(Position {
        line: u32::try_from(before.matches('\n').count()).ok()?,
        character: u32::try_from(before[line_start..].encode_utf16().count()).ok()?,
    })
}

fn remove_export_diagnostic(
    export: &fallow_api::editor_results::UnusedExport,
    msg_prefix: &str,
    export_line: u32,
    mapper: &mut PositionMapper,
) -> Diagnostic {
    let (start, end) =
        mapper.utf16_col_span(&export.path, export_line, export.col, &export.export_name);
    Diagnostic {
        range: Range {
            start: Position {
                line: export_line,
                character: start,
            },
            end: Position {
                line: export_line,
                character: end,
            },
        },
        severity: Some(DiagnosticSeverity::HINT),
        source: Some("fallow".to_string()),
        message: format!("{msg_prefix} '{}' is unused", export.export_name),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        ..Default::default()
    }
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
/// The `root` input field is required because `UnusedCatalogEntry.path` is
/// stored project-root-relative; `Uri::from_file_path` would silently
/// reject the relative path and the action would never appear.
///
/// `file_lines` is the caller-supplied content of `pnpm-workspace.yaml`
/// (in-memory document text when available, otherwise on-disk content).
/// Passing the buffer in mirrors `build_remove_export_actions` and keeps
/// the deletion range consistent with what the user actually sees in
/// their editor, even when there are unsaved edits to the YAML file.
/// Empty `file_lines` short-circuits the function with no actions.
#[derive(Clone, Copy)]
pub struct CatalogEntryActionInput<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub uri: &'a Uri,
    pub cursor_range: &'a Range,
    pub file_lines: &'a [&'a str],
}

impl<'a> CatalogEntryActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        root: &'a Path,
        uri: &'a Uri,
        cursor_range: &'a Range,
        file_lines: &'a [&'a str],
    ) -> Self {
        Self {
            results,
            root,
            uri,
            cursor_range,
            file_lines,
        }
    }
}

pub fn build_remove_catalog_entry_actions(
    input: CatalogEntryActionInput<'_>,
) -> Vec<CodeActionOrCommand> {
    let CatalogEntryActionInput {
        results,
        root,
        uri,
        cursor_range,
        file_lines,
    } = input;
    let mut actions = Vec::new();

    if file_lines.is_empty() {
        return actions;
    }

    for entry in &results.unused_catalog_entries {
        let entry = &entry.entry;
        let Some((entry_line, plan)) =
            catalog_entry_delete_span(entry, root, uri, cursor_range, file_lines)
        else {
            continue;
        };
        actions.push(remove_catalog_entry_action(
            entry, uri, file_lines, entry_line, &plan,
        ));
    }

    actions
}

#[cfg(test)]
fn build_remove_catalog_entry_actions_for_test(
    results: &AnalysisResults,
    root: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    build_remove_catalog_entry_actions(CatalogEntryActionInput::new(
        results,
        root,
        uri,
        cursor_range,
        file_lines,
    ))
}

fn catalog_entry_delete_span(
    entry: &fallow_api::editor_results::UnusedCatalogEntry,
    root: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Option<(u32, CatalogEditPlan)> {
    if !entry.hardcoded_consumers.is_empty() {
        return None;
    }
    if !is_pnpm_catalog_source(&entry.path) {
        return None;
    }

    let entry_uri = Uri::from_file_path(root.join(&entry.path))?;
    if entry_uri != *uri {
        return None;
    }

    let entry_line = entry.line.saturating_sub(1);
    if entry_line < cursor_range.start.line || entry_line > cursor_range.end.line {
        return None;
    }

    if entry_line as usize >= file_lines.len() {
        return None;
    }
    let removal = CatalogRemoval::Entry {
        catalog: &entry.catalog_name,
        entry: &entry.entry_name,
    };
    plan_catalog_removal_at(file_lines, removal, entry_line).map(|plan| (entry_line, plan))
}

#[expect(
    clippy::disallowed_types,
    reason = "WorkspaceEdit.changes is typed as std::collections::HashMap by ls-types"
)]
fn remove_catalog_entry_action(
    entry: &fallow_api::editor_results::UnusedCatalogEntry,
    uri: &Uri,
    file_lines: &[&str],
    entry_line: u32,
    plan: &CatalogEditPlan,
) -> CodeActionOrCommand {
    let title = catalog_entry_action_title(entry);
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), catalog_text_edits(file_lines, plan));

    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![catalog_entry_diagnostic(entry, entry_line)]),
        ..Default::default()
    })
}

/// The `TextEdit`s of `plan`: deletions first, then header rewrites.
fn catalog_text_edits(file_lines: &[&str], plan: &CatalogEditPlan) -> Vec<TextEdit> {
    let deletions = plan.edits.iter().filter(|edit| edit.replacement.is_none());
    let rewrites = plan.edits.iter().filter(|edit| edit.replacement.is_some());
    deletions
        .chain(rewrites)
        .map(|edit| {
            let (end_line, end_character, new_text) = match &edit.replacement {
                None => (edit.lines.end, 0, String::new()),
                Some(text) => (
                    edit.lines.start,
                    file_lines[edit.lines.start].encode_utf16().count(),
                    text.clone(),
                ),
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "line index is bounded by source size"
            )]
            let range = Range {
                start: Position {
                    line: edit.lines.start as u32,
                    character: 0,
                },
                end: Position {
                    line: end_line as u32,
                    character: end_character as u32,
                },
            };
            TextEdit { range, new_text }
        })
        .collect()
}

/// Build the `unused-catalog-entry` diagnostic linked to the removal action.
fn catalog_entry_diagnostic(
    entry: &fallow_api::editor_results::UnusedCatalogEntry,
    entry_line: u32,
) -> Diagnostic {
    Diagnostic {
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
        message: catalog_entry_diagnostic_message(entry),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        ..Default::default()
    }
}

fn catalog_entry_action_title(entry: &fallow_api::editor_results::UnusedCatalogEntry) -> String {
    if entry.catalog_name == "default" {
        format!("Remove unused catalog entry `{}`", entry.entry_name)
    } else {
        format!(
            "Remove unused catalog entry `{}` from `{}`",
            entry.entry_name, entry.catalog_name
        )
    }
}

fn catalog_entry_diagnostic_message(
    entry: &fallow_api::editor_results::UnusedCatalogEntry,
) -> String {
    if entry.catalog_name == "default" {
        format!(
            "Unused catalog entry: '{}' is not referenced by any workspace package",
            entry.entry_name
        )
    } else {
        format!(
            "Unused catalog entry: '{}' in catalog '{}' is not referenced by any workspace package",
            entry.entry_name, entry.catalog_name
        )
    }
}

/// Build quick-fix code actions for empty pnpm catalog groups (delete the
/// named `catalogs.<name>:` header line).
///
/// The action plans its edit with `plan_catalog_edits` on the open buffer.
/// It offers no action when the planner refuses the removal, for example
/// when the group has entries in the buffer or the buffer is not valid
/// YAML. The deletion covers the lines of the group key and its value; no
/// parent rewrite is needed (the parent `catalogs:` map keeps its other
/// named-catalog siblings, or, if this was the only one, the user can
/// remove the remaining `catalogs:` header by hand).
///
/// The default catalog (top-level `catalog:`) is intentionally never
/// flagged by the detector, so this function never offers to delete it.
///
/// `file_lines` is the caller-supplied content of `pnpm-workspace.yaml`;
/// passing the buffer in mirrors the sibling so the deletion range
/// matches what the user sees in their editor when there are unsaved
/// edits.
#[derive(Clone, Copy)]
pub struct EmptyCatalogGroupActionInput<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub uri: &'a Uri,
    pub cursor_range: &'a Range,
    pub file_lines: &'a [&'a str],
}

impl<'a> EmptyCatalogGroupActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        root: &'a Path,
        uri: &'a Uri,
        cursor_range: &'a Range,
        file_lines: &'a [&'a str],
    ) -> Self {
        Self {
            results,
            root,
            uri,
            cursor_range,
            file_lines,
        }
    }
}

pub fn build_remove_empty_catalog_group_actions(
    input: EmptyCatalogGroupActionInput<'_>,
) -> Vec<CodeActionOrCommand> {
    let EmptyCatalogGroupActionInput {
        results,
        root,
        uri,
        cursor_range,
        file_lines,
    } = input;
    let mut actions = Vec::new();

    if file_lines.is_empty() {
        return actions;
    }

    for group in &results.empty_catalog_groups {
        let group = &group.group;
        let Some((group_line, plan)) =
            empty_catalog_group_delete_line(group, root, uri, cursor_range, file_lines)
        else {
            continue;
        };
        actions.push(remove_empty_catalog_group_action(
            group, uri, file_lines, group_line, &plan,
        ));
    }

    actions
}

#[cfg(test)]
fn build_remove_empty_catalog_group_actions_for_test(
    results: &AnalysisResults,
    root: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Vec<CodeActionOrCommand> {
    build_remove_empty_catalog_group_actions(EmptyCatalogGroupActionInput::new(
        results,
        root,
        uri,
        cursor_range,
        file_lines,
    ))
}

/// Validate that an empty-catalog-group finding is within the cursor range
/// and that the open buffer still has the empty group on the line of the
/// finding. Returns the 0-based line of the group and its edit plan.
fn empty_catalog_group_delete_line(
    group: &fallow_api::editor_results::EmptyCatalogGroup,
    root: &Path,
    uri: &Uri,
    cursor_range: &Range,
    file_lines: &[&str],
) -> Option<(u32, CatalogEditPlan)> {
    if !is_pnpm_catalog_source(&group.path) {
        return None;
    }
    let group_uri = Uri::from_file_path(root.join(&group.path))?;
    if group_uri != *uri {
        return None;
    }

    let group_line = group.line.saturating_sub(1);
    if group_line < cursor_range.start.line || group_line > cursor_range.end.line {
        return None;
    }

    if group_line as usize >= file_lines.len() {
        return None;
    }
    let removal = CatalogRemoval::Group {
        catalog: &group.catalog_name,
    };
    plan_catalog_removal_at(file_lines, removal, group_line).map(|plan| (group_line, plan))
}

/// Build the deletion code action for an empty catalog group.
#[expect(
    clippy::disallowed_types,
    reason = "WorkspaceEdit.changes is typed as std::collections::HashMap by ls-types"
)]
fn remove_empty_catalog_group_action(
    group: &fallow_api::editor_results::EmptyCatalogGroup,
    uri: &Uri,
    file_lines: &[&str],
    group_line: u32,
    plan: &CatalogEditPlan,
) -> CodeActionOrCommand {
    let title = format!("Remove empty catalog group `{}`", group.catalog_name);

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), catalog_text_edits(file_lines, plan));

    let diagnostic_message = format!(
        "Empty catalog group: '{}' has no entries",
        group.catalog_name
    );

    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![Diagnostic {
            range: Range {
                start: Position {
                    line: group_line,
                    character: 0,
                },
                end: Position {
                    line: group_line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("empty-catalog-group".to_string())),
            message: diagnostic_message,
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        }]),
        ..Default::default()
    })
}

/// Plans `removal` on the open buffer with `plan_catalog_edits`. Returns
/// the plan only when the key is on `expected_line` (0-based), the line of
/// the diagnostic. The quick fix never deletes a comment block, even when
/// `fallow fix` uses the default `fix.catalog.deletePrecedingComments =
/// "auto"` policy.
fn plan_catalog_removal_at(
    file_lines: &[&str],
    removal: CatalogRemoval<'_>,
    expected_line: u32,
) -> Option<CatalogEditPlan> {
    let source = file_lines.join("\n");
    let plan =
        plan_catalog_edits(&source, &[removal], CatalogPrecedingCommentPolicy::Never).ok()?;
    match plan.outcomes.first()? {
        CatalogRemovalOutcome::Planned(planned) if planned.key_line == expected_line as usize => {
            Some(plan)
        }
        _ => None,
    }
}

/// Typed input for unused file delete quick-fix code actions.
#[derive(Clone, Copy)]
pub struct DeleteFileActionInput<'a> {
    pub results: &'a AnalysisResults,
    pub file_path: &'a Path,
    pub uri: &'a Uri,
    pub cursor_range: &'a Range,
}

impl<'a> DeleteFileActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        file_path: &'a Path,
        uri: &'a Uri,
        cursor_range: &'a Range,
    ) -> Self {
        Self {
            results,
            file_path,
            uri,
            cursor_range,
        }
    }
}

/// Build quick-fix code actions for unused files (delete the file).
///
/// This one performs a `DeleteFile` RESOURCE operation, the most destructive
/// edit the server can hand an editor, so it honours the same gate as the
/// export removal: a file whose unreachability verdict rests on a source the
/// run never read offers no delete action at all.
pub fn build_delete_file_actions(input: DeleteFileActionInput<'_>) -> Vec<CodeActionOrCommand> {
    let DeleteFileActionInput {
        results,
        file_path,
        uri,
        cursor_range,
    } = input;
    let mut actions = Vec::new();

    for file in &results.unused_files {
        if file.file.path != file_path {
            continue;
        }

        if !file.may_auto_apply_mutation() {
            continue;
        }

        if cursor_range.start.line > 0 {
            continue;
        }

        let title = "Delete this unused file".to_string();

        let delete_file_op = DocumentChangeOperation::Op(ResourceOp::Delete(DeleteFile {
            uri: uri.clone(),
            options: Some(DeleteFileOptions {
                recursive: Some(false),
                ignore_if_not_exists: Some(true),
            }),
            annotation_id: None,
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
fn build_delete_file_actions_for_test(
    results: &AnalysisResults,
    file_path: &Path,
    uri: &Uri,
    cursor_range: &Range,
) -> Vec<CodeActionOrCommand> {
    build_delete_file_actions(DeleteFileActionInput::new(
        results,
        file_path,
        uri,
        cursor_range,
    ))
}

#[cfg(test)]
#[path = "quick_fix_identifier_tests.rs"]
mod identifier_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_api::editor_results::{
        UnusedExport, UnusedFile, UnusedFileFinding, UnusedTypeFinding,
    };

    pub(super) fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    pub(super) fn make_range(start_line: u32, end_line: u32) -> Range {
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

    pub(super) fn make_unused_export(
        path: &Path,
        name: &str,
        line: u32,
        col: u32,
    ) -> fallow_api::editor_results::UnusedExportFinding {
        fallow_api::editor_results::UnusedExportFinding::with_actions(UnusedExport {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    }

    pub(super) fn unwrap_code_action(action: &CodeActionOrCommand) -> &CodeAction {
        match action {
            CodeActionOrCommand::CodeAction(ca) => ca,
            CodeActionOrCommand::Command(_) => panic!("expected CodeAction, got Command"),
        }
    }

    #[test]
    fn no_export_actions_when_results_empty() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let results = AnalysisResults::default();
        let lines = vec!["export const foo = 1;"];

        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_export_actions_for_different_file() {
        let root = test_root();
        let file_a = root.join("a.ts");
        let file_b = root.join("b.ts");
        let uri_b = Uri::from_file_path(&file_b).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file_a, "foo", 1, 7));

        let lines = vec!["export const foo = 1;"];
        let actions = build_remove_export_actions_for_test(
            &results,
            &file_b,
            &uri_b,
            &make_range(0, 10),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn no_export_actions_when_cursor_outside_export_line() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "bar", 5, 7));

        let lines = vec!["line0", "line1", "line2", "line3", "export const bar = 2;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 2), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn generates_action_for_unused_export_const() {
        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 13));

        let lines = vec!["export const foo = 42;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        assert_eq!(ca.title, "Remove unused export `foo`");
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));

        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 7);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn generates_action_for_export_default() {
        let root = test_root();
        let file = root.join("component.tsx");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "default", 1, 0));

        let lines = vec!["export default function App() {}"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.character, 15);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn preserves_indentation_in_edit_range() {
        let root = test_root();
        let file = root.join("nested.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 2, 20));

        let lines = vec![
            "const before = 1;",
            "    export function helper() {}", // 4 spaces indent
            "const after = 2;",
        ];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(1, 1), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.start.character, 4);
        assert_eq!(edits[0].range.end.character, 11);
    }

    #[test]
    fn handles_type_exports() {
        let root = test_root();
        let file = root.join("types.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: file.clone(),
                export_name: "MyType".to_string(),
                is_type_only: true,
                line: 1,
                col: 12,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));

        let lines = vec!["export type MyType = string;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

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
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 13));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: file.clone(),
                export_name: "Bar".to_string(),
                is_type_only: true,
                line: 2,
                col: 12,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));

        let lines = vec!["export const foo = 1;", "export type Bar = string;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 1), &lines);

        assert_eq!(actions.len(), 2);

        let ca0 = unwrap_code_action(&actions[0]);
        let ca1 = unwrap_code_action(&actions[1]);

        assert_eq!(ca0.title, "Remove unused export `foo`");
        assert_eq!(ca1.title, "Remove unused export `Bar`");

        let diag0 = &ca0.diagnostics.as_ref().unwrap()[0];
        let diag1 = &ca1.diagnostics.as_ref().unwrap()[0];
        assert!(diag0.message.starts_with("Export "));
        assert!(diag1.message.starts_with("Type export "));
    }

    #[test]
    fn skips_line_without_export_prefix() {
        let root = test_root();
        let file = root.join("odd.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 0));

        let lines = vec!["const foo = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "Should skip exports where line doesn't start with 'export'"
        );
    }

    #[test]
    fn handles_export_on_line_0_saturating_sub() {
        let root = test_root();
        let file = root.join("edge.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "x", 0, 7));

        let lines = vec!["export const x = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn multiple_exports_same_file_all_in_range() {
        let root = test_root();
        let file = root.join("multi.ts");
        let uri = Uri::from_file_path(&file).unwrap();

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
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 2), &lines);

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
        let uri = Uri::from_file_path(&file).unwrap();

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
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(2, 2), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        assert_eq!(ca.title, "Remove unused export `b`");
    }

    #[test]
    fn diagnostic_range_matches_export_name_span() {
        let root = test_root();
        let file = root.join("span.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "myLongExport", 1, 13));

        let lines = vec!["export const myLongExport = 42;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

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
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "x", 1, 0));

        let lines: Vec<&str> = vec![];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(actions.is_empty());
    }

    #[test]
    fn handles_tab_indentation() {
        let root = test_root();
        let file = root.join("tabs.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "val", 1, 0));

        let lines = vec!["\t\texport const val = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].range.start.character, 2);
        assert_eq!(edits[0].range.end.character, 9);
    }

    /// Apply a single-line `TextEdit` to `line` by splicing UTF-16 code
    /// units, mirroring how an LSP client interprets `character` offsets.
    pub(super) fn apply_single_line_edit(line: &str, edit: &TextEdit) -> String {
        let units: Vec<u16> = line.encode_utf16().collect();
        let start = edit.range.start.character as usize;
        let end = edit.range.end.character as usize;
        let mut spliced = units[..start].to_vec();
        spliced.extend(edit.new_text.encode_utf16());
        spliced.extend_from_slice(&units[end..]);
        String::from_utf16(&spliced).unwrap()
    }

    #[test]
    fn handles_nbsp_indentation_with_utf16_offsets() {
        let root = test_root();
        let file = root.join("nbsp.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 1, 0));

        // U+00A0 is 2 bytes in UTF-8 but 1 UTF-16 code unit: the indent
        // width in LSP `character` units is 1, not 2.
        let lines = vec!["\u{00A0}export const helper = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].range.start.character, 1);
        assert_eq!(edits[0].range.end.character, 8);
        assert_eq!(
            apply_single_line_edit(lines[0], &edits[0]),
            "\u{00A0}const helper = 1;"
        );
    }

    #[test]
    fn ascii_indentation_edit_applies_cleanly() {
        let root = test_root();
        let file = root.join("ascii.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 1, 0));

        let lines = vec!["  export const helper = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);
        let changes = ca.edit.as_ref().unwrap().changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits[0].range.start.character, 2);
        assert_eq!(edits[0].range.end.character, 9);
        assert_eq!(
            apply_single_line_edit(lines[0], &edits[0]),
            "  const helper = 1;"
        );
    }

    #[test]
    fn no_delete_actions_when_no_unused_files() {
        let root = test_root();
        let file = root.join("used.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let results = AnalysisResults::default();

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 10));
        assert!(actions.is_empty());
    }

    #[test]
    fn no_delete_action_for_different_file() {
        let root = test_root();
        let file_a = root.join("a.ts");
        let file_b = root.join("b.ts");
        let uri_b = Uri::from_file_path(&file_b).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile { path: file_a }));

        let actions =
            build_delete_file_actions_for_test(&results, &file_b, &uri_b, &make_range(0, 10));
        assert!(actions.is_empty());
    }

    #[test]
    fn no_delete_action_when_cursor_not_at_line_0() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(1, 5));
        assert!(actions.is_empty());
    }

    #[test]
    fn generates_delete_action_when_cursor_at_line_0() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 0));

        assert_eq!(actions.len(), 1);
        let ca = unwrap_code_action(&actions[0]);

        assert_eq!(ca.title, "Delete this unused file");
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
    }

    #[test]
    fn delete_action_uses_document_changes_with_delete_op() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 0));
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
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 0));
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
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 50));
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn multiple_unused_files_same_path_produces_multiple_actions() {
        let root = test_root();
        let file = root.join("unused.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));

        let actions = build_delete_file_actions_for_test(&results, &file, &uri, &make_range(0, 0));
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn unused_file_and_unused_export_in_same_file() {
        let root = test_root();
        let file = root.join("orphan.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));
        results
            .unused_exports
            .push(make_unused_export(&file, "helper", 1, 16));

        let lines = vec!["export function helper() {}"];
        let cursor = make_range(0, 0);

        let export_actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &cursor, &lines);
        let delete_actions = build_delete_file_actions_for_test(&results, &file, &uri, &cursor);

        assert_eq!(export_actions.len(), 1);
        assert_eq!(delete_actions.len(), 1);

        let export_ca = unwrap_code_action(&export_actions[0]);
        let delete_ca = unwrap_code_action(&delete_actions[0]);
        assert!(export_ca.title.contains("Remove unused export"));
        assert!(delete_ca.title.contains("Delete"));
    }

    use fallow_api::editor_results::{UnusedCatalogEntry, UnusedCatalogEntryFinding};

    fn make_catalog_entry(
        name: &str,
        catalog: &str,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntryFinding {
        make_catalog_entry_in_file(
            name,
            catalog,
            PathBuf::from("pnpm-workspace.yaml"),
            line,
            consumers,
        )
    }

    fn make_catalog_entry_in_file(
        name: &str,
        catalog: &str,
        path: PathBuf,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path,
            line,
            hardcoded_consumers: consumers,
        })
    }

    fn workspace_yaml_uri(dir: &tempfile::TempDir) -> Uri {
        Uri::from_file_path(dir.path().join("pnpm-workspace.yaml")).unwrap()
    }

    #[test]
    fn no_catalog_action_when_no_findings() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);
        let results = AnalysisResults::default();
        let lines = vec!["catalog:", "  is-even: ^1.0.0"];

        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 2, vec![]));

        let lines = vec!["catalog:", "  is-even: ^1.0.0"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let actions = build_remove_catalog_entry_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_skips_bun_package_json_catalog_entries() {
        let dir = tempfile::tempdir().unwrap();
        let uri = Uri::from_file_path(dir.path().join("package.json")).unwrap();

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry_in_file(
                "unused",
                "default",
                PathBuf::from("package.json"),
                4,
                vec![],
            ));

        let lines = vec![
            "{",
            "  \"workspaces\": {",
            "    \"catalog\": {",
            "      \"unused\": \"^1.0.0\"",
            "    }",
            "  }",
            "}",
        ];
        let actions = build_remove_catalog_entry_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(3, 3),
            &lines,
        );
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
        let actions = build_remove_catalog_entry_actions_for_test(
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
        assert_eq!(edits[0].range.start.line, 1);
        assert_eq!(edits[0].range.end.line, 4);
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn catalog_action_bails_when_line_does_not_match_entry_name() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 2, vec![]));

        let lines = vec!["catalog:", "  different-entry: ^2.0.0"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "default", 2, vec![]));

        let lines = vec!["catalog:", "  react-native: ^0.73.0"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "default", 2, vec![]));

        let lines = vec!["catalog:", "  description: react fork"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("@scope/foo", "default", 2, vec![]));

        let lines = vec!["catalog:", "  \"@scope/foo\": ^1.0.0"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        results
            .unused_catalog_entries
            .push(make_catalog_entry("is-even", "default", 3, vec![]));

        let lines = vec!["catalog:", "  is-odd: ^1.0.0", "  is-even: ^1.0.0"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "react17", 3, vec![]));

        let lines = vec!["catalogs:", "  react17:", "    react: ^17.0.2"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "react17", 3, vec![]));

        let lines = vec!["catalogs:", "  react17:", "    react: ^17.0.2"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        assert_eq!(edits[0].range.start.line, 2);
        assert_eq!(edits[0].new_text, "");
        assert_eq!(edits[1].range.start.line, 1);
        assert_eq!(edits[1].new_text, "  react17: {}");
    }

    #[test]
    fn catalog_action_parent_rewrite_uses_utf16_character_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(make_catalog_entry("react", "日本", 3, vec![]));

        let lines = vec!["catalogs:", "  \"日本\":", "    react: ^17.0.2"];
        let actions = build_remove_catalog_entry_actions_for_test(
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
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].range.start.character, 0);
        assert_eq!(
            edits[1].range.end.character,
            lines[1].encode_utf16().count() as u32
        );
        assert_eq!(edits[1].new_text, "  \"日本\": {}");
    }

    #[test]
    fn catalog_action_no_parent_rewrite_when_siblings_remain() {
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
        let actions = build_remove_catalog_entry_actions_for_test(
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
        let actions = build_remove_catalog_entry_actions_for_test(
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

    use fallow_api::editor_results::{EmptyCatalogGroup, EmptyCatalogGroupFinding};

    fn make_empty_group(name: &str, line: u32) -> EmptyCatalogGroupFinding {
        make_empty_group_in_file(name, PathBuf::from("pnpm-workspace.yaml"), line)
    }

    fn make_empty_group_in_file(name: &str, path: PathBuf, line: u32) -> EmptyCatalogGroupFinding {
        EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: name.to_string(),
            path,
            line,
        })
    }

    #[test]
    fn empty_catalog_group_action_deletes_single_header_line() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("legacy", 3));

        let lines = vec!["catalogs:", "  react17: {}", "  legacy:"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(2, 2),
            &lines,
        );

        assert_eq!(actions.len(), 1, "expected one action for legacy");
        let ca = unwrap_code_action(&actions[0]);
        assert_eq!(ca.title, "Remove empty catalog group `legacy`");
        assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));

        let edit = ca.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = changes.get(&uri).unwrap();
        assert_eq!(edits.len(), 1, "empty groups need only one TextEdit");
        assert_eq!(edits[0].range.start.line, 2);
        assert_eq!(edits[0].range.start.character, 0);
        assert_eq!(edits[0].range.end.line, 3);
        assert_eq!(edits[0].range.end.character, 0);
        assert_eq!(edits[0].new_text, "");

        let diag = &ca.diagnostics.as_ref().unwrap()[0];
        assert_eq!(diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diag.source, Some("fallow".to_string()));
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("empty-catalog-group".to_string()))
        );
        assert_eq!(diag.message, "Empty catalog group: 'legacy' has no entries");
        assert_eq!(diag.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    /// The edits of each catalog action on `lines`, with the cursor on `line`.
    fn catalog_action_edits(
        results: &AnalysisResults,
        lines: &[&str],
        line: u32,
    ) -> Vec<Vec<TextEdit>> {
        let dir = tempfile::tempdir().unwrap();
        let (uri, range) = (workspace_yaml_uri(&dir), make_range(line, line));
        let mut actions =
            build_remove_catalog_entry_actions_for_test(results, dir.path(), &uri, &range, lines);
        actions.extend(build_remove_empty_catalog_group_actions_for_test(
            results,
            dir.path(),
            &uri,
            &range,
            lines,
        ));
        let edits_of = |action| {
            let edit = unwrap_code_action(action).edit.as_ref().unwrap();
            edit.changes.as_ref().unwrap()[&uri].clone()
        };
        actions.iter().map(edits_of).collect()
    }

    fn entry_results(name: &str, catalog: &str, line: u32) -> AnalysisResults {
        let mut results = AnalysisResults::default();
        let entry = make_catalog_entry(name, catalog, line, vec![]);
        results.unused_catalog_entries.push(entry);
        results
    }

    fn group_results(name: &str, line: u32) -> AnalysisResults {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group(name, line));
        results
    }

    #[test]
    fn empty_catalog_group_action_deletes_every_line_of_the_group() {
        let lines = [
            "catalogs:",
            "  old: !!map {",
            "    }",
            "  ui:",
            "    react: ^18",
        ];
        let actions = catalog_action_edits(&group_results("old", 2), &lines, 1);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].len(), 1);
        assert_eq!(actions[0][0].range.start.line, 1);
        assert_eq!(actions[0][0].range.end.line, 3);
    }

    #[test]
    fn empty_catalog_group_action_skipped_when_live_group_has_children() {
        let lines = ["catalogs:", "  legacy:", "    react: ^18"];
        let actions = catalog_action_edits(&group_results("legacy", 2), &lines, 1);
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_skipped_when_other_catalog_owns_the_key() {
        let lines = ["catalogs:", "  react18:", "    react: ^18"];
        let actions = catalog_action_edits(&entry_results("react", "react17", 3), &lines, 2);
        assert!(actions.is_empty());
    }

    #[test]
    fn catalog_action_parent_rewrite_goes_before_inline_comment() {
        let lines = ["catalogs:", "  ui: # pinned versions", "    react: ^18"];
        let actions = catalog_action_edits(&entry_results("react", "ui", 3), &lines, 2);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].len(), 2);
        assert_eq!(actions[0][1].new_text, "  ui: {} # pinned versions");
    }

    #[test]
    fn catalog_action_deletes_object_entry_with_blank_line() {
        let lines = [
            "catalog:",
            "  react:",
            "",
            "    specifier: ^18",
            "  vue: ^3",
        ];
        let actions = catalog_action_edits(&entry_results("react", "default", 2), &lines, 1);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].len(), 1);
        assert_eq!(actions[0][0].range.start.line, 1);
        assert_eq!(actions[0][0].range.end.line, 4);
    }

    #[test]
    fn empty_catalog_group_action_skips_bun_package_json_groups() {
        let dir = tempfile::tempdir().unwrap();
        let uri = Uri::from_file_path(dir.path().join("package.json")).unwrap();

        let mut results = AnalysisResults::default();
        results.empty_catalog_groups.push(make_empty_group_in_file(
            "empty",
            PathBuf::from("package.json"),
            4,
        ));

        let lines = vec![
            "{",
            "  \"workspaces\": {",
            "    \"catalogs\": {",
            "      \"empty\": {}",
            "    }",
            "  }",
            "}",
        ];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(3, 3),
            &lines,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn empty_catalog_group_action_skips_when_uri_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let other_uri = Uri::from_file_path(dir.path().join("not-the-workspace.yaml")).unwrap();

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("legacy", 3));

        let lines = vec!["catalogs:", "  react17: {}", "  legacy:"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &other_uri,
            &make_range(0, 100),
            &lines,
        );
        assert!(actions.is_empty(), "must not offer fix for unrelated URI");
    }

    #[test]
    fn empty_catalog_group_action_skips_when_outside_cursor_range() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("legacy", 3));

        let lines = vec!["catalogs:", "  react17: {}", "  legacy:"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(0, 0),
            &lines,
        );
        assert!(
            actions.is_empty(),
            "must not offer fix when cursor is outside the finding's line"
        );
    }

    #[test]
    fn empty_catalog_group_action_skips_when_prefix_collision() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("react17", 2));

        let lines = vec!["catalogs:", "  react18:"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert!(
            actions.is_empty(),
            "anchored match must reject `react17` finding when line says `react18:`"
        );
    }

    #[test]
    fn empty_catalog_group_action_handles_quoted_names() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("@scope/legacy", 2));

        let lines = vec!["catalogs:", "  \"@scope/legacy\":"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(1, 1),
            &lines,
        );
        assert_eq!(actions.len(), 1, "quoted catalog names must match");
        let ca = unwrap_code_action(&actions[0]);
        assert!(ca.title.contains("@scope/legacy"));
    }

    #[test]
    fn empty_catalog_group_action_handles_multiple_findings() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("react17", 2));
        results
            .empty_catalog_groups
            .push(make_empty_group("legacy", 3));

        let lines = vec!["catalogs:", "  react17:", "  legacy:"];
        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(0, 100),
            &lines,
        );
        assert_eq!(actions.len(), 2);
        let titles: Vec<&str> = actions
            .iter()
            .map(|a| unwrap_code_action(a).title.as_str())
            .collect();
        assert!(titles.iter().any(|t| t.contains("react17")));
        assert!(titles.iter().any(|t| t.contains("legacy")));
    }

    #[test]
    fn empty_catalog_group_action_short_circuits_on_empty_file_lines() {
        let dir = tempfile::tempdir().unwrap();
        let uri = workspace_yaml_uri(&dir);

        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(make_empty_group("legacy", 3));

        let actions = build_remove_empty_catalog_group_actions_for_test(
            &results,
            dir.path(),
            &uri,
            &make_range(0, 100),
            &[],
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn build_remove_export_actions_skips_stale_line() {
        let root = test_root();
        let file = root.join("src/utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(make_unused_export(
            &file, "foo", 1, // 1-based
            7,
        ));
        let lines = vec!["const foo = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "expected zero actions when live line lacks export prefix",
        );
    }

    #[test]
    fn build_remove_export_actions_skips_substring_collision() {
        let root = test_root();
        let file = root.join("src/utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(make_unused_export(
            &file, "foo", 1, // 1-based
            7,
        ));
        let lines = vec!["export const foobar = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "expected zero actions when cached name only matches as a substring",
        );
    }

    #[test]
    fn build_remove_export_actions_skips_value_reference_collision() {
        let root = test_root();
        let file = root.join("src/utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(make_unused_export(
            &file, "foo", 1, // 1-based
            7,
        ));
        let lines = vec!["export const bar = foo;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "expected zero actions when cached name appears as a value, not a declaration",
        );
    }

    #[test]
    fn build_remove_export_actions_skips_reexport_block() {
        let root = test_root();
        let file = root.join("src/utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(make_unused_export(
            &file, "foo", 1, // 1-based
            9,
        ));
        let lines = vec!["export { foo } from './bar';"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert!(
            actions.is_empty(),
            "expected zero actions on re-export blocks (action's output would be a syntax error)",
        );
    }

    #[test]
    fn build_remove_export_actions_accepts_matching_live_line() {
        let root = test_root();
        let file = root.join("src/utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(make_unused_export(
            &file, "foo", 1, // 1-based
            7,
        ));
        let lines = vec!["export const foo = 1;"];
        let actions =
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines);
        assert_eq!(
            actions.len(),
            1,
            "expected one action when live line still matches the cached finding",
        );
    }

    /// BLOCKER: the quick fix builds its own `WorkspaceEdit` in process, so
    /// the CLI-side withholding protected nothing here. The editor is the
    /// surface where the user is one keystroke from applying it.
    #[test]
    fn a_caveated_export_offers_no_remove_quick_fix() {
        use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

        let root = test_root();
        let file = root.join("utils.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let lines = vec!["export const foo = 1;"];

        let mut clean = AnalysisResults::default();
        clean
            .unused_exports
            .push(make_unused_export(&file, "foo", 1, 13));
        assert_eq!(
            build_remove_export_actions_for_test(&clean, &file, &uri, &make_range(0, 0), &lines)
                .len(),
            1,
            "a run with the evidence still offers the quick fix"
        );

        let mut caveated = AnalysisResults::default();
        let mut finding = make_unused_export(&file, "foo", 1, 13);
        finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        caveated.unused_exports.push(finding);

        assert!(
            build_remove_export_actions_for_test(&caveated, &file, &uri, &make_range(0, 0), &lines)
                .is_empty(),
            "a file the run never read may hold the import that credits this export"
        );
    }

    /// Type exports go through the same quick-fix builder, so the gate has to
    /// reach `unused_types[]` too rather than only the value-export array.
    #[test]
    fn a_caveated_type_export_offers_no_remove_quick_fix() {
        use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

        let root = test_root();
        let file = root.join("types.ts");
        let uri = Uri::from_file_path(&file).unwrap();
        let lines = vec!["export type Foo = string;"];

        let mut results = AnalysisResults::default();
        let mut finding = UnusedTypeFinding::with_actions(UnusedExport {
            path: file.clone(),
            export_name: "Foo".to_string(),
            is_type_only: true,
            line: 1,
            col: 12,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        });
        finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        results.unused_types.push(finding);

        assert!(
            build_remove_export_actions_for_test(&results, &file, &uri, &make_range(0, 0), &lines)
                .is_empty(),
            "an unused type and an unused export must render with the same confidence"
        );
    }

    /// The delete action is a `DeleteFile` RESOURCE operation, the most
    /// destructive edit the server can hand an editor.
    #[test]
    fn a_caveated_unused_file_offers_no_delete_quick_fix() {
        use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

        let root = test_root();
        let file = root.join("orphan.ts");
        let uri = Uri::from_file_path(&file).unwrap();

        let mut clean = AnalysisResults::default();
        clean
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: file.clone(),
            }));
        assert_eq!(
            build_delete_file_actions_for_test(&clean, &file, &uri, &make_range(0, 0)).len(),
            1,
            "a run with the evidence still offers the delete"
        );

        let mut caveated = AnalysisResults::default();
        let mut finding = UnusedFileFinding::with_actions(UnusedFile { path: file.clone() });
        finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        caveated.unused_files.push(finding);

        assert!(
            build_delete_file_actions_for_test(&caveated, &file, &uri, &make_range(0, 0))
                .is_empty(),
            "an unreachability verdict resting on an unread file must not offer a file deletion"
        );
    }
}
