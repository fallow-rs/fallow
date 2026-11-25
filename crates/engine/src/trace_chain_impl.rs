//! Symbol-level call chains (`fallow trace <symbol> --callers --callees`).
//!
//! Best-effort, syntactic (ADR-001), EXPLICITLY OFF the ranked path. This walk
//! NEVER feeds the focus map / ranking (verified by a dedicated test in the
//! CLI crate). It reports resolved-vs-unresolved callees HONESTLY: a referenced
//! callee that the syntactic walk cannot resolve to an import-symbol edge is
//! surfaced in [`SymbolChainTrace::unresolved_callees`], never silently dropped.
//!
//! ## What "resolved" means (honest scoping)
//!
//! The graph models import-symbol edges: module A imports binding `foo` from
//! module B. From those edges this walk reconstructs:
//!
//! - **Callers (UP):** for a symbol `S` defined in module `M`, every module that
//!   imports `S` from `M` (via [`ModuleGraph::importers_of`] + the per-edge
//!   `ImportedSymbol` set), recursed through the importer's own binding up to
//!   `depth`.
//! - **Callees (DOWN):** the import-symbol edges OUT of `M` (resolved callees,
//!   each a `(local, imported, target_module)` triple), recursed into the target
//!   module's exports up to `depth`, PLUS the call sites in `M`
//!   (`ModuleInfo.callee_uses`) whose leading identifier has no import-symbol edge.
//!
//! NOT resolved (reported as unresolved or absent, the same class of limits the
//! security taint walk carries): computed-member calls, dynamic dispatch,
//! re-bound callees, methods reached only through type inference, and any callee
//! with no import-symbol edge. The walk is MODULE-scoped, not per-function: a
//! true intra-function dataflow is beyond ADR-001 syntactic scope.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use fallow_types::extract::{ImportedName, ModuleInfo};
use fallow_types::semantic::SemanticNamespace;
use fallow_types::trace_chain::{
    ChainHop, StarExportAmbiguity, SymbolChainQuery, SymbolChainTrace, UnresolvedCallee,
    UnresolvedReason,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ImportedSymbol, ModuleGraph};
use crate::trace::trace_impl::find_module;

/// Trace the symbol-level call chain for `query.symbol` in `query.file`.
///
/// `modules` is the parsed `ModuleInfo` set (retained from the pipeline) used to
/// surface unresolved callees; pass an empty slice to skip unresolved-callee
/// reporting (callers / resolved callees still work from the graph alone).
#[must_use]
pub fn trace_symbol_chain(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    root: &Path,
    query: SymbolChainQuery<'_>,
) -> Option<SymbolChainTrace> {
    let SymbolChainQuery {
        file,
        symbol,
        depth,
        directions,
    } = query;
    let module = find_module(graph, root, file)?;
    let rel_file = relativize(&module.path, root);

    let symbol_found = module.exports.iter().any(|e| e.name.to_string() == *symbol);
    // A barrel whose star sources collide on this name exports nothing under
    // it, so it looks exactly like an unknown name unless the collision is
    // reported alongside.
    let star_export_ambiguity = (!symbol_found)
        .then(|| star_export_ambiguity(graph, root, module.file_id, symbol))
        .flatten();

    let module_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    // A name that the file does not export has no chains. A namespace
    // importer would otherwise match any name, and the callee walk is
    // file-scoped, so a typo would get plausible hops.
    let callers = directions.callers.then(|| {
        if symbol_found {
            collect_callers(graph, root, module.file_id, symbol, depth)
        } else {
            Vec::new()
        }
    });
    let callees_walk = directions.callees.then(|| {
        if symbol_found {
            collect_callees(graph, &module_by_id, root, module.file_id, depth)
        } else {
            (Vec::new(), Vec::new())
        }
    });
    let (callees, unresolved_callees) = match callees_walk {
        Some((resolved, unresolved)) => (Some(resolved), Some(unresolved)),
        None => (None, None),
    };

    let reason = build_reason(
        symbol_found,
        star_export_ambiguity.as_ref(),
        callers.as_deref(),
        callees.as_deref(),
        unresolved_callees.as_deref(),
    );

    Some(SymbolChainTrace {
        file: rel_file,
        symbol: symbol.to_string(),
        symbol_found,
        depth,
        best_effort: true,
        callers,
        callees,
        unresolved_callees,
        star_export_ambiguity,
        reason,
    })
}

