mod absent_props;
mod quick_fix;
mod suppress;

use std::path::Path;

use fallow_api::EditorAnalysisResults as AnalysisResults;
#[allow(clippy::wildcard_imports, reason = "many LSP types used")]
use ls_types::*;

pub use absent_props::*;
pub use quick_fix::*;
pub use suppress::*;

#[derive(Clone, Copy)]
pub struct CodeActionInput<'a> {
    results: &'a AnalysisResults,
    root: Option<&'a Path>,
    file_path: &'a Path,
    uri: &'a Uri,
    range: &'a Range,
    file_lines: &'a [&'a str],
    absent_prop_source: Option<&'a str>,
}

impl<'a> CodeActionInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        root: Option<&'a Path>,
        file_path: &'a Path,
        uri: &'a Uri,
        range: &'a Range,
        file_lines: &'a [&'a str],
    ) -> Self {
        Self {
            results,
            root,
            file_path,
            uri,
            range,
            file_lines,
            absent_prop_source: None,
        }
    }
    /// Enable candidate actions only after request-boundary document freshness checks.
    #[must_use]
    pub const fn with_current_absent_prop_source(mut self, source: &'a str) -> Self {
        self.absent_prop_source = Some(source);
        self
    }
}

#[must_use]
pub fn build_code_action_response(input: CodeActionInput<'_>) -> Option<CodeActionResponse> {
    let CodeActionInput {
        results,
        root,
        file_path,
        uri,
        range,
        file_lines,
        absent_prop_source,
    } = input;
    let mut actions = Vec::new();

    actions.extend(build_remove_export_actions(RemoveExportActionInput::new(
        results, file_path, uri, range, file_lines,
    )));
    actions.extend(build_delete_file_actions(DeleteFileActionInput::new(
        results, file_path, uri, range,
    )));

    if let Some(root) = root {
        actions.extend(build_remove_catalog_entry_actions(
            CatalogEntryActionInput::new(results, root, uri, range, file_lines),
        ));
        actions.extend(build_remove_empty_catalog_group_actions(
            EmptyCatalogGroupActionInput::new(results, root, uri, range, file_lines),
        ));
    }

    if let Some(source) = absent_prop_source {
        actions.extend(build_suppress_absent_prop_actions(
            results, file_path, uri, range, source,
        ));
    }

    actions.extend(build_suppress_security_actions(
        SuppressSecurityActionInput::new(results, file_path, uri, range, file_lines),
    ));

    (!actions.is_empty()).then_some(actions)
}
