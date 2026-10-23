use super::*;

/// Whether `export` has a class method `name` that returns `this`.
fn is_self_returning_method(export: &crate::extract::ExportInfo, name: &str) -> bool {
    export.members.iter().any(|member| {
        member.kind == MemberKind::ClassMethod && member.name == name && member.is_self_returning
    })
}

/// How many leading `steps` a chain credits on one class export: each step
/// up to and including the first step that does not return `this`. A step is
/// used when every earlier step returns `this`.
fn credited_step_count(export: &crate::extract::ExportInfo, steps: &[String]) -> usize {
    steps
        .iter()
        .position(|step| !is_self_returning_method(export, step))
        .map_or(steps.len(), |index| index + 1)
}

/// The steps a factory-rooted chain credits on a single class export.
fn factory_chain_credits(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    root_method: &str,
    steps: &[String],
) -> usize {
    if !export.name.matches_str(origin.export_name.as_str()) {
        return 0;
    }
    let has_factory = export.members.iter().any(|member| {
        member.is_instance_returning_static
            && member.kind == MemberKind::ClassMethod
            && member.name == root_method
    });
    if !has_factory {
        return 0;
    }
    credited_step_count(export, steps)
}

/// Credit member accesses produced by fluent-builder chain calls.
pub(super) fn propagate_fluent_chain_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in fluent_chain_member_accesses(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(access.root_object.as_str()) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = indexes.module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let credited = origin_module
                        .exports
                        .iter()
                        .map(|export| {
                            factory_chain_credits(
                                export,
                                &origin,
                                access.root_method.as_str(),
                                &access.steps,
                            )
                        })
                        .max()
                        .unwrap_or(0);
                    if credited == 0 {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .extend(access.steps[..credited].iter().cloned());
                }
            }
        }
    }
}

/// The steps a constructor-rooted chain credits on a single class export.
fn new_chain_credits(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    first_method: &str,
    steps: &[String],
) -> usize {
    if !export.name.matches_str(origin.export_name.as_str())
        || !is_self_returning_method(export, first_method)
    {
        return 0;
    }
    credited_step_count(export, steps)
}

/// Credit member accesses produced by fluent chains rooted at `new`.
pub(super) fn propagate_fluent_chain_new_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in fluent_chain_new_member_accesses(resolved) {
            let Some(seed_keys) = local_to_export_keys.get(access.class_name.as_str()) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = indexes.module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let credited = origin_module
                        .exports
                        .iter()
                        .map(|export| {
                            new_chain_credits(
                                export,
                                &origin,
                                access.first_method.as_str(),
                                &access.steps,
                            )
                        })
                        .max()
                        .unwrap_or(0);
                    if credited == 0 {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .extend(access.steps[..credited].iter().cloned());
                }
            }
        }
    }
}