/// The `export *` collision on `module_id` that swallows `symbol`, if any.
fn star_export_ambiguity(
    graph: &ModuleGraph,
    root: &Path,
    module_id: FileId,
    symbol: &str,
) -> Option<StarExportAmbiguity> {
    let collisions: Vec<_> = graph
        .ambiguous_star_exports_on(module_id)
        .into_iter()
        .filter(|collision| collision.name.as_ref() == symbol)
        .collect();
    if collisions.is_empty() {
        return None;
    }

    let mut sources: Vec<PathBuf> = collisions
        .iter()
        .flat_map(|collision| collision.contributors.iter())
        .filter_map(|contributor| graph.module(*contributor))
        .map(|module| relativize(&module.path, root))
        .collect();
    sources.sort();
    sources.dedup();

    let mut namespaces: Vec<SemanticNamespace> = collisions
        .iter()
        .map(|collision| match collision.namespace {
            ExportNamespace::Type => SemanticNamespace::Type,
            ExportNamespace::Value => SemanticNamespace::Value,
        })
        .collect();
    namespaces.sort_unstable_by_key(|namespace| namespace_order(*namespace));
    namespaces.dedup();

    Some(StarExportAmbiguity {
        sources,
        namespaces,
    })
}

/// Report type space before value space, matching the collision order the
/// graph reports so both surfaces read the same way.
const fn namespace_order(namespace: SemanticNamespace) -> u8 {
    match namespace {
        SemanticNamespace::Type => 0,
        SemanticNamespace::Value => 1,
    }
}

/// Walk UP: every module that imports `symbol` from `target`, followed through
/// each importer's own binding up to `depth`.
///
/// The walk is breadth-first, so each binding gets its shortest depth and a
/// deeper path cannot hide a shorter one from the depth limit.
fn collect_callers(
    graph: &ModuleGraph,
    root: &Path,
    target: FileId,
    symbol: &str,
    depth: u32,
) -> Vec<ChainHop> {
    let mut hops = Vec::new();
    let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut queue: VecDeque<(FileId, String, u32)> = VecDeque::new();
    queue.push_back((target, symbol.to_owned(), 1));
    while let Some((target, symbol, current_depth)) = queue.pop_front() {
        if current_depth > depth {
            continue;
        }
        for (importer, sym) in importing_symbols(graph, target, &symbol) {
            if !visited.insert((importer, sym.local_name.clone())) {
                continue;
            }
            hops.push(ChainHop {
                file: module_path(graph, importer, root),
                imported_as: imported_name_label(&sym.imported_name).to_owned(),
                local_name: sym.local_name.clone(),
                type_only: sym.is_type_only,
                depth: current_depth,
            });
            // The importer's local binding is the next symbol whose callers
            // the walk follows (a re-exported symbol).
            queue.push_back((importer, sym.local_name.clone(), current_depth + 1));
        }
    }
    sort_hops(&mut hops);
    hops
}

/// Walk DOWN: the import-symbol edges OUT of `module_id` (resolved callees),
/// followed into each target module up to `depth`, plus the call sites in
/// `module_id` whose callee has no matching import-symbol edge.
///
/// The walk is breadth-first, so each module is expanded at its shortest
/// depth.
fn collect_callees(
    graph: &ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ModuleInfo>,
    root: &Path,
    module_id: FileId,
    depth: u32,
) -> (Vec<ChainHop>, Vec<UnresolvedCallee>) {
    let mut resolved = Vec::new();
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    visited.insert(module_id);
    let mut queue: VecDeque<(FileId, u32)> = VecDeque::new();
    queue.push_back((module_id, 1));
    while let Some((module_id, current_depth)) = queue.pop_front() {
        if current_depth > depth {
            continue;
        }
        let Some(module) = graph.module(module_id) else {
            continue;
        };
        // The import edge for a re-export has no binding. Its names live on
        // the re-export record, which a bare side-effect import does not have.
        let forwarded = module.re_exports.iter().map(|edge| {
            (
                edge.source_file,
                edge.imported_name.as_str(),
                edge.exported_name.as_str(),
                edge.is_type_only,
            )
        });
        // Importing and re-exporting the same binding describes one hop.
        let forwarded_bindings: FxHashSet<_> = forwarded.clone().collect();
        let imports = direct_callee_bindings(graph, module_id)
            .filter(|binding| !forwarded_bindings.contains(binding));
        for (target, imported_as, local_name, type_only) in imports.chain(forwarded) {
            resolved.push(ChainHop {
                file: module_path(graph, target, root),
                imported_as: imported_as.to_owned(),
                local_name: local_name.to_owned(),
                type_only,
                depth: current_depth,
            });
            if visited.insert(target) {
                queue.push_back((target, current_depth + 1));
            }
        }
    }
    sort_hops(&mut resolved);

    let unresolved = module_by_id
        .get(&module_id)
        .map(|info| {
            let resolved_locals = direct_callee_bindings(graph, module_id)
                .map(|(_, _, local_name, _)| local_name)
                .collect();
            collect_unresolved_callees(info, &resolved_locals)
        })
        .unwrap_or_default();

    (resolved, unresolved)
}

