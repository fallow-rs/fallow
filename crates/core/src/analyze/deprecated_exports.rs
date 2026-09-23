//! `deprecated-export-in-use`: exports marked `@deprecated` that still have a
//! reachable reference.
//!
//! A deprecated export with no reference is not reported here: the
//! unused-export detector reports it and carries the deprecation on that
//! finding, so one symbol produces one report.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::results::{
    DEPRECATED_CONSUMER_SAMPLE_CAP, DeprecatedConsumerKind, DeprecatedExportConsumer,
    DeprecatedExportInUse,
};

use crate::discover::FileId;
use crate::graph::{
    ExportSymbol, ModuleGraph, ModuleNode, ReExportEdge, ReferenceKind, SymbolReference,
};
use crate::suppress::{IssueKind, SuppressionContext};

use super::LineOffsetsMap;
use super::byte_offset_to_line_col;
use super::unused_exports::reference_location;

/// Find every `@deprecated` export that still has a consumer in a reachable
/// file.
pub fn find_deprecated_exports_in_use(
    graph: &ModuleGraph,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<DeprecatedExportInUse> {
    if !graph
        .modules
        .iter()
        .any(|module| module.exports.iter().any(|export| export.deprecated))
    {
        return Vec::new();
    }
    let file_paths: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|m| (m.file_id, m.path.as_path()))
        .collect();
    let re_exporters = re_exporters_by_source(graph);

    graph
        .modules
        .par_iter()
        .filter(|module| module.exports.iter().any(|export| export.deprecated))
        .flat_map_iter(|module| {
            let mut source_cache = FxHashMap::default();
            module
                .exports
                .iter()
                .filter_map(|export| {
                    deprecated_export_finding(
                        graph,
                        module,
                        export,
                        &DetectorContext {
                            file_paths: &file_paths,
                            re_exporters: &re_exporters,
                            suppressions,
                            line_offsets_by_file,
                        },
                        &mut source_cache,
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// For each module, the modules that re-export from it, with the edge.
type ReExporters<'a> = FxHashMap<FileId, Vec<(FileId, &'a ReExportEdge)>>;

fn re_exporters_by_source(graph: &ModuleGraph) -> ReExporters<'_> {
    let mut by_source: ReExporters<'_> = FxHashMap::default();
    for module in &graph.modules {
        for edge in &module.re_exports {
            by_source
                .entry(edge.source_file)
                .or_default()
                .push((module.file_id, edge));
        }
    }
    by_source
}

struct DetectorContext<'a> {
    file_paths: &'a FxHashMap<FileId, &'a std::path::Path>,
    re_exporters: &'a ReExporters<'a>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn deprecated_export_finding(
    graph: &ModuleGraph,
    module: &ModuleNode,
    export: &ExportSymbol,
    ctx: &DetectorContext<'_>,
    source_cache: &mut FxHashMap<FileId, (String, Vec<u32>)>,
) -> Option<DeprecatedExportInUse> {
    if !export.deprecated || (export.span.start == 0 && export.span.end == 0) {
        return None;
    }
    let (line, col) =
        byte_offset_to_line_col(ctx.line_offsets_by_file, module.file_id, export.span.start);
    if ctx
        .suppressions
        .is_suppressed(module.file_id, line, IssueKind::DeprecatedExportInUse)
    {
        return None;
    }

    let export_name = export.name.to_string();
    let mut consumers = reachable_consumers(graph, module, export, &export_name, ctx, source_cache);
    if consumers.is_empty() {
        return None;
    }
    let consumer_count = consumers.len();
    consumers.truncate(DEPRECATED_CONSUMER_SAMPLE_CAP);
    let public_api = module.is_entry_point()
        || re_export_chain_reaches_entry_point(graph, ctx.re_exporters, module, &export_name);

    Some(DeprecatedExportInUse {
        path: module.path.clone(),
        export_name,
        is_type_only: export.is_type_only,
        line,
        col,
        span_start: export.span.start,
        deprecated_reason: export.deprecated_reason.as_deref().map(str::to_owned),
        consumer_count,
        consumers,
        public_api,
    })
}

/// The distinct reference sites of `export` in reachable files, sorted by
/// path, line, column and kind. A reference from an unreachable file is not
/// a use: that file is dead code itself.
fn reachable_consumers(
    graph: &ModuleGraph,
    module: &ModuleNode,
    export: &ExportSymbol,
    export_name: &str,
    ctx: &DetectorContext<'_>,
    source_cache: &mut FxHashMap<FileId, (String, Vec<u32>)>,
) -> Vec<DeprecatedExportConsumer> {
    let mut consumers: Vec<DeprecatedExportConsumer> = export
        .physical_references()
        .filter(|reference| {
            graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(ModuleNode::is_reachable)
        })
        .filter_map(|reference| {
            let reference = with_re_export_span(reference, graph, module, export_name);
            let location = reference_location(
                &reference,
                ctx.file_paths,
                ctx.line_offsets_by_file,
                source_cache,
            )?;
            Some(DeprecatedExportConsumer {
                path: location.path,
                line: location.line,
                col: location.col,
                kind: consumer_kind(reference.kind),
            })
        })
        .collect();
    consumers.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.kind.cmp(&b.kind))
    });
    consumers.dedup();
    consumers
}

/// The name a re-export edge gives to `name` from its source module, or
/// `None` when the edge forwards another name.
fn forwarded_name(edge: &ReExportEdge, name: &str) -> Option<String> {
    if edge.imported_name == "*" && edge.exported_name == "*" && name == "default" {
        // `export * from` never re-exports the default export.
        return None;
    }
    if edge.imported_name == "*" {
        // `export *` keeps the name; `export * as ns` exposes it under `ns`.
        return Some(if edge.exported_name == "*" {
            name.to_string()
        } else {
            edge.exported_name.clone()
        });
    }
    (edge.imported_name == name).then(|| edge.exported_name.clone())
}

/// Whether a chain of re-exports carries `export_name` from `module` to an
/// entry point. Such an export is public API even when no internal file
/// imports it, so the finding must not advise removal.
fn re_export_chain_reaches_entry_point(
    graph: &ModuleGraph,
    re_exporters: &ReExporters<'_>,
    module: &ModuleNode,
    export_name: &str,
) -> bool {
    let mut queue: Vec<(FileId, String)> = vec![(module.file_id, export_name.to_string())];
    let mut seen: FxHashSet<(FileId, String)> = FxHashSet::default();
    while let Some((file_id, name)) = queue.pop() {
        if !seen.insert((file_id, name.clone())) {
            continue;
        }
        let Some(edges) = re_exporters.get(&file_id) else {
            continue;
        };
        for (barrel, edge) in edges {
            let Some(forwarded) = forwarded_name(edge, &name) else {
                continue;
            };
            if graph
                .modules
                .get(barrel.0 as usize)
                .is_some_and(ModuleNode::is_entry_point)
            {
                return true;
            }
            queue.push((*barrel, forwarded));
        }
    }
    false
}

/// A re-export reference that the graph synthesized carries no span. Point it
/// at the statement in the re-exporting file that exports the name, so the
/// consumer has a location: the edge from the declaring module first, then
/// any edge that exports the name, then a star re-export.
fn with_re_export_span(
    reference: &SymbolReference,
    graph: &ModuleGraph,
    module: &ModuleNode,
    export_name: &str,
) -> SymbolReference {
    let mut reference = *reference;
    if reference.kind != ReferenceKind::ReExport
        || !(reference.import_span.start == 0 && reference.import_span.end == 0)
    {
        return reference;
    }
    let Some(barrel) = graph.modules.get(reference.from_file.0 as usize) else {
        return reference;
    };
    let spanned = || {
        barrel
            .re_exports
            .iter()
            .filter(|edge| !(edge.span.start == 0 && edge.span.end == 0))
    };
    let span = spanned()
        .find(|edge| {
            edge.source_file == module.file_id && forwarded_name(edge, export_name).is_some()
        })
        .or_else(|| spanned().find(|edge| edge.exported_name == export_name))
        .or_else(|| spanned().find(|edge| edge.imported_name == "*" && edge.exported_name == "*"))
        .map(|edge| edge.span);
    if let Some(span) = span {
        reference.import_span = span;
    }
    reference
}

const fn consumer_kind(kind: ReferenceKind) -> DeprecatedConsumerKind {
    match kind {
        ReferenceKind::NamedImport => DeprecatedConsumerKind::NamedImport,
        ReferenceKind::DefaultImport => DeprecatedConsumerKind::DefaultImport,
        ReferenceKind::NamespaceImport => DeprecatedConsumerKind::NamespaceImport,
        ReferenceKind::ReExport => DeprecatedConsumerKind::ReExport,
        ReferenceKind::DynamicImport => DeprecatedConsumerKind::DynamicImport,
        ReferenceKind::SideEffectImport => DeprecatedConsumerKind::SideEffectImport,
    }
}
