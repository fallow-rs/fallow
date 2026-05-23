//! Phase 2c: namespace re-export propagation.
//!
//! Handles the `export * as Foo from './bar'` pattern. The barrel records a
//! `ReExportEdge { source_file: ./bar, imported_name: "*", exported_name: "Foo" }`
//! plus a synthesised stub `ExportSymbol` named `"Foo"` (see `build_module_node`).
//! A downstream consumer that does `import { Foo } from './barrel'` and
//! accesses `Foo.member` records the access in its own `member_accesses` as
//! `{ object: "Foo", member: "member" }`, but neither the barrel's stub nor
//! `./bar`'s real exports get a reference because:
//!
//! 1. `attach_symbol_reference` (Phase 2) only narrows namespace member
//!    accesses for `ImportedName::Namespace` imports, not for `ImportedName::Named`.
//! 2. `propagate_named_re_export` (Phase 4) looks for a source export matching
//!    the edge's `imported_name`, which here is the literal `"*"` and never
//!    matches a real export name.
//!
//! This pass walks every namespace re-export edge, enumerates the consumer
//! files that import the re-exported name (directly or through outer named
//! re-export barrels), collects each consumer's member accesses on its local
//! binding, and credits accessed members on the namespace target file via the
//! same `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports`
//! pair that `narrow_namespace_references` uses for direct namespace imports.
//! Whole-object uses (`Object.values(Foo)`, spread, destructure-with-rest)
//! credit every target export. Barrels that expose the namespace through an
//! entry point also credit every target export, mirroring the entry-point
//! semantics of `propagate_entry_point_star`.
//!
//! Runs after Phase 2b (cross-package alias propagation) and before Phase 3
//! (reachability) so credits attached here participate in reachability and
//! Phase 4 chain propagation downstream. See issue #324.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::ImportedName;

use crate::resolve::ResolvedModule;

use super::ModuleGraph;
use super::narrowing::{
    create_synthetic_exports_for_star_re_exports, mark_all_exports_referenced,
    mark_member_exports_referenced,
};
use super::types::ReferenceKind;

/// Either credit a specific member on the target, or credit every export
/// (whole-object use, or entry-point exposure where the external accesses
/// are unknown).
enum CreditKind {
    Member(String),
    AllExports,
}

struct PendingCredit {
    /// Index into `ModuleGraph::modules` of the namespace target file.
    target_module_idx: usize,
    /// What to credit on the target.
    kind: CreditKind,
    /// File whose code produced the access (used as `from_file` on the
    /// resulting `SymbolReference`).
    consumer_file_id: FileId,
    /// Span of the consumer's import that brought the re-exported binding
    /// into scope; used as `import_span` on the resulting reference.
    import_span: oxc_span::Span,
}

