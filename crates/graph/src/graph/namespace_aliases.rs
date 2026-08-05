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

use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, ModuleLoadMechanism, NamespaceObjectAlias};

use crate::resolve::ResolvedModule;

use super::ModuleGraph;
use super::namespace_indexes::{
    NamespacePropagationIndexes, NamespaceReferenceRoutes, ReachableNamespaceExports,
};
use super::narrowing::{
    ReferenceDedup, ReferenceSite, create_synthetic_exports_for_star_re_exports_at_site,
    mark_member_exports_referenced_at_site,
};
use super::types::{
    ReferenceKind, ReferencePathId, ReferencePathInterner, ReferenceRouteGraphSpec,
    ReferenceRouteNodeId, ReferenceRouteNodeSpec,
};

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
    /// Exact consumer-to-target path, including every alias/re-export hop.
    path: Option<ReferencePathId>,
}

/// Propagate cross-package consumer accesses through `NamespaceObjectAlias`
/// entries on each `ResolvedModule`. Mutates `graph.modules[*].exports` to
/// attach a `SymbolReference` for each accessed member on the namespace's
/// source file.
pub(super) fn propagate_cross_package_aliases(
    graph: &mut ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    indexes: &NamespacePropagationIndexes<'_>,
    reference_paths: &mut ReferencePathInterner,
) {
    let pending = collect_pending_credits(graph, module_by_id, indexes, reference_paths);
    apply_pending_credits(graph, pending);
}

fn collect_pending_credits(
    graph: &ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    indexes: &NamespacePropagationIndexes<'_>,
    reference_paths: &mut ReferencePathInterner,
) -> Vec<PendingCredit> {
    let mut pending = Vec::new();

    let mut alias_modules: Vec<_> = module_by_id.values().copied().collect();
    alias_modules.sort_unstable_by_key(|module| module.file_id.0);
    for alias_module in alias_modules {
        if alias_module.namespace_object_aliases.is_empty() {
            continue;
        }
        let alias_file_id = alias_module.file_id;
        for alias in &alias_module.namespace_object_aliases {
            let Some(namespace_target) = resolve_namespace_target(alias_module, alias) else {
                continue;
            };
            let Some(target_module_idx) = module_index_for_file(graph, namespace_target.file_id)
            else {
                continue;
            };
            let reachable =
                indexes.enumerate_reachable_barrels(alias_file_id, &alias.via_export_name);
            let routes = reachable.intern_routes(
                namespace_target.file_id,
                namespace_target.mechanism,
                reference_paths,
            );
            collect_credits_for_alias(NamespaceCreditInput {
                graph,
                indexes,
                alias_file_id,
                alias,
                target_module_idx,
                reachable: &reachable,
                routes: &routes,
                pending: &mut pending,
                reference_paths,
            });
        }
    }

    pending
}

/// Resolve the file_id of a namespace import on `alias_module` whose local
/// name matches `alias.namespace_local`. Only `InternalModule` targets count;
/// external packages cannot have references propagated.
#[derive(Clone, Copy)]
struct NamespaceTarget {
    file_id: FileId,
    mechanism: ModuleLoadMechanism,
}

