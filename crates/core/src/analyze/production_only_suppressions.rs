//! Keep a suppression that only production mode needs out of the stale list.
//!
//! Production mode excludes test, story, and fixture files from discovery. An
//! export that only those files import is unused under `--production`, so its
//! `unused-export` or `unused-type` suppression is needed there. A run without
//! `--production` credits the test import and does not consume the
//! suppression. Reporting it stale there would leave no comment that is clean
//! in both modes (issue #3825).

use fallow_config::ResolvedConfig;
use fallow_types::extract::ExportName;
use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{ExportSymbol, ModuleGraph, ModuleNode};
use crate::results::StaleSuppression;
use crate::suppress::{IssueKind, Suppression};

use super::unused_deps::production_exclude_globset;
use super::{LineOffsetsMap, PostDetectionInput, byte_offset_to_line_col};

/// Collect the stale suppressions of this run, without a suppression that
/// only production mode needs.
pub(super) fn find_stale(input: &PostDetectionInput<'_, '_>) -> Vec<StaleSuppression> {
    let (graph, config) = (input.graph, input.config);
    input
        .suppressions
        .find_stale_except(graph, config, |file_id, suppression| {
            is_needed_in_production(
                graph,
                config,
                input.line_offsets_by_file,
                file_id,
                suppression,
            )
        })
}

/// Whether production mode would report an export that `suppression` covers.
///
/// Always `false` in production mode: that run consumes the suppression
/// directly when it is needed.
fn is_needed_in_production(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    file_id: FileId,
    suppression: &Suppression,
) -> bool {
    if config.production {
        return false;
    }
    let Some(module) = graph.modules.get(file_id.0 as usize) else {
        return false;
    };
    module.exports.iter().any(|export| {
        let (line, _) = byte_offset_to_line_col(line_offsets_by_file, file_id, export.span.start);
        let kind = if export.is_type_only {
            IssueKind::UnusedType
        } else {
            IssueKind::UnusedExport
        };
        suppression.matches_issue_kind(line, kind)
            && is_referenced_only_by_production_excluded_files(graph, config, export)
            && !is_re_exported_by_production_entry(graph, config, file_id, &export.name)
    })
}

/// Whether `export` has references and every one of them comes from a file
/// that production mode excludes from discovery.
fn is_referenced_only_by_production_excluded_files(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    export: &ExportSymbol,
) -> bool {
    !export.references.is_empty()
        && export.references.iter().all(|reference| {
            graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(|importer| is_production_excluded(config, importer))
        })
}

/// Whether an entry point that production mode keeps re-exports the export,
/// directly or through a chain of barrels.
///
/// Production mode credits the exports of an entry-point barrel that has no
/// consumer in the graph, so such an export is used there.
fn is_re_exported_by_production_entry(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    file_id: FileId,
    name: &ExportName,
) -> bool {
    let start = match name {
        ExportName::Named(name) => name.clone(),
        ExportName::Default => "default".to_owned(),
    };
    let mut seen: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut pending = vec![(file_id, start)];
    while let Some((source, name)) = pending.pop() {
        if !seen.insert((source, name.clone())) {
            continue;
        }
        for barrel in &graph.modules {
            if is_production_excluded(config, barrel) {
                continue;
            }
            for edge in &barrel.re_exports {
                let forwards = edge.source_file == source
                    && (edge.imported_name == name
                        || (edge.imported_name == "*" && name != "default"));
                if !forwards {
                    continue;
                }
                if barrel.is_entry_point() {
                    return true;
                }
                let next = if edge.exported_name == "*" {
                    name.clone()
                } else {
                    edge.exported_name.clone()
                };
                pending.push((barrel.file_id, next));
            }
        }
    }
    false
}

/// Whether production mode leaves `module` out of discovery.
fn is_production_excluded(config: &ResolvedConfig, module: &ModuleNode) -> bool {
    let relative = module
        .path
        .strip_prefix(&config.root)
        .unwrap_or(&module.path);
    production_exclude_globset().is_some_and(|excludes| excludes.is_match(relative))
}
