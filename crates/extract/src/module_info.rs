use crate::{ExportInfo, ImportInfo, ModuleInfo};
use fallow_types::discover::FileId;

pub struct NonJsModuleInfoInput<'a> {
    pub(crate) file_id: FileId,
    pub(crate) content_hash: u64,
    pub(crate) source: &'a str,
    pub(crate) parsed_suppressions: crate::suppress::ParsedSuppressions,
    pub(crate) imports: Vec<ImportInfo>,
    pub(crate) exports: Vec<ExportInfo>,
}

/// Build the shared empty-module baseline used by non-JS extractors.
///
/// Callers provide the fields that can be present for their surface. Everything
/// else stays empty because CSS, SFC shells, and other non-JS wrappers do not
/// directly contribute JS AST-level facts.
pub fn non_js_module_info(input: NonJsModuleInfoInput<'_>) -> ModuleInfo {
    ModuleInfo {
        exports: input.exports,
        imports: input.imports,
        content_hash: input.content_hash,
        suppressions: input.parsed_suppressions.suppressions,
        unknown_suppression_kinds: input.parsed_suppressions.unknown_kinds,
        line_offsets: fallow_types::extract::compute_line_offsets(input.source),
        ..ModuleInfo::empty(input.file_id)
    }
}
