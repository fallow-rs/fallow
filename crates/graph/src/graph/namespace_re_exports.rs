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

use rustc_hash::FxHashMap;
#[cfg(test)]
use rustc_hash::FxHashSet;

use super::ModuleGraph;
use super::namespace_indexes::{NamespacePropagationIndexes, ReachableNamespaceExports};
use super::narrowing::{
    ReferenceSite, create_synthetic_exports_for_star_re_exports_at_site,
    mark_all_exports_referenced_at_site, mark_member_exports_referenced_at_site,
};
use super::types::{ReferenceKind, ReferencePathId, ReferencePathInterner};
use fallow_types::discover::FileId;
use fallow_types::extract::ModuleLoadMechanism;

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
    /// Exact consumer-to-target path, including every named/star barrel.
    path: ReferencePathId,
}

/// Phase 2c: credit `export * as Foo from './bar'` member accesses onto `./bar`.
pub(super) fn propagate_namespace_re_exports(
    graph: &mut ModuleGraph,
    indexes: &NamespacePropagationIndexes<'_>,
    reference_paths: &mut ReferencePathInterner,
) {
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

        let reachable = indexes.enumerate_reachable_barrels(*barrel_file_id, exported_name);

        for export in reachable.iter().filter(|export| {
            graph
                .modules
                .get(export.file_id.0 as usize)
                .is_some_and(super::types::ModuleNode::is_entry_point)
        }) {
            let path = reachable.entry_path(
                export,
                *source_file_id,
                ModuleLoadMechanism::EsModule,
                reference_paths,
            );
            pending.push(PendingCredit {
                target_module_idx,
                kind: CreditKind::AllExports,
                consumer_file_id: export.file_id,
                import_span: oxc_span::Span::default(),
                path,
            });
        }

        let context = ConsumerCreditContext {
            indexes,
            seed_barrel_file: *barrel_file_id,
            target_module_idx,
            final_target: *source_file_id,
        };
        collect_consumer_credits(&context, &reachable, &mut pending, reference_paths);
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

/// For every consumer in `module_by_id` that imports a name reachable from
/// the seed namespace re-export, collect a `PendingCredit` per
/// `<local>.<member>` access and per whole-object use.
struct ConsumerCreditContext<'indexes, 'modules> {
    indexes: &'indexes NamespacePropagationIndexes<'modules>,
    seed_barrel_file: FileId,
    target_module_idx: usize,
    final_target: FileId,
}

fn collect_consumer_credits(
    context: &ConsumerCreditContext<'_, '_>,
    reachable: &ReachableNamespaceExports,
    pending: &mut Vec<PendingCredit>,
    reference_paths: &mut ReferencePathInterner,
) {
    for export in reachable.iter() {
        for indexed in context
            .indexes
            .consumers_for(export.file_id, &export.exported_name)
        {
            let consumer = indexed.consumer;
            let import = indexed.import;
            if consumer.file_id == context.seed_barrel_file {
                continue;
            }
            let path = reachable.consumer_path(
                export,
                indexed,
                context.final_target,
                ModuleLoadMechanism::EsModule,
                reference_paths,
            );

            let consumer_local = import.info.local_name.as_str();
            if consumer_local.is_empty() {
                continue;
            }

            if consumer.unused_import_bindings.contains(consumer_local) {
                continue;
            }

            let whole_object = consumer
                .whole_object_uses
                .iter()
                .any(|n| n == consumer_local);
            if whole_object {
                pending.push(PendingCredit {
                    target_module_idx: context.target_module_idx,
                    kind: CreditKind::AllExports,
                    consumer_file_id: consumer.file_id,
                    import_span: import.info.span,
                    path,
                });
                continue;
            }

            for access in &consumer.member_accesses {
                if access.object != consumer_local {
                    continue;
                }
                pending.push(PendingCredit {
                    target_module_idx: context.target_module_idx,
                    kind: CreditKind::Member(access.member.clone()),
                    consumer_file_id: consumer.file_id,
                    import_span: import.info.span,
                    path,
                });
            }
        }
    }
}

/// Apply the collected credits, grouping by the exact reference site so each
/// `(consumer file, namespace target, import site, path)` runs through the
/// same `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports`
/// pipeline that `narrow_namespace_references` uses for direct namespace
/// imports. `AllExports` credits short-circuit to `mark_all_exports_referenced`
/// for the whole-object and entry-point cases.
fn apply_pending_credits(graph: &mut ModuleGraph, pending: &[PendingCredit]) {
    type GroupKey = (usize, FileId, oxc_span::Span, ReferencePathId);

    let mut groups: FxHashMap<GroupKey, GroupState> = FxHashMap::default();
    for credit in pending {
        let key = (
            credit.target_module_idx,
            credit.consumer_file_id,
            credit.import_span,
            credit.path,
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

    for ((target_module_idx, consumer_file_id, import_span, path), state) in groups {
        let module = &mut graph.modules[target_module_idx];
        let site = ReferenceSite::exact(consumer_file_id, import_span, path);
        if state.whole_object {
            mark_all_exports_referenced_at_site(
                &mut module.exports,
                site,
                ReferenceKind::NamespaceImport,
            );
        } else {
            let found = mark_member_exports_referenced_at_site(
                &mut module.exports,
                site,
                &state.members,
                ReferenceKind::NamespaceImport,
            );
            create_synthetic_exports_for_star_re_exports_at_site(
                &mut module.exports,
                &module.re_exports,
                site,
                &state.members,
                &found,
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
    use crate::resolve::{
        ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport,
        ResolvedReplacedModuleTarget,
    };
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess, ReExportInfo, VisibilityTag,
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
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 10),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn named_import_from(source: &str, name: &str, target: FileId) -> ResolvedImport {
        named_import_with_mechanism(source, name, target, false)
    }

    fn named_import_with_mechanism(
        source: &str,
        name: &str,
        target: FileId,
        commonjs: bool,
    ) -> ResolvedImport {
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
            target: if commonjs {
                ResolveResult::CommonJsInternalModule(target)
            } else {
                ResolveResult::InternalModule(target)
            },
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

    fn namespace_coverage_graph(commonjs_consumer: bool) -> ModuleGraph {
        let files = vec![
            discovered_file(0, "/project/consumer.test.ts", 100),
            discovered_file(1, "/project/barrel.ts", 50),
            discovered_file(2, "/project/source.ts", 50),
            discovered_file(3, "/project/alternate.ts", 50),
        ];
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![
                    named_import_with_mechanism("./barrel", "Ns", FileId(1), commonjs_consumer),
                    ResolvedImport {
                        info: ImportInfo {
                            source: "./alternate".to_string(),
                            imported_name: ImportedName::SideEffect,
                            local_name: String::new(),
                            is_type_only: false,
                            from_style: false,
                            span: oxc_span::Span::new(20, 30),
                            source_span: oxc_span::Span::default(),
                        },
                        target: ResolveResult::InternalModule(FileId(3)),
                    },
                ],
                member_accesses: vec![MemberAccess {
                    object: "Ns".to_string(),
                    member: "used".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                re_exports: vec![ns_re_export("./source", "Ns", FileId(2))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![named_export("used")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./source".to_string(),
                        imported_name: ImportedName::SideEffect,
                        local_name: String::new(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                }],
                ..Default::default()
            },
        ];
        let test_entries = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::TestFile,
        }];

        ModuleGraph::build_with_reachability_roots_and_replacements(
            &modules,
            &[ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
            &test_entries,
            &[],
            &test_entries,
            &files,
        )
    }

    #[test]
    fn mocked_namespace_barrel_stays_uncovered_when_target_has_an_alternate_route() {
        let graph = namespace_coverage_graph(false);
        let reference = &graph.modules[2].exports[0].references[0];

        assert!(graph.modules[2].is_test_reachable());
        assert_eq!(
            graph.reference_path_hops(reference),
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::EsModule),
            ]
        );
        assert!(!graph.is_test_reference_covered(reference));
    }

    #[test]
    fn commonjs_namespace_consumer_retains_its_exact_load_path() {
        let graph = namespace_coverage_graph(true);
        let reference = &graph.modules[2].exports[0].references[0];

        assert_eq!(
            graph.reference_path_hops(reference),
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::CommonJsRequire),
            ]
        );
        assert!(graph.is_test_reference_covered(reference));
    }

    #[test]
    fn issue_324_simple_namespace_re_export_credits_target_members() {
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
                whole_object_uses: vec!["Ns".to_string()].into(),
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
