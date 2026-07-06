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
//!   (`ModuleInfo.callee_uses`) whose leading identifier is bound to no import
//!   (unresolved callees: locals, globals, dynamic dispatch, re-bound callees).
//!
//! NOT resolved (reported as unresolved or absent, the same class of limits the
//! security taint walk carries): computed-member calls, dynamic dispatch,
//! re-bound callees, methods reached only through type inference, and any callee
//! with no import-symbol edge. The walk is MODULE-scoped, not per-function: a
//! true intra-function dataflow is beyond ADR-001 syntactic scope.

use std::path::{Path, PathBuf};

use fallow_types::extract::{ImportedName, ModuleInfo};
pub use fallow_types::trace_chain::{
    ChainHop, DEFAULT_TRACE_DEPTH, SymbolChainQuery, SymbolChainTrace, TraceDirections,
    UnresolvedCallee, UnresolvedReason,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::ModuleGraph;

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
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file))?;
    let rel_file = relativize(&module.path, root);

    let symbol_found = module.exports.iter().any(|e| e.name.to_string() == *symbol);

    let module_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let callers = directions
        .callers
        .then(|| collect_callers(graph, root, module.file_id, symbol, depth));
    let callees_walk = directions
        .callees
        .then(|| collect_callees(graph, &module_by_id, root, module.file_id, symbol, depth));
    let (callees, unresolved_callees) = match callees_walk {
        Some((resolved, unresolved)) => (Some(resolved), Some(unresolved)),
        None => (None, None),
    };

    let reason = build_reason(
        symbol_found,
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
        reason,
    })
}

/// Shared walk state, so the recursive helpers stay under the argument cap.
struct WalkCtx<'a> {
    graph: &'a ModuleGraph,
    root: &'a Path,
    max_depth: u32,
}

/// Walk UP: every module that imports `symbol` from `target`, recursed through
/// each importer's own binding up to `depth`.
fn collect_callers(
    graph: &ModuleGraph,
    root: &Path,
    target: FileId,
    symbol: &str,
    depth: u32,
) -> Vec<ChainHop> {
    let ctx = WalkCtx {
        graph,
        root,
        max_depth: depth,
    };
    let mut hops = Vec::new();
    let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
    walk_callers_recursive(&ctx, target, symbol, 1, &mut visited, &mut hops);
    hops.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.local_name.cmp(&b.local_name))
    });
    hops
}

fn walk_callers_recursive(
    ctx: &WalkCtx<'_>,
    target: FileId,
    symbol: &str,
    current_depth: u32,
    visited: &mut FxHashSet<(FileId, String)>,
    hops: &mut Vec<ChainHop>,
) {
    if current_depth > ctx.max_depth {
        return;
    }
    for &importer in ctx.graph.importers_of(target) {
        for (edge_target, symbols) in ctx.graph.outgoing_symbol_edges(importer) {
            if edge_target != target {
                continue;
            }
            for sym in symbols {
                if !imported_name_matches(&sym.imported_name, symbol) {
                    continue;
                }
                let key = (importer, sym.local_name.clone());
                if !visited.insert(key) {
                    continue;
                }
                let importer_path = ctx.graph.modules.get(importer.0 as usize).map_or_else(
                    || PathBuf::from("<unknown>"),
                    |m| relativize(&m.path, ctx.root),
                );
                hops.push(ChainHop {
                    file: importer_path,
                    imported_as: imported_name_label(&sym.imported_name),
                    local_name: sym.local_name.clone(),
                    type_only: sym.is_type_only,
                    depth: current_depth,
                });
                // Recurse: the importer's local binding becomes the next symbol
                // whose callers we walk (covers a re-exported / forwarded symbol).
                walk_callers_recursive(
                    ctx,
                    importer,
                    &sym.local_name,
                    current_depth + 1,
                    visited,
                    hops,
                );
            }
        }
    }
}

/// Walk DOWN: the import-symbol edges OUT of `module_id` (resolved callees),
/// recursed into each target module's exports up to `depth`, plus the call sites
/// in `module_id` whose callee did not resolve to an import binding (unresolved).
fn collect_callees(
    graph: &ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ModuleInfo>,
    root: &Path,
    module_id: FileId,
    _symbol: &str,
    depth: u32,
) -> (Vec<ChainHop>, Vec<UnresolvedCallee>) {
    let ctx = WalkCtx {
        graph,
        root,
        max_depth: depth,
    };
    let mut resolved = Vec::new();
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    visited.insert(module_id);
    walk_callees_recursive(&ctx, module_id, 1, &mut visited, &mut resolved);
    resolved.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.local_name.cmp(&b.local_name))
    });

    let unresolved = module_by_id
        .get(&module_id)
        .map(|info| collect_unresolved_callees(info))
        .unwrap_or_default();

    (resolved, unresolved)
}

