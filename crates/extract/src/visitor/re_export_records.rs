//! Re-export records of `export { x } from './src'` statements.

use oxc_ast::ast::ExportFromDeclaration;

use super::ModuleInfoExtractor;
use crate::js_text::JsText;
use fallow_types::extract::{ReExportInfo, VisibilityTag};

impl ModuleInfoExtractor {
    /// Record `export { x } from './src'` re-export specifiers, abstaining the
    /// SvelteKit load-data harvest on a re-exported `load`.
    pub(super) fn record_export_re_exports(
        &mut self,
        decl: &ExportFromDeclaration<'_>,
        is_type_only: bool,
    ) {
        let source = &decl.source;
        for spec in &decl.specifiers {
            // `export { load } from './x'` re-exports the load: the terminal
            // object is not a direct literal here, so the load-data harvest
            // abstains on this file.
            if !is_type_only
                && !spec.export_kind.is_type()
                && spec.exported.name().as_str() == "load"
            {
                self.has_unharvestable_load = true;
            }
            self.re_exports.push(ReExportInfo {
                source: source.value.text().into_owned(),
                imported_name: spec.local.name().to_string(),
                exported_name: spec.exported.name().to_string(),
                is_type_only: is_type_only || spec.export_kind.is_type(),
                span: spec.span,
                statement_span: decl.span,
                source_span: source.span,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
            });
        }
    }
}
