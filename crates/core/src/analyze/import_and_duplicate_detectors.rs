//! The unresolved-import, duplicate-export and duplicate-export-alias
//! detectors of the parallel dead-code pass.

use super::*;

pub(super) fn run_import_and_duplicate_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (
    Vec<UnresolvedImportFinding>,
    (
        Vec<DuplicateExportFinding>,
        Vec<DuplicateExportAliasFinding>,
    ),
) {
    rayon::join(
        || {
            run_unresolved_import_detector(UnresolvedImportDetectorInput {
                resolved_modules: input.resolved_modules,
                config: input.config,
                suppressions: input.suppressions,
                virtual_prefixes: input.virtual_prefixes,
                generated_patterns: input.generated_patterns,
                generated_type_prefixes: input.generated_type_prefixes,
                line_offsets_by_file: input.line_offsets_by_file,
            })
        },
        || {
            (
                run_duplicate_export_detector(
                    input.graph,
                    input.config,
                    input.suppressions,
                    input.line_offsets_by_file,
                    input.plugin_result,
                    input.resolved_modules,
                ),
                run_duplicate_export_alias_detector(input),
            )
        },
    )
}

fn run_duplicate_export_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    resolved_modules: &[ResolvedModule],
) -> Vec<DuplicateExportFinding> {
    if config.rules.duplicate_exports == Severity::Off {
        return Vec::new();
    }
    let duplicate_exports = if let Some(plugin_result) = plugin_result {
        unused_exports::find_duplicate_exports_with_plugins(
            graph,
            config,
            suppressions,
            line_offsets_by_file,
            Some(plugin_result),
            resolved_modules,
        )
    } else {
        unused_exports::find_duplicate_exports(
            graph,
            config,
            suppressions,
            line_offsets_by_file,
            resolved_modules,
        )
    };
    duplicate_exports
        .into_iter()
        .map(DuplicateExportFinding::with_actions)
        .collect()
}

fn run_duplicate_export_alias_detector(
    input: DeadCodeDetectorInput<'_>,
) -> Vec<DuplicateExportAliasFinding> {
    if input.config.rules.duplicate_export_aliases == Severity::Off {
        return Vec::new();
    }
    duplicate_export_aliases::find_duplicate_export_aliases(
        input.graph,
        input.config,
        input.suppressions,
        input.line_offsets_by_file,
        input.resolved_modules,
    )
    .into_iter()
    .map(DuplicateExportAliasFinding::with_actions)
    .collect()
}
