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
    let factory_local_keys = context.indexes.local_keys(factory_origin_file_id);
    let Some(class_seed_keys) = factory_local_keys.get(class_local_name) else {
        return;
    };
    for class_seed in class_seed_keys {
        for class_origin in export_key_with_origins(context.graph, class_seed) {
            let class_has_members = context
                .indexes
                .module_by_id
                .get(&class_origin.file_id)
                .is_some_and(|class_module| {
                    export_is_class_with_members(class_module, class_origin.export_name.as_str())
                });
            if class_has_members {
                context
                    .accessed_members
                    .entry(class_origin)
                    .or_default()
                    .insert(member.to_string());
            }
        }
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
    let mut credit_context = FactoryReturnCreditContext {
        graph,
        indexes,
        accessed_members,
    };

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in factory_fn_member_accesses(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(access.callee_name.as_str()) else {
                continue;
            };
            for seed_key in seed_keys {
                for factory_origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(factory_module) = credit_context
                        .indexes
                        .module_by_id
                        .get(&factory_origin.file_id)
                    else {
                        continue;
                    };
                    // (2) the origin must declare an exported factory-return for
                    // this export name, the cross-module over-credit gate.
                    let Some(factory_return) =
                        factory_module
                            .exported_factory_returns
                            .iter()
                            .find(|factory_return| {
                                factory_origin.export_name.as_str()
                                    == factory_return.export_name.as_str()
                            })
                    else {
                        continue;
                    };
                    // (3) resolve the returned class's LOCAL name through the
                    // factory module's own imports/exports to a class export.
                    credit_factory_return_class_member(
                        &mut credit_context,
                        factory_origin.file_id,
                        factory_return.class_local_name.as_str(),
                        access.member.as_str(),
                    );
                }
            }
        }
    }
}