/// Phase 2c: credit `export * as Foo from './bar'` member accesses onto `./bar`.
pub(super) fn propagate_namespace_re_exports(
    graph: &mut ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
) {
    // Collect every `export * as Name from './source'` edge across all barrels.
    // Tuple: (barrel_file_id, source_file_id, exported_name).
    //
    // Note: `is_type_only` is intentionally not filtered here. A
    // `export type * as Ns from './bar'` re-export may still have its
    // accessed members consumed from type positions on the source file,
    // and the existing `attach_direct_export_references` two-namespace
    // (type vs value) split runs on the synthesised `Ns` stub on the
    // barrel, not on the source target. Crediting at the target here
    // keeps `unused-type` findings on type members of `./bar` accurate
    // for type-only namespace re-exports, matching the value case.
    let ns_edges: Vec<(FileId, FileId, String)> = graph
        .modules
        .iter()
        .flat_map(|m| {
            let barrel_file = m.file_id;
            m.re_exports.iter().filter_map(move |re| {
                if re.imported_name == "*" && re.exported_name != "*" {
                    Some((barrel_file, re.source_file, re.exported_name.clone()))
                } else {
                    None
                }
            })
        })
        .collect();

    if ns_edges.is_empty() {
        return;
    }

    let mut pending: Vec<PendingCredit> = Vec::new();

    for (barrel_file_id, source_file_id, exported_name) in &ns_edges {
        let Some(target_module_idx) = module_index_for_file(graph, *source_file_id) else {
            continue;
        };

        // Walk forward through named re-exports so consumers that import
        // `Foo` from an OUTER barrel (rather than the original `export * as`
        // barrel) also match. This mirrors the multi-hop fix from issue #310.
        let reachable = enumerate_reachable_barrels(graph, *barrel_file_id, exported_name);

        // Entry-point barrel: namespace is exposed to external consumers.
        // Without seeing their code, fallow conservatively credits every
        // target export. Mirrors `propagate_entry_point_star` in Phase 4.
        // `consumer_file_id` is set to the seed barrel itself because there
        // is no real importing file: the barrel is the entry-point and the
        // namespace is consumed externally. Using the barrel as the synthetic
        // `from_file` keeps the reference attributable in dedup checks
        // (`attach_reference` skips duplicates per `source_id`).
        if reachable.iter().any(|(file_id, _)| {
            graph
                .modules
                .get(file_id.0 as usize)
                .is_some_and(super::types::ModuleNode::is_entry_point)
        }) {
            pending.push(PendingCredit {
                target_module_idx,
                kind: CreditKind::AllExports,
                consumer_file_id: *barrel_file_id,
                import_span: oxc_span::Span::default(),
            });
        }

        collect_consumer_credits(
            module_by_id,
            *barrel_file_id,
            target_module_idx,
            &reachable,
            &mut pending,
        );
    }

    apply_pending_credits(graph, &pending);
}

/// Map a `FileId` to its index in `graph.modules`. Returns `None` if the id
/// is out of range (defensive: should not happen for FileIds emitted by the
/// build pipeline).
fn module_index_for_file(graph: &ModuleGraph, file_id: FileId) -> Option<usize> {
    let idx = file_id.0 as usize;
    (idx < graph.modules.len()).then_some(idx)
}

/// Walk forward through named re-export edges starting from
/// `(seed_file, seed_name)` and return every reachable
/// `(barrel_file_id, exported_name_at_barrel)` pair, including the seed.
///
/// Mirrors the named-rename and star-passthrough behavior in
/// `namespace_aliases::enumerate_alias_reachable_barrels`:
///
/// - `export { A as B } from './seed'` yields `(barrel, "B")`.
/// - `export * from './seed'` propagates the source name unchanged.
/// - `export * as ns from './seed'` is intentionally NOT followed: at that
///   barrel the original identifier is hidden behind `ns.<name>` rather than
///   exposed directly, so the seed name is no longer reachable as itself.
/// - Cycles are bounded by the `reachable` set used as a visited marker.
fn enumerate_reachable_barrels(
    graph: &ModuleGraph,
    seed_file: FileId,
    seed_name: &str,
) -> FxHashSet<(FileId, String)> {
    let mut reachable: FxHashSet<(FileId, String)> = FxHashSet::default();
    reachable.insert((seed_file, seed_name.to_string()));
    let mut frontier: Vec<(FileId, String)> = vec![(seed_file, seed_name.to_string())];

    while let Some((source_file, source_name)) = frontier.pop() {
        for (idx, module) in graph.modules.iter().enumerate() {
            for edge in &module.re_exports {
                if edge.source_file != source_file {
                    continue;
                }
                let exported_name = if edge.imported_name == source_name {
                    edge.exported_name.clone()
                } else if edge.imported_name == "*" && edge.exported_name == "*" {
                    source_name.clone()
                } else {
                    continue;
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "file count is bounded by project size, well under u32::MAX"
                )]
                let barrel_file = FileId(idx as u32);
                let pair = (barrel_file, exported_name);
                if reachable.insert(pair.clone()) {
                    frontier.push(pair);
                }
            }
        }
    }

    reachable
}