fn walk_callees_recursive(
    ctx: &WalkCtx<'_>,
    module_id: FileId,
    current_depth: u32,
    visited: &mut FxHashSet<FileId>,
    hops: &mut Vec<ChainHop>,
) {
    if current_depth > ctx.max_depth {
        return;
    }
    for (edge_target, symbols) in ctx.graph.outgoing_symbol_edges(module_id) {
        let target_path = ctx.graph.modules.get(edge_target.0 as usize).map_or_else(
            || PathBuf::from("<unknown>"),
            |m| relativize(&m.path, ctx.root),
        );
        for sym in symbols {
            // Side-effect imports carry no value-level callee; skip them.
            if matches!(sym.imported_name, ImportedName::SideEffect) {
                continue;
            }
            hops.push(ChainHop {
                file: target_path.clone(),
                imported_as: imported_name_label(&sym.imported_name),
                local_name: sym.local_name.clone(),
                type_only: sym.is_type_only,
                depth: current_depth,
            });
        }
        if visited.insert(edge_target) {
            walk_callees_recursive(ctx, edge_target, current_depth + 1, visited, hops);
        }
    }
}

/// Build the unresolved-callee list from a module's call sites: every
/// `callee_uses` path whose leading identifier is bound to no import is
/// surfaced (best-effort classification). A dotted path is `MemberOrDynamic`; a
/// bare identifier with no import binding is `LocalOrGlobal`.
fn collect_unresolved_callees(info: &ModuleInfo) -> Vec<UnresolvedCallee> {
    let import_locals: FxHashSet<&str> =
        info.imports.iter().map(|i| i.local_name.as_str()).collect();

    let mut out = Vec::new();
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    for callee in &info.callee_uses {
        let path = callee.callee_path.as_str();
        let leading = path.split('.').next().unwrap_or(path);
        // A callee whose leading identifier is an imported binding resolves to a
        // graph edge already (covered by the resolved callee list); skip it.
        if import_locals.contains(leading) {
            continue;
        }
        if !seen.insert(path) {
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
    callers: Option<&[ChainHop]>,
    callees: Option<&[ChainHop]>,
    unresolved: Option<&[UnresolvedCallee]>,
) -> String {
    if !symbol_found {
        return "symbol not found as an export of this file; chains are file-scoped best-effort and may be empty".to_string();
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
    format!(
        "best-effort syntactic chain (ADR-001): {}",
        parts.join(", ")
    )
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

fn imported_name_label(name: &ImportedName) -> String {
    match name {
        ImportedName::Named(name) => name.clone(),
        ImportedName::Default => "default".to_string(),
        ImportedName::Namespace => "*".to_string(),
        ImportedName::SideEffect => "side-effect".to_string(),
    }
}

fn relativize(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Match a user-provided file path against a module's actual path. Mirrors
/// `trace::path_matches` (kept local to avoid widening that private helper).
fn path_matches(module_path: &Path, root: &Path, user_path: &str) -> bool {
    let user_path_norm = user_path.replace('\\', "/");
    let rel = module_path.strip_prefix(root).unwrap_or(module_path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let module_str = module_path.to_string_lossy().replace('\\', "/");
    if rel_str == user_path_norm || module_str == user_path_norm {
        return true;
    }
    if dunce::canonicalize(root).is_ok_and(|canonical_root| {
        module_path
            .strip_prefix(&canonical_root)
            .is_ok_and(|rel| rel.to_string_lossy().replace('\\', "/") == user_path_norm)
    }) {
        return true;
    }
    module_str.ends_with(&format!("/{user_path_norm}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::test_support::empty_module;

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
                    span_start: 0,
                },
                fallow_types::extract::CalleeUse {
                    callee_path: "obj.method".to_string(),
                    span_start: 10,
                },
            ],
            ..empty_module()
        };
        let unresolved = collect_unresolved_callees(&info);
        assert_eq!(unresolved.len(), 2);
        assert_eq!(unresolved[0].callee, "localHelper");
        assert_eq!(unresolved[0].reason, UnresolvedReason::LocalOrGlobal);
        assert_eq!(unresolved[1].callee, "obj.method");
        assert_eq!(unresolved[1].reason, UnresolvedReason::MemberOrDynamic);
    }

    #[test]
    fn imported_callees_are_not_listed_as_unresolved() {
        let info = ModuleInfo {
            imports: vec![fallow_types::extract::ImportInfo {
                source: "./dep".to_string(),
                imported_name: ImportedName::Named("dep".to_string()),
                local_name: "dep".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            }],
            callee_uses: vec![
                fallow_types::extract::CalleeUse {
                    callee_path: "dep".to_string(),
                    span_start: 0,
                },
                fallow_types::extract::CalleeUse {
                    callee_path: "ghost".to_string(),
                    span_start: 5,
                },
            ],
            ..empty_module()
        };
        let unresolved = collect_unresolved_callees(&info);
        // `dep` resolves to an import (covered by resolved callees); only `ghost`
        // is unresolved.
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].callee, "ghost");
    }
}
