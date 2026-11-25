//! Exports that only test-side files use (issue #5252).
//!
//! The pass runs the unused-export rules a second time, with the references
//! from test-side files left out. An export that this run reports, and that
//! a test-side file references as a value, is test-only. Each other rule of
//! the unused-export pass applies unchanged: entry exports, `keepExportTags`,
//! `ignoreExports` and plugin `usedExports` rules keep the export, as they do
//! in a production run. Only a `test-only-export` marker suppresses a
//! finding, because an `unused-export` marker targets another kind.

use fallow_config::Severity;
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleNode};
use crate::suppress::SuppressionContext;

use super::namespace_escapes::export_index;
use super::{
    ReferenceView, UnusedExportInput, find_unused_exports_for_module, with_unused_export_context,
};

/// A value export that only test-side files use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestOnlyCandidate {
    /// The file that declares the export.
    pub file: FileId,
    /// The index of the export in the exports of its module.
    pub export_index: usize,
    /// The test-side files that reference the export, sorted by id.
    pub importers: Vec<FileId>,
}

/// Find the value exports of production files that only test-side files
/// use. Empty in a production run, because its graph has no test-side file.
pub fn find_test_only_export_candidates(input: UnusedExportInput<'_>) -> Vec<TestOnlyCandidate> {
    let config = input.config;
    if config.production {
        return Vec::new();
    }
    let graph = input.graph;
    let test_side = crate::test_side::test_side_files(graph, config);
    if test_side.is_empty() {
        return Vec::new();
    }
    let asked: Vec<(&ModuleNode, FxHashSet<usize>)> = graph
        .modules()
        .iter()
        .filter(|module| module.is_reachable() && !test_side.contains(&module.file_id))
        .filter(|module| {
            config
                .resolve_rules_for_path(&module.path)
                .test_only_exports
                != Severity::Off
        })
        .filter_map(|module| {
            let only: FxHashSet<usize> = module
                .exports
                .iter()
                .enumerate()
                .filter(|(_, export)| {
                    !export.is_type_only
                        && export.references.iter().any(|reference| {
                            reference.namespace == ExportNamespace::Value
                                && test_side.contains(&reference.from_file)
                        })
                })
                .map(|(index, _)| index)
                .collect();
            (!only.is_empty()).then_some((module, only))
        })
        .collect();
    if asked.is_empty() {
        return Vec::new();
    }

    // The caller checks the `test-only-export` suppressions. A check of the
    // `unused-export` suppressions here would mark them as consumed.
    let no_suppressions = SuppressionContext::empty();
    let input = UnusedExportInput {
        suppressions: &no_suppressions,
        ..input
    };
    let test_side = &test_side;
    let mut candidates: Vec<TestOnlyCandidate> =
        with_unused_export_context(input, ReferenceView::Without(test_side), None, |ctx| {
            asked
                .par_iter()
                .flat_map_iter(|(module, only)| {
                    let (exports, _, _) = find_unused_exports_for_module(module, ctx, Some(only));
                    exports.into_iter().filter_map(move |finding| {
                        let index = export_index(
                            module,
                            &finding.export_name,
                            finding.is_type_only,
                            finding.span_start,
                        )?;
                        let mut importers: Vec<FileId> = module.exports[index]
                            .references
                            .iter()
                            .map(|reference| reference.from_file)
                            .filter(|file| test_side.contains(file))
                            .collect();
                        importers.sort_unstable_by_key(|file| file.0);
                        importers.dedup();
                        Some(TestOnlyCandidate {
                            file: module.file_id,
                            export_index: index,
                            importers,
                        })
                    })
                })
                .collect()
        });
    drop_forwarding_barrels(graph, &mut candidates);
    candidates.sort_unstable_by_key(|candidate| (candidate.file.0, candidate.export_index));
    candidates
}

/// Drop each re-export of a barrel whose forwarded export is a candidate too.
/// A test that imports through a barrel references both the barrel entry and
/// the declaration, and the finding on the declaration is the actionable one.
fn drop_forwarding_barrels(
    graph: &crate::graph::ModuleGraph,
    candidates: &mut Vec<TestOnlyCandidate>,
) {
    let reported: FxHashSet<(FileId, String)> = candidates
        .iter()
        .filter_map(|candidate| {
            let module = graph.module(candidate.file)?;
            let export = module.exports.get(candidate.export_index)?;
            Some((candidate.file, export.name.to_string()))
        })
        .collect();
    candidates.retain(|candidate| {
        let Some(module) = graph.module(candidate.file) else {
            return true;
        };
        let Some(export) = module.exports.get(candidate.export_index) else {
            return true;
        };
        let name = export.name.to_string();
        !module.re_exports.iter().any(|edge| {
            edge.exported_name == name
                && reported.contains(&(edge.source_file, edge.imported_name.clone()))
        })
    });
}