fn resolve_namespace_target(
    alias_module: &ResolvedModule,
    alias: &NamespaceObjectAlias,
) -> Option<NamespaceTarget> {
    alias_module.resolved_imports.iter().find_map(|import| {
        if import.info.local_name != alias.namespace_local {
            return None;
        }
        if !matches!(import.info.imported_name, ImportedName::Namespace) {
            return None;
        }
        Some(NamespaceTarget {
            file_id: import.target.internal_file_id()?,
            mechanism: if import.target.is_commonjs_require() {
                ModuleLoadMechanism::CommonJsRequire
            } else {
                ModuleLoadMechanism::EsModule
            },
        })
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
    reachable: &'a ReachableNamespaceExports,
    routes: &'a NamespaceReferenceRoutes,
    pending: &'a mut Vec<PendingCredit>,
    reference_paths: &'a mut ReferencePathInterner,
}

struct ConsumerCreditInput<'a> {
    graph: &'a ModuleGraph,
    consumer: &'a ResolvedModule,
    import: &'a crate::resolve::ResolvedImport,
    prefix_match: &'a str,
    target_module_idx: usize,
    path: Option<ReferencePathId>,
    pending: &'a mut Vec<PendingCredit>,
    reference_paths: &'a mut ReferencePathInterner,
}

fn collect_credits_for_alias(input: NamespaceCreditInput<'_>) {
    let NamespaceCreditInput {
        graph,
        indexes,
        alias_file_id,
        alias,
        target_module_idx,
        reachable,
        routes,
        pending,
        reference_paths,
    } = input;
    let prefix_match = format!(".{}", alias.suffix);
    for export in reachable.iter() {
        for indexed in indexes.consumers_for(export.file_id, &export.exported_name) {
            let consumer = indexed.consumer;
            let import = indexed.import;
            if consumer.file_id == alias_file_id {
                continue;
            }
            let path = routes.consumer_path(export, indexed, reference_paths);
            collect_credits_for_consumer_import(&mut ConsumerCreditInput {
                graph,
                consumer,
                import,
                prefix_match: &prefix_match,
                target_module_idx,
                path,
                pending,
                reference_paths,
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
    let path = input.path;
    let pending = &mut *input.pending;
    let reference_paths = &mut *input.reference_paths;

    let consumer_local = import.info.local_name.as_str();
    if consumer_local.is_empty() {
        return;
    }
    for access in &consumer.member_accesses {
        // Zero-allocation equivalent of `access.object == format!("{consumer_local}{prefix_match}")`.
        let object_matches = access
            .object
            .strip_prefix(consumer_local)
            .is_some_and(|rest| rest == prefix_match);
        if !object_matches {
            continue;
        }
        pending.push(PendingCredit {
            target_module_idx,
            member: access.member.clone(),
            consumer_file_id: consumer.file_id,
            import_span: import.info.span,
            path,
        });
        // The chain walker only produces credits when the alias target has an
        // `export * as <member>` edge, so skip building its state machine (and
        // formatting the accessor prefix) for the common leaf-target case.
        let target_has_chained_re_export =
            graph.modules.get(target_module_idx).is_some_and(|barrel| {
                barrel
                    .re_exports
                    .iter()
                    .any(|edge| edge.imported_name == "*" && edge.exported_name == access.member)
            });
        if !target_has_chained_re_export {
            continue;
        }
        let mut ctx = ChainWalkContext {
            graph,
            consumer,
            import_span: import.info.span,
            pending,
            reference_paths,
        };
        collect_chained_re_export_credits(
            &mut ctx,
            target_module_idx,
            &access.member,
            &format!("{consumer_local}{prefix_match}.{}", access.member),
            path,
        );
    }
}

/// Invariant context passed through the chain walker: the read-only graph,
/// the consumer module producing the accesses, and the original import span
/// to use as the `from` site on every resulting `SymbolReference`. Grouped
/// into a struct so the recursive helper keeps one explicit traversal contract.
struct ChainWalkContext<'a> {
    graph: &'a ModuleGraph,
    consumer: &'a ResolvedModule,
    import_span: oxc_span::Span,
    pending: &'a mut Vec<PendingCredit>,
    reference_paths: &'a mut ReferencePathInterner,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct AliasChainStateKey {
    module_idx: usize,
    credited_name: String,
    accessor_prefix: String,
}

struct AliasChainState {
    key: AliasChainStateKey,
    successors: Vec<usize>,
}

struct AliasChain {
    states: Vec<AliasChainState>,
}

impl AliasChain {
    fn build(
        graph: &ModuleGraph,
        consumer: &ResolvedModule,
        target_module_idx: usize,
        credited_name: &str,
        accessor_prefix: &str,
    ) -> Self {
        let root = AliasChainStateKey {
            module_idx: target_module_idx,
            credited_name: credited_name.to_string(),
            accessor_prefix: accessor_prefix.to_string(),
        };
        let mut states = vec![AliasChainState {
            key: root.clone(),
            successors: Vec::new(),
        }];
        let mut state_by_key = FxHashMap::from_iter([(root, 0)]);
        let mut frontier = VecDeque::from([0]);

        while let Some(state_index) = frontier.pop_front() {
            let state = &states[state_index].key;
            let Some(barrel) = graph.modules.get(state.module_idx) else {
                continue;
            };
            let mut chained_targets: Vec<FileId> = barrel
                .re_exports
                .iter()
                .filter(|edge| {
                    edge.imported_name == "*" && edge.exported_name == state.credited_name
                })
                .map(|edge| edge.source_file)
                .collect();
            chained_targets.sort_unstable_by_key(|file_id| file_id.0);
            chained_targets.dedup();

            let mut accessed_members: Vec<String> = consumer
                .member_accesses
                .iter()
                .filter(|access| access.object == state.accessor_prefix)
                .map(|access| access.member.clone())
                .collect();
            accessed_members.sort_unstable();
            accessed_members.dedup();

            let accessor_prefix = state.accessor_prefix.clone();
            for source_file in chained_targets {
                let Some(source_module_idx) = module_index_for_file(graph, source_file) else {
                    continue;
                };
                for member in &accessed_members {
                    let key = AliasChainStateKey {
                        module_idx: source_module_idx,
                        credited_name: member.clone(),
                        accessor_prefix: format!("{accessor_prefix}.{member}"),
                    };
                    let successor = if let Some(index) = state_by_key.get(&key) {
                        *index
                    } else {
                        let index = states.len();
                        states.push(AliasChainState {
                            key: key.clone(),
                            successors: Vec::new(),
                        });
                        state_by_key.insert(key, index);
                        frontier.push_back(index);
                        index
                    };
                    states[state_index].successors.push(successor);
                }
            }
        }
        for state in &mut states {
            state.successors.sort_unstable();
            state.successors.dedup();
        }
        Self { states }
    }

    fn collect_credits(&self, ctx: &mut ChainWalkContext<'_>, base_path: Option<ReferencePathId>) {
        if self.states.len() == 1 {
            return;
        }

        if !ctx.reference_paths.tracks_provenance() {
            debug_assert!(base_path.is_none());
            for state in self.states.iter().skip(1).map(|state| &state.key) {
                ctx.pending.push(PendingCredit {
                    target_module_idx: state.module_idx,
                    member: state.credited_name.clone(),
                    consumer_file_id: ctx.consumer.file_id,
                    import_span: ctx.import_span,
                    path: None,
                });
            }
            return;
        }

        let mut canonical_order: Vec<usize> = (0..self.states.len()).collect();
        canonical_order.sort_unstable_by(|&left, &right| {
            let left_state = &self.states[left].key;
            let right_state = &self.states[right].key;
            (
                ctx.graph.modules[left_state.module_idx].file_id.0,
                left_state.credited_name.as_str(),
                left_state.accessor_prefix.as_str(),
            )
                .cmp(&(
                    ctx.graph.modules[right_state.module_idx].file_id.0,
                    right_state.credited_name.as_str(),
                    right_state.accessor_prefix.as_str(),
                ))
        });

        let mut route_node_by_state = vec![ReferenceRouteNodeId(0); self.states.len()];
        for (route_index, state_index) in canonical_order.iter().copied().enumerate() {
            route_node_by_state[state_index] = ReferenceRouteNodeId(route_index as u32);
        }

        let nodes = canonical_order
            .iter()
            .map(|&state_index| {
                let state = &self.states[state_index];
                ReferenceRouteNodeSpec::new(
                    ctx.graph.modules[state.key.module_idx].file_id,
                    ModuleLoadMechanism::EsModule,
                    state
                        .successors
                        .iter()
                        .map(|&successor| route_node_by_state[successor])
                        .collect(),
                )
            })
            .collect();
        let graph = ctx
            .reference_paths
            .intern_route_graph(ReferenceRouteGraphSpec::new(nodes));
        let root = route_node_by_state[0];

        for state_index in canonical_order {
            if state_index == 0 {
                continue;
            }
            let state = &self.states[state_index].key;
            let path = ctx.reference_paths.route(
                base_path,
                graph,
                root,
                route_node_by_state[state_index],
                None,
            );
            ctx.pending.push(PendingCredit {
                target_module_idx: state.module_idx,
                member: state.credited_name.clone(),
                consumer_file_id: ctx.consumer.file_id,
                import_span: ctx.import_span,
                path,
            });
        }
    }

    #[cfg(test)]
    fn state_count(&self) -> usize {
        self.states.len()
    }

    #[cfg(test)]
    fn transition_count(&self) -> usize {
        self.states.iter().map(|state| state.successors.len()).sum()
    }
}

/// Follow `export * as <name> from './source'` chains on the alias target
/// side. When `barrel_module_idx.re_exports` contains an edge with
/// `imported_name == "*" && exported_name == credited_name`, every consumer
/// access of the form `<accessor_prefix>.<X>` becomes a credit for `<X>` on
/// the re-export's `source_file`. Recurses if the new credit also lands on
/// another namespace re-export, bounded by `visited` to short-circuit cycles.
fn collect_chained_re_export_credits(
    ctx: &mut ChainWalkContext<'_>,
    barrel_module_idx: usize,
    credited_name: &str,
    accessor_prefix: &str,
    path: Option<ReferencePathId>,
) {
    AliasChain::build(
        ctx.graph,
        ctx.consumer,
        barrel_module_idx,
        credited_name,
        accessor_prefix,
    )
    .collect_credits(ctx, path);
}

/// Apply collected credits, grouping by `(target_module_idx, consumer, import_span)`
/// so each (consumer file, namespace target) pair runs through the same
/// `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports`
/// pipeline that `narrow_namespace_references` uses for direct namespace
/// imports. The synthetic-export step is what handles the case where the
/// namespace target is a star barrel (`export * from './bar'`): missing
/// member exports are stubbed so Phase 4 chain resolution can propagate the
/// reference to the real defining file.
fn apply_pending_credits(graph: &mut ModuleGraph, pending: Vec<PendingCredit>) {
    type GroupKey = (usize, FileId, oxc_span::Span, Option<ReferencePathId>);

    let mut groups: FxHashMap<GroupKey, Vec<String>> = FxHashMap::default();
    for credit in pending {
        groups
            .entry((
                credit.target_module_idx,
                credit.consumer_file_id,
                credit.import_span,
                credit.path,
            ))
            .or_default()
            .push(credit.member);
    }

    let mut dedup = ReferenceDedup::default();
    for ((target_module_idx, consumer_file_id, import_span, path), members) in groups {
        let module = &mut graph.modules[target_module_idx];
        let site = ReferenceSite::exact(consumer_file_id, import_span, path);
        let found_members = mark_member_exports_referenced_at_site(
            &mut module.exports,
            module.file_id,
            site,
            &members,
            ReferenceKind::NamespaceImport,
            &mut dedup,
        );
        create_synthetic_exports_for_star_re_exports_at_site(
            &mut module.exports,
            &module.re_exports,
            site,
            &members,
            &found_members,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::resolve::{ResolveResult, ResolvedModule, ResolvedReExport};
    use fallow_types::discover::DiscoveredFile;
    use fallow_types::extract::{MemberAccess, ReExportInfo};

    fn build_namespace_reexport_graph(adjacency: &[Vec<u32>]) -> ModuleGraph {
        let files: Vec<_> = adjacency
            .iter()
            .enumerate()
            .map(|(index, _)| DiscoveredFile {
                id: FileId(index as u32),
                path: PathBuf::from(format!("/project/module-{index}.ts")),
                size_bytes: 10,
            })
            .collect();
        let modules: Vec<_> = adjacency
            .iter()
            .enumerate()
            .map(|(index, targets)| ResolvedModule {
                file_id: FileId(index as u32),
                path: files[index].path.clone(),
                re_exports: targets
                    .iter()
                    .map(|&target| ResolvedReExport {
                        info: ReExportInfo {
                            source: format!("./module-{target}"),
                            imported_name: "*".to_string(),
                            exported_name: "N".to_string(),
                            is_type_only: false,
                            span: oxc_span::Span::default(),
                            statement_span: oxc_span::Span::new(0, 0),
                            source_span: oxc_span::Span::new(0, 0),
                        },
                        target: ResolveResult::InternalModule(FileId(target)),
                    })
                    .collect(),
                ..Default::default()
            })
            .collect();
        ModuleGraph::build(&modules, &[], &files)
    }

    fn chain_consumer(depth: usize) -> ResolvedModule {
        let mut object = "API.foo.N".to_string();
        let mut member_accesses = Vec::with_capacity(depth);
        for _ in 0..depth {
            member_accesses.push(MemberAccess {
                object: object.clone(),
                member: "N".to_string(),
            });
            object.push_str(".N");
        }
        ResolvedModule {
            file_id: FileId(u32::MAX),
            member_accesses,
            ..Default::default()
        }
    }

    #[test]
    fn dense_alias_cycle_is_bounded_by_member_states() {
        const MODULES: usize = 10;
        const DEPTH: usize = 8;
        let adjacency: Vec<Vec<u32>> = (0..MODULES)
            .map(|source| {
                (0..MODULES)
                    .filter(|&target| target != source)
                    .map(|target| target as u32)
                    .collect()
            })
            .collect();
        let graph = build_namespace_reexport_graph(&adjacency);
        let consumer = chain_consumer(DEPTH);

        let chain = AliasChain::build(&graph, &consumer, 0, "N", "API.foo.N");

        assert!(chain.state_count() <= 1 + (MODULES * DEPTH));
        assert!(chain.transition_count() <= MODULES * (MODULES - 1) * DEPTH);
    }

    #[test]
    fn repeated_alias_diamonds_grow_by_states_and_edges_not_paths() {
        const LAYERS: usize = 24;
        let module_count = 1 + (LAYERS * 2);
        let mut adjacency = vec![Vec::new(); module_count];
        let mut previous = vec![0_usize];
        let mut next_module = 1_usize;
        for _ in 0..LAYERS {
            let next = vec![next_module, next_module + 1];
            next_module += 2;
            for source in &previous {
                adjacency[*source].extend(next.iter().map(|target| *target as u32));
            }
            previous = next;
        }
        let graph = build_namespace_reexport_graph(&adjacency);
        let consumer = chain_consumer(LAYERS);

        let chain = AliasChain::build(&graph, &consumer, 0, "N", "API.foo.N");

        assert_eq!(chain.state_count(), 1 + (LAYERS * 2));
        assert_eq!(chain.transition_count(), 2 + ((LAYERS - 1) * 4));
    }

    #[test]
    fn untracked_alias_chain_credits_states_without_interning_routes() {
        let mut graph = build_namespace_reexport_graph(&[vec![1], vec![]]);
        let consumer = chain_consumer(1);
        let chain = AliasChain::build(&graph, &consumer, 0, "N", "API.foo.N");
        let mut pending = Vec::new();
        let mut reference_paths = ReferencePathInterner::new(false);

        chain.collect_credits(
            &mut ChainWalkContext {
                graph: &graph,
                consumer: &consumer,
                import_span: oxc_span::Span::new(3, 9),
                pending: &mut pending,
                reference_paths: &mut reference_paths,
            },
            None,
        );

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target_module_idx, 1);
        assert_eq!(pending[0].member, "N");
        assert!(pending[0].path.is_none());
        let finalized = reference_paths.finalize(&mut graph.modules);
        assert!(finalized.paths.is_empty());
        assert!(finalized.routes.graphs.is_empty());
    }
}