/// Direct import bindings with an internal target. Re-exports do not introduce
/// local bindings that can resolve a call in this module.
fn direct_callee_bindings(
    graph: &ModuleGraph,
    module_id: FileId,
) -> impl Iterator<Item = (FileId, &str, &str, bool)> {
    graph
        .outgoing_symbol_edges(module_id)
        .flat_map(|(target, symbols)| {
            symbols
                .iter()
                .filter(|symbol| !matches!(symbol.imported_name, ImportedName::SideEffect))
                .map(move |symbol| {
                    (
                        target,
                        imported_name_label(&symbol.imported_name),
                        symbol.local_name.as_str(),
                        symbol.is_type_only,
                    )
                })
        })
}

/// Each import of `symbol` from `target`, with the module that imports it.
fn importing_symbols<'g>(
    graph: &'g ModuleGraph,
    target: FileId,
    symbol: &'g str,
) -> impl Iterator<Item = (FileId, &'g ImportedSymbol)> + 'g {
    graph
        .importers_of(target)
        .iter()
        .flat_map(move |&importer| {
            graph
                .outgoing_symbol_edges(importer)
                .filter(move |(edge_target, _)| *edge_target == target)
                .flat_map(|(_, symbols)| symbols.iter())
                .filter(move |sym| imported_name_matches(&sym.imported_name, symbol))
                .map(move |sym| (importer, sym))
        })
}

fn module_path(graph: &ModuleGraph, file_id: FileId, root: &Path) -> PathBuf {
    graph
        .modules()
        .get(file_id.0 as usize)
        .map_or_else(|| PathBuf::from("<unknown>"), |m| relativize(&m.path, root))
}

fn sort_hops(hops: &mut [ChainHop]) {
    hops.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.local_name.cmp(&b.local_name))
    });
}

/// Report calls whose leading identifier has no resolved edge in this module.
fn collect_unresolved_callees(
    info: &ModuleInfo,
    resolved_locals: &FxHashSet<&str>,
) -> Vec<UnresolvedCallee> {
    let mut out = Vec::new();
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    for callee in info.callee_uses.iter().filter(|callee| !callee.is_new) {
        let path = callee.callee_path.as_str();
        let leading = path.split('.').next().unwrap_or(path);
        if resolved_locals.contains(leading) || !seen.insert(path) {
            continue;
        }
        let reason = if path.contains('.') {
            UnresolvedReason::MemberOrDynamic
        } else {
            UnresolvedReason::LocalOrGlobal
        };
        out.push(UnresolvedCallee {
            callee: path.to_string(),
            reason,
        });
    }
    out.sort_by(|a, b| a.callee.cmp(&b.callee));
    out
}

