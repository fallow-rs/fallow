//! Config, ignore and plugin credits for unused-export analysis, with the
//! CommonJS default-object rule.

use fallow_types::extract::ExportName;

use crate::graph::ModuleNode;
use crate::plugins::{ConfigExportCredit, is_export_ignored};

use super::UnusedExportMatchContext;

/// Whether a config credit, an `ignoreExports` rule or a plugin rule covers
/// `export_name` in `module`.
pub(super) fn is_credited(
    module: &ModuleNode,
    export_name: &str,
    match_ctx: &UnusedExportMatchContext<'_>,
) -> bool {
    ConfigExportCredit::credits(match_ctx.config_credited_exports, export_name)
        || is_export_ignored(
            export_name,
            match_ctx.matching_ignore,
            match_ctx.matching_plugin,
        )
        || tool_reads_default_object(module, match_ctx)
}

/// A tool that reads only the default export of a CommonJS module reads the
/// whole `module.exports` object, so each of its keys is used (#3876).
///
/// Only plugin and config credits that list exactly `default` apply. User
/// `ignoreExports` rules stay exact-name, and an explicit default export means
/// that the keys are not that object.
fn tool_reads_default_object(
    module: &ModuleNode,
    match_ctx: &UnusedExportMatchContext<'_>,
) -> bool {
    module.has_cjs_exports()
        && !module
            .exports
            .iter()
            .any(|export| matches!(export.name, ExportName::Default))
        && (match_ctx
            .matching_plugin
            .iter()
            .any(|exports| *exports == ["default"])
            || match_ctx
                .config_credited_exports
                .iter()
                .any(|exports| matches!(exports, [only] if only == "default")))
}
