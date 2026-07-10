//! Cross-package propagation for namespace-import object aliases.
//!
//! When a barrel re-exports a namespace import inside an object literal
//! (`import * as foo from './bar'; export const API = { foo }`), a downstream
//! consumer accessing `API.foo.bar` would lose the connection between `bar`
//! and the namespace target file because `narrow_namespace_references` only
//! scans member accesses in the file that contains the `import *`. This
//! module propagates each consumer's `<imported>.<suffix>.<member>` access
//! onto the namespace target's matching export so cross-package access does
//! not surface as a false `unused-export`. See issue #303.
//!
//! Chained namespace re-exports on the alias target side are also followed:
//! when the alias target does `export * as N from './S'` and the consumer
//! accesses `API.foo.N.X`, the access `X` is credited on `./S` (and so on
//! recursively). See issue #328.
//!
//! Runs once after Phase 2 (reference population) and before Phase 3
//! (reachability) so any reference attached here participates in reachability
//! and re-export chain propagation downstream.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, NamespaceObjectAlias};

use crate::resolve::ResolvedModule;

use super::ModuleGraph;
use super::namespace_indexes::NamespacePropagationIndexes;
use super::narrowing::{
    create_synthetic_exports_for_star_re_exports, mark_member_exports_referenced,
};
use super::types::ReferenceKind;

/// One credit operation collected during the scan and applied after the loop
/// to keep mutable borrows of `ModuleGraph::modules` localised.
struct PendingCredit {
    /// Index into `ModuleGraph::modules` of the namespace target file.
    target_module_idx: usize,
    /// Member name to credit on the target's exports.
    member: String,
    /// Consumer file that produced the access.
    consumer_file_id: FileId,
    /// Span of the consumer's import that brought the aliased export into scope.
    import_span: oxc_span::Span,
}

/// Propagate cross-package consumer accesses through `NamespaceObjectAlias`
/// entries on each `ResolvedModule`. Mutates `graph.modules[*].exports` to
/// attach a `SymbolReference` for each accessed member on the namespace's
/// source file.
pub(super) fn propagate_cross_package_aliases(
    graph: &mut ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    indexes: &NamespacePropagationIndexes<'_>,
) {
    let pending = collect_pending_credits(graph, module_by_id, indexes);
    apply_pending_credits(graph, &pending);
}

fn collect_pending_credits(
    graph: &ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    indexes: &NamespacePropagationIndexes<'_>,
) -> Vec<PendingCredit> {
    let mut pending = Vec::new();

    for alias_module in module_by_id.values() {
        if alias_module.namespace_object_aliases.is_empty() {
            continue;
        }
        let alias_file_id = alias_module.file_id;
        for alias in &alias_module.namespace_object_aliases {
            let Some(namespace_target_id) = resolve_namespace_target(alias_module, alias) else {
                continue;
            };
            let Some(target_module_idx) = module_index_for_file(graph, namespace_target_id) else {
                continue;
            };
            let reachable =
                indexes.enumerate_reachable_barrels(alias_file_id, &alias.via_export_name);
            collect_credits_for_alias(NamespaceCreditInput {
                graph,
                indexes,
                alias_file_id,
                alias,
                target_module_idx,
                reachable: &reachable,
                pending: &mut pending,
            });
        }
    }

    pending
}

/// Resolve the file_id of a namespace import on `alias_module` whose local
/// name matches `alias.namespace_local`. Only `InternalModule` targets count;
/// external packages cannot have references propagated.
fn resolve_namespace_target(
    alias_module: &ResolvedModule,
    alias: &NamespaceObjectAlias,
) -> Option<FileId> {
    alias_module.resolved_imports.iter().find_map(|import| {
        if import.info.local_name != alias.namespace_local {
            return None;
        }
        if !matches!(import.info.imported_name, ImportedName::Namespace) {
            return None;
        }
        import.target.internal_file_id()
    })
}

fn module_index_for_file(graph: &ModuleGraph, file_id: FileId) -> Option<usize> {
    let idx = file_id.0 as usize;
    if idx >= graph.modules.len() {
        return None;
    }
    Some(idx)
}

struct NamespaceCreditInput<'a> {
    graph: &'a ModuleGraph,
    indexes: &'a NamespacePropagationIndexes<'a>,
    alias_file_id: FileId,
    alias: &'a NamespaceObjectAlias,
    target_module_idx: usize,
    reachable: &'a FxHashSet<(FileId, String)>,
    pending: &'a mut Vec<PendingCredit>,
}

struct ConsumerCreditInput<'a> {
    graph: &'a ModuleGraph,
    consumer: &'a ResolvedModule,
    import: &'a crate::resolve::ResolvedImport,
    prefix_match: &'a str,
    target_module_idx: usize,
    pending: &'a mut Vec<PendingCredit>,
}

fn collect_credits_for_alias(input: NamespaceCreditInput<'_>) {
    let NamespaceCreditInput {
        graph,
        indexes,
        alias_file_id,
        alias,
        target_module_idx,
        reachable,
        pending,
    } = input;
    let prefix_match = format!(".{}", alias.suffix);
    for (target, imported_name) in reachable {
        for indexed in indexes.consumers_for(*target, imported_name) {
            let consumer = indexed.consumer;
            let import = indexed.import;
            if consumer.file_id == alias_file_id {
                continue;
            }
            collect_credits_for_consumer_import(&mut ConsumerCreditInput {
                graph,
                consumer,
                import,
                prefix_match: &prefix_match,
                target_module_idx,
                pending,
            });
        }
    }
}