/// For every consumer in `module_by_id` that imports a name reachable from
/// the seed namespace re-export, collect a `PendingCredit` per
/// `<local>.<member>` access and per whole-object use.
fn collect_consumer_credits(
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    seed_barrel_file: FileId,
    target_module_idx: usize,
    reachable: &FxHashSet<(FileId, String)>,
    pending: &mut Vec<PendingCredit>,
) {
    for consumer in module_by_id.values() {
        // A barrel that contains the seed namespace re-export does not also
        // "consume" the binding through itself; skip to avoid spurious
        // self-credits. (Outer named-re-export barrels are still scanned for
        // their own consumers via the reachable set.)
        if consumer.file_id == seed_barrel_file {
            continue;
        }
        for import in &consumer.resolved_imports {
            let Some(import_target) = import.target.internal_file_id() else {
                continue;
            };
            let imported_name = match &import.info.imported_name {
                ImportedName::Named(n) => n.as_str(),
                ImportedName::Default => "default",
                _ => continue,
            };
            if !reachable.contains(&(import_target, imported_name.to_string())) {
                continue;
            }

            let consumer_local = import.info.local_name.as_str();
            if consumer_local.is_empty() {
                continue;
            }

            // If the binding is reported as unused, the consumer has no
            // accesses to record. Skip without scanning to avoid emitting a
            // SymbolReference for what was effectively dead code.
            if consumer.unused_import_bindings.contains(consumer_local) {
                continue;
            }

            let whole_object = consumer
                .whole_object_uses
                .iter()
                .any(|n| n == consumer_local);
            if whole_object {
                pending.push(PendingCredit {
                    target_module_idx,
                    kind: CreditKind::AllExports,
                    consumer_file_id: consumer.file_id,
                    import_span: import.info.span,
                });
                continue;
            }

            for access in &consumer.member_accesses {
                if access.object != consumer_local {
                    continue;
                }
                pending.push(PendingCredit {
                    target_module_idx,
                    kind: CreditKind::Member(access.member.clone()),
                    consumer_file_id: consumer.file_id,
                    import_span: import.info.span,
                });
            }
        }
    }
}

/// Apply the collected credits, grouping by `(target_module_idx, consumer_file_id, import_span)`
/// so each `(consumer file, namespace target, import site)` runs through the
/// same `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports`
/// pipeline that `narrow_namespace_references` uses for direct namespace
/// imports. `AllExports` credits short-circuit to `mark_all_exports_referenced`
/// for the whole-object and entry-point cases.
fn apply_pending_credits(graph: &mut ModuleGraph, pending: &[PendingCredit]) {
    type GroupKey = (usize, FileId, oxc_span::Span);

    let mut groups: FxHashMap<GroupKey, GroupState> = FxHashMap::default();
    for credit in pending {
        let key = (
            credit.target_module_idx,
            credit.consumer_file_id,
            credit.import_span,
        );
        let entry = groups.entry(key).or_default();
        match &credit.kind {
            CreditKind::Member(name) => {
                if !entry.whole_object {
                    entry.members.push(name.clone());
                }
            }
            CreditKind::AllExports => {
                entry.whole_object = true;
                entry.members.clear();
            }
        }
    }

    for ((target_module_idx, consumer_file_id, import_span), state) in groups {
        let module = &mut graph.modules[target_module_idx];
        if state.whole_object {
            mark_all_exports_referenced(
                &mut module.exports,
                consumer_file_id,
                import_span,
                ReferenceKind::NamespaceImport,
            );
        } else {
            let found = mark_member_exports_referenced(
                &mut module.exports,
                consumer_file_id,
                &state.members,
                import_span,
                ReferenceKind::NamespaceImport,
            );
            create_synthetic_exports_for_star_re_exports(
                &mut module.exports,
                &module.re_exports,
                consumer_file_id,
                &state.members,
                &found,
                import_span,
            );
        }
    }
}