fn build_reason(
    symbol_found: bool,
    ambiguity: Option<&StarExportAmbiguity>,
    callers: Option<&[ChainHop]>,
    callees: Option<&[ChainHop]>,
    unresolved: Option<&[UnresolvedCallee]>,
) -> String {
    if let Some(ambiguity) = ambiguity {
        let sources = ambiguity
            .sources
            .iter()
            .map(|source| source.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>()
            .join(", ");
        return format!(
            "two or more `export *` sources of this file declare this name, so it resolves as ambiguous and the file exports nothing under it (ECMA-262 ResolveExport); colliding origins: {sources}"
        );
    }
    if !symbol_found {
        return "symbol not found as an export of this file, so the chains are empty".to_string();
    }
    let mut parts = Vec::new();
    if let Some(callers) = callers {
        parts.push(format!("{} caller hop(s)", callers.len()));
    }
    if let Some(callees) = callees {
        parts.push(format!("{} resolved callee hop(s)", callees.len()));
    }
    if let Some(unresolved) = unresolved {
        parts.push(format!(
            "{} unresolved callee(s) reported",
            unresolved.len()
        ));
    }
    format!("best-effort syntactic chain: {}", parts.join(", "))
}

/// True when an [`ImportedName`] refers to `symbol`. A namespace import (`*`)
/// brings every export of the target into scope, so it matches any symbol.
fn imported_name_matches(name: &ImportedName, symbol: &str) -> bool {
    match name {
        ImportedName::Named(n) => n == symbol,
        ImportedName::Default => symbol == "default",
        ImportedName::Namespace => true,
        ImportedName::SideEffect => false,
    }
}

fn imported_name_label(name: &ImportedName) -> &str {
    match name {
        ImportedName::Named(name) => name,
        ImportedName::Default => "default",
        ImportedName::Namespace => "*",
        ImportedName::SideEffect => "side-effect",
    }
}

fn relativize(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_name_matches_named_default_namespace() {
        assert!(imported_name_matches(
            &ImportedName::Named("foo".to_string()),
            "foo"
        ));
        assert!(!imported_name_matches(
            &ImportedName::Named("bar".to_string()),
            "foo"
        ));
        assert!(imported_name_matches(&ImportedName::Default, "default"));
        assert!(!imported_name_matches(&ImportedName::Default, "foo"));
        // A namespace import brings every export into scope.
        assert!(imported_name_matches(&ImportedName::Namespace, "anything"));
        assert!(!imported_name_matches(&ImportedName::SideEffect, "foo"));
    }

    #[test]
    fn unresolved_reason_classifies_member_vs_bare() {
        let info = ModuleInfo {
            callee_uses: vec![
                fallow_types::extract::CalleeUse {
                    callee_path: "localHelper".to_string(),
                    span_starts: vec![0],
                    is_new: false,
                },
                fallow_types::extract::CalleeUse {
                    callee_path: "obj.method".to_string(),
                    span_starts: vec![10],
                    is_new: false,
                },
            ],
            ..ModuleInfo::empty(FileId(1))
        };
        let unresolved = collect_unresolved_callees(&info, &FxHashSet::default());
        assert_eq!(unresolved.len(), 2);
        assert_eq!(unresolved[0].callee, "localHelper");
        assert_eq!(unresolved[0].reason, UnresolvedReason::LocalOrGlobal);
        assert_eq!(unresolved[1].callee, "obj.method");
        assert_eq!(unresolved[1].reason, UnresolvedReason::MemberOrDynamic);
    }

    #[test]
    fn new_expressions_are_not_listed_as_unresolved() {
        let info = ModuleInfo {
            callee_uses: vec![
                fallow_types::extract::CalleeUse {
                    callee_path: "Foo".to_string(),
                    span_starts: vec![0],
                    is_new: true,
                },
                fallow_types::extract::CalleeUse {
                    callee_path: "localHelper".to_string(),
                    span_starts: vec![10],
                    is_new: false,
                },
            ],
            ..ModuleInfo::empty(FileId(1))
        };
        let unresolved = collect_unresolved_callees(&info, &FxHashSet::default());
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].callee, "localHelper");
    }

    #[test]
    fn resolved_callees_are_not_listed_as_unresolved() {
        let info = ModuleInfo {
            callee_uses: vec![
                fallow_types::extract::CalleeUse {
                    callee_path: "dep".to_string(),
                    span_starts: vec![0],
                    is_new: false,
                },
                fallow_types::extract::CalleeUse {
                    callee_path: "ghost".to_string(),
                    span_starts: vec![5],
                    is_new: false,
                },
            ],
            ..ModuleInfo::empty(FileId(1))
        };
        let resolved_locals = std::iter::once("dep").collect();
        let unresolved = collect_unresolved_callees(&info, &resolved_locals);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].callee, "ghost");
    }
}
