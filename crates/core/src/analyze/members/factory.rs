use super::*;

/// Credit member accesses produced by static-factory call bindings on the
/// originating class export.
pub(super) fn propagate_factory_call_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in factory_call_member_accesses(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(access.callee_object.as_str()) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = indexes.module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let matches_factory = origin_module.exports.iter().any(|export| {
                        export.name.matches_str(origin.export_name.as_str())
                            && export.members.iter().any(|member| {
                                member.is_instance_returning_static
                                    && member.kind == MemberKind::ClassMethod
                                    && member.name == access.callee_method
                            })
                    });
                    if !matches_factory {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}

pub(super) struct FactoryReturnCreditContext<'a, 'ctx> {
    pub(super) graph: &'ctx ModuleGraph,
    pub(super) indexes: &'ctx MemberPassIndexes<'a>,
    pub(super) accessed_members: &'ctx mut FxHashMap<ExportKey, FxHashSet<String>>,
}

pub(super) fn credit_factory_return_class_member(
    context: &mut FactoryReturnCreditContext<'_, '_>,
    factory_origin_file_id: FileId,
    class_local_name: &str,
    member: &str,
) {
    for class_origin in factory_return_class_origins(
        context.graph,
        context.indexes,
        factory_origin_file_id,
        class_local_name,
    ) {
        context
            .accessed_members
            .entry(class_origin)
            .or_default()
            .insert(member.to_string());
    }
}

/// Credit member accesses produced by cross-module free-function factory
/// bindings (`const x = importedFactory(); x.member`) onto the class the factory
/// returns. Each link in the resolution chain is also an over-credit guard, and
/// a wrong credit is a silent false-negative, so every link must hold:
///
///   1. the fact's callee resolves through the consumer's imports/exports to an
///      export key (`local_to_export_keys`);
///   2. that key walks (re-export aware) to an origin module that actually
///      declares an `exported_factory_returns` entry for the export, i.e. an
///      internal exported factory proven to return a single class;
///   3. the entry's `class_local_name` resolves through THAT factory module's own
///      imports/exports to a class export;
///   4. the resolved export is a class with members.
///
/// See issue #1441 (Part A).
pub(super) fn propagate_factory_fn_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in factory_fn_member_accesses(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(access.callee_name.as_str()) else {
                continue;
            };
            let classes = seed_keys
                .iter()
                .flat_map(|seed_key| factory_return_classes_for_callee(graph, indexes, seed_key));
            for class_origin in classes {
                accessed_members
                    .entry(class_origin)
                    .or_default()
                    .insert(access.member.clone());
            }
        }
    }
}

/// The class exports a proven exported factory returns, resolved from the factory's
/// own module. Shared by the member-credit and whole-object-suppress passes: each
/// link is an over-credit gate, so a callee that is not a proven factory yields
/// nothing.
fn factory_return_class_origins(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    factory_origin_file_id: FileId,
    class_local_name: &str,
) -> Vec<ExportKey> {
    let factory_local_keys = indexes.local_keys(factory_origin_file_id);
    let Some(class_seed_keys) = factory_local_keys.get(class_local_name) else {
        return Vec::new();
    };
    class_seed_keys
        .iter()
        .flat_map(|class_seed| export_key_with_origins(graph, class_seed))
        .filter(|class_origin| {
            indexes
                .module_by_id
                .get(&class_origin.file_id)
                .is_some_and(|class_module| {
                    export_is_class_with_members(class_module, class_origin.export_name.as_str())
                })
        })
        .collect()
}

/// The classes a callee's proven exported factory returns, resolved across modules.
/// Empty for any callee that is not an internal exported factory with a strict,
/// value-proven return: each link of the chain is an over-credit gate.
fn factory_return_classes_for_callee(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    seed_key: &ExportKey,
) -> Vec<ExportKey> {
    let mut origins = Vec::new();
    for factory_origin in
        walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
    {
        let Some(factory_module) = indexes.module_by_id.get(&factory_origin.file_id) else {
            continue;
        };
        let Some(factory_return) =
            factory_module
                .exported_factory_returns
                .iter()
                .find(|factory_return| {
                    factory_origin.export_name.as_str() == factory_return.export_name.as_str()
                })
        else {
            continue;
        };
        origins.extend(factory_return_class_origins(
            graph,
            indexes,
            factory_origin.file_id,
            factory_return.class_local_name.as_str(),
        ));
    }
    origins
}

/// Suppress a class whose factory-returned instance is consumed opaquely
/// (`const { a, ...rest } = importedFactory()`, a computed destructure key).
///
/// The consumer can read any property, so no set of member accesses describes what
/// is used. Crediting only the keys the pattern names would leave every other live
/// member reported as dead. Mark the export wholly used instead; the member scan
/// then skips it, exactly as it does for a class whose instance escapes.
pub(super) fn propagate_factory_fn_whole_object_uses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for fact in factory_fn_whole_objects(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(fact.callee_name.as_str()) else {
                continue;
            };
            let classes = seed_keys
                .iter()
                .flat_map(|seed_key| factory_return_classes_for_callee(graph, indexes, seed_key));
            whole_object_used_exports.extend(classes);
        }
    }
}