#[derive(Default)]
struct GroupState {
    members: Vec<String>,
    whole_object: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ModuleGraph;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedReExport};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::extract::{
        ExportInfo, ExportName, ImportInfo, MemberAccess, ReExportInfo, VisibilityTag,
    };
    use std::path::PathBuf;

    fn discovered_file(id: u32, path: &str, size: u64) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: size,
        }
    }

    fn named_export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 10),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn named_import_from(source: &str, name: &str, target: FileId) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: source.to_string(),
                imported_name: ImportedName::Named(name.to_string()),
                local_name: name.to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn ns_re_export(source: &str, alias: &str, target: FileId) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: source.to_string(),
                imported_name: "*".to_string(),
                exported_name: alias.to_string(),
                is_type_only: false,
                span: oxc_span::Span::new(0, 10),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn named_re_export(source: &str, name: &str, target: FileId) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: source.to_string(),
                imported_name: name.to_string(),
                exported_name: name.to_string(),
                is_type_only: false,
                span: oxc_span::Span::new(0, 10),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    #[test]
    fn issue_324_simple_namespace_re_export_credits_target_members() {
        // source-module.ts exports `someExportedSymbol`, `anotherSymbol`
        // barrel.ts re-exports as `export * as MyNamespace from './source-module'`
        // main.ts imports { MyNamespace } and accesses MyNamespace.someExportedSymbol
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source-module.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./barrel", "MyNamespace", FileId(1))],
                member_accesses: vec![MemberAccess {
                    object: "MyNamespace".to_string(),
                    member: "someExportedSymbol".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ns_re_export("./source-module", "MyNamespace", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source-module.ts"),
                exports: vec![
                    named_export("someExportedSymbol"),
                    named_export("anotherSymbol"),
                ],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let someexp = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "someExportedSymbol")
            .unwrap();
        assert!(
            !someexp.references.is_empty(),
            "someExportedSymbol should be credited via namespace re-export"
        );

        let unused = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "anotherSymbol")
            .unwrap();
        assert!(
            unused.references.is_empty(),
            "anotherSymbol stays unreferenced when only someExportedSymbol is accessed"
        );
    }

    #[test]
    fn issue_324_multi_hop_named_re_export_chain_credits_target() {
        // Multi-hop variant (implement-skill rule from incident 2026-05-08):
        // source.ts: export const used = ...
        // inner-barrel.ts: export * as Ns from './source'
        // outer-barrel.ts: export { Ns } from './inner-barrel'   (named re-export, NOT namespace)
        // main.ts: import { Ns } from './outer-barrel'; Ns.used()
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/outer-barrel.ts", 50),
            discovered_file(2, "/project/inner-barrel.ts", 50),
            discovered_file(3, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./outer-barrel", "Ns", FileId(1))],
                member_accesses: vec![MemberAccess {
                    object: "Ns".to_string(),
                    member: "used".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/outer-barrel.ts"),
                re_exports: vec![named_re_export("./inner-barrel", "Ns", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/inner-barrel.ts"),
                re_exports: vec![ns_re_export("./source", "Ns", FileId(3))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("used"), named_export("unused")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let used = graph.modules[3]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "used")
            .unwrap();
        assert!(
            !used.references.is_empty(),
            "used should be credited through two-hop barrel chain"
        );
        let still_unused = graph.modules[3]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "unused")
            .unwrap();
        assert!(
            still_unused.references.is_empty(),
            "unused stays flagged across the chain"
        );
    }

    #[test]
    fn issue_324_whole_object_use_credits_all_target_exports() {
        // main.ts: import { Ns } from './barrel'; Object.values(Ns)
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./barrel", "Ns", FileId(1))],
                whole_object_uses: vec!["Ns".to_string()],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ns_re_export("./source", "Ns", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("a"), named_export("b"), named_export("c")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        for export in &graph.modules[2].exports {
            assert!(
                !export.references.is_empty(),
                "{} should be credited under whole-object use",
                export.name
            );
        }
    }

    #[test]
    fn issue_324_entry_point_barrel_credits_all_target_exports() {
        // No internal consumer. The barrel IS the entry point and exposes
        // `export * as Ns from './source'` externally; credit all source exports.
        let files = vec![
            discovered_file(0, "/project/index.ts", 100),
            discovered_file(1, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/index.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/index.ts"),
                re_exports: vec![ns_re_export("./source", "Public", FileId(1))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("apiOne"), named_export("apiTwo")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        for export in &graph.modules[1].exports {
            assert!(
                !export.references.is_empty(),
                "{} should be credited because the namespace re-export is exposed externally",
                export.name
            );
        }
    }

    #[test]
    fn issue_324_synthetic_export_propagates_through_star_chain_on_target() {
        // source-barrel.ts: export * from './impl'
        // barrel.ts: export * as Ns from './source-barrel'
        // main.ts: import { Ns } from './barrel'; Ns.deepMember()
        //
        // The target file (source-barrel) has NO own export named `deepMember`;
        // it forwards through `export *` to impl.ts. The synthetic-export
        // helper stubs `deepMember` on source-barrel so Phase 4 chain
        // resolution carries the credit through to impl.ts.
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source-barrel.ts", 50),
            discovered_file(3, "/project/impl.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./barrel", "Ns", FileId(1))],
                member_accesses: vec![MemberAccess {
                    object: "Ns".to_string(),
                    member: "deepMember".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ns_re_export("./source-barrel", "Ns", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source-barrel.ts"),
                re_exports: vec![ResolvedReExport {
                    info: ReExportInfo {
                        source: "./impl".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                    },
                    target: ResolveResult::InternalModule(FileId(3)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/project/impl.ts"),
                exports: vec![named_export("deepMember"), named_export("unused")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let deep = graph.modules[3]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "deepMember")
            .unwrap();
        assert!(
            !deep.references.is_empty(),
            "deepMember should be credited via synthetic stub plus Phase 4 star chain"
        );
        let unused = graph.modules[3]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "unused")
            .unwrap();
        assert!(
            unused.references.is_empty(),
            "non-accessed members in the chain target stay flagged"
        );
    }

    #[test]
    fn issue_324_unused_binding_skipped() {
        // Consumer imports `Ns` but never accesses it.
        // The binding goes into unused_import_bindings, so no credits emerge.
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let mut consumer_unused = FxHashSet::default();
        consumer_unused.insert("Ns".to_string());
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./barrel", "Ns", FileId(1))],
                unused_import_bindings: consumer_unused,
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ns_re_export("./source", "Ns", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("a")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let a = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "a")
            .unwrap();
        assert!(
            a.references.is_empty(),
            "unused namespace binding should not credit any target export"
        );
    }

    #[test]
    fn issue_324_renamed_local_binding_still_credits_members() {
        // import { Foo as MyFoo } from './barrel'; MyFoo.X()
        // The reachable-set lookup keys on `imported_name="Foo"` (the
        // exported name at the barrel); the member-access match keys on
        // `local_name="MyFoo"` (the renamed local binding).
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let renamed_import = ResolvedImport {
            info: ImportInfo {
                source: "./barrel".to_string(),
                imported_name: ImportedName::Named("Foo".to_string()),
                local_name: "MyFoo".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(1)),
        };
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![renamed_import],
                member_accesses: vec![MemberAccess {
                    object: "MyFoo".to_string(),
                    member: "used".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ns_re_export("./source", "Foo", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("used"), named_export("unused")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let used = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "used")
            .unwrap();
        assert!(
            !used.references.is_empty(),
            "used credited via the renamed local binding MyFoo.used"
        );
        let unused = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "unused")
            .unwrap();
        assert!(
            unused.references.is_empty(),
            "unused stays flagged; renamed-local narrowing is precise"
        );
    }

    #[test]
    fn issue_324_plain_export_star_not_credited_by_this_pass() {
        // `export * from './source'` is NOT a namespace re-export
        // (imported_name=*, exported_name=*); Phase 4 already handles it.
        // Phase 2c must not interfere: if the consumer doesn't import any
        // re-exported name, the only credits should come from Phase 4 or 2.
        let files = vec![
            discovered_file(0, "/project/main.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source.ts", 50),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/main.ts"),
                resolved_imports: vec![named_import_from("./barrel", "fromSource", FileId(1))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/barrel.ts"),
                re_exports: vec![ResolvedReExport {
                    info: ReExportInfo {
                        source: "./source".to_string(),
                        imported_name: "*".to_string(),
                        exported_name: "*".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/source.ts"),
                exports: vec![named_export("fromSource"), named_export("untouched")],
                ..Default::default()
            },
        ];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let from_source = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "fromSource")
            .unwrap();
        assert!(
            !from_source.references.is_empty(),
            "fromSource credited via existing Phase 4 star-re-export path"
        );
        let untouched = graph.modules[2]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "untouched")
            .unwrap();
        assert!(
            untouched.references.is_empty(),
            "Phase 2c does not over-credit unrelated exports under plain export-star"
        );
    }
}