/// Collect credits for one consumer import that resolves to a reachable alias
/// barrel: match `<local>.<suffix>` member accesses and walk chained
/// `export * as N` re-exports on the alias target side.
fn collect_credits_for_consumer_import(input: &mut ConsumerCreditInput<'_>) {
    let graph = input.graph;
    let consumer = input.consumer;
    let import = input.import;
    let prefix_match = input.prefix_match;
    let target_module_idx = input.target_module_idx;
    let pending = &mut *input.pending;

    let consumer_local = import.info.local_name.as_str();
    if consumer_local.is_empty() {
        return;
    }
    let expected_object = format!("{consumer_local}{prefix_match}");
    for access in &consumer.member_accesses {
        if access.object != expected_object {
            continue;
        }
        pending.push(PendingCredit {
            target_module_idx,
            member: access.member.clone(),
            consumer_file_id: consumer.file_id,
            import_span: import.info.span,
        });
        let mut visited: FxHashSet<usize> = FxHashSet::default();
        visited.insert(target_module_idx);
        let ctx = ChainWalkCtx {
            graph,
            consumer,
            import_span: import.info.span,
        };
        collect_chained_re_export_credits(
            &ctx,
            target_module_idx,
            &access.member,
            &format!("{expected_object}.{}", access.member),
            &mut visited,
            pending,
        );
    }
}

/// Invariant context passed through the chain walker: the read-only graph,
/// the consumer module producing the accesses, and the original import span
/// to use as the `from` site on every resulting `SymbolReference`. Grouped
/// into a struct so the recursive helper stays under the workspace's 7-arg
/// clippy limit.
struct ChainWalkCtx<'a> {
    graph: &'a ModuleGraph,
    consumer: &'a ResolvedModule,
    import_span: oxc_span::Span,
}

/// Follow `export * as <name> from './source'` chains on the alias target
/// side. When `barrel_module_idx.re_exports` contains an edge with
/// `imported_name == "*" && exported_name == credited_name`, every consumer
/// access of the form `<accessor_prefix>.<X>` becomes a credit for `<X>` on
/// the re-export's `source_file`. Recurses if the new credit also lands on
/// another namespace re-export, bounded by `visited` to short-circuit cycles.
fn collect_chained_re_export_credits(
    ctx: &ChainWalkCtx<'_>,
    barrel_module_idx: usize,
    credited_name: &str,
    accessor_prefix: &str,
    visited: &mut FxHashSet<usize>,
    pending: &mut Vec<PendingCredit>,
) {
    let Some(barrel) = ctx.graph.modules.get(barrel_module_idx) else {
        return;
    };
    let chained_targets: Vec<FileId> = barrel
        .re_exports
        .iter()
        .filter(|edge| edge.imported_name == "*" && edge.exported_name == credited_name)
        .map(|edge| edge.source_file)
        .collect();
    for source_file in chained_targets {
        let Some(source_module_idx) = module_index_for_file(ctx.graph, source_file) else {
            continue;
        };
        if !visited.insert(source_module_idx) {
            continue;
        }
        for access in &ctx.consumer.member_accesses {
            if access.object != accessor_prefix {
                continue;
            }
            pending.push(PendingCredit {
                target_module_idx: source_module_idx,
                member: access.member.clone(),
                consumer_file_id: ctx.consumer.file_id,
                import_span: ctx.import_span,
            });
            collect_chained_re_export_credits(
                ctx,
                source_module_idx,
                &access.member,
                &format!("{accessor_prefix}.{}", access.member),
                visited,
                pending,
            );
        }
    }
}

/// Apply collected credits, grouping by `(target_module_idx, consumer, import_span)`
/// so each (consumer file, namespace target) pair runs through the same
/// `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports`
/// pipeline that `narrow_namespace_references` uses for direct namespace
/// imports. The synthetic-export step is what handles the case where the
/// namespace target is a star barrel (`export * from './bar'`): missing
/// member exports are stubbed so Phase 4 chain resolution can propagate the
/// reference to the real defining file.
fn apply_pending_credits(graph: &mut ModuleGraph, pending: &[PendingCredit]) {
    type GroupKey = (usize, FileId, oxc_span::Span);

    let mut groups: FxHashMap<GroupKey, Vec<String>> = FxHashMap::default();
    for credit in pending {
        groups
            .entry((
                credit.target_module_idx,
                credit.consumer_file_id,
                credit.import_span,
            ))
            .or_default()
            .push(credit.member.clone());
    }

    for ((target_module_idx, consumer_file_id, import_span), members) in groups {
        let module = &mut graph.modules[target_module_idx];
        let found_members = mark_member_exports_referenced(
            &mut module.exports,
            consumer_file_id,
            &members,
            import_span,
            ReferenceKind::NamespaceImport,
        );
        create_synthetic_exports_for_star_re_exports(
            &mut module.exports,
            &module.re_exports,
            consumer_file_id,
            &members,
            &found_members,
            import_span,
        );
    }
}
