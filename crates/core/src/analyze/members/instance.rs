use super::*;

pub(super) fn build_instance_export_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut targets_by_instance: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in instance_export_bindings(resolved) {
            let Some(target_keys) = local_to_export_keys.get(access.target_name.as_str()) else {
                continue;
            };

            let instance_key = ExportKey::new(resolved.file_id, access.export_name.clone());
            let instance_targets = targets_by_instance.entry(instance_key).or_default();
            for target_key in target_keys {
                for key in export_key_with_origins(graph, target_key) {
                    push_export_key(instance_targets, key);
                }
            }
        }
    }

    targets_by_instance
}

pub(super) fn propagate_accesses_through_instance_exports(
    instance_targets: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    if instance_targets.is_empty() {
        return;
    }

    let accessed_snapshot: Vec<(ExportKey, Vec<String>)> = accessed_members
        .iter()
        .map(|(key, members)| (key.clone(), members.iter().cloned().collect()))
        .collect();
    for (instance_key, members) in accessed_snapshot {
        let Some(target_keys) = instance_targets.get(&instance_key) else {
            continue;
        };
        for target_key in target_keys {
            accessed_members
                .entry(target_key.clone())
                .or_default()
                .extend(members.iter().cloned());
        }
    }

    let whole_snapshot: Vec<ExportKey> = whole_object_used_exports.iter().cloned().collect();
    for instance_key in whole_snapshot {
        let Some(target_keys) = instance_targets.get(&instance_key) else {
            continue;
        };
        whole_object_used_exports.extend(target_keys.iter().cloned());
    }
}

pub(super) fn build_typed_instance_binding_targets(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let mut targets_by_class: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();

    for module in modules {
        if !indexes.module_by_id.contains_key(&module.file_id) {
            continue;
        }
        let local_to_export_keys = indexes.local_keys(module.file_id);
        for heritage in &module.class_heritage {
            if heritage.instance_bindings.is_empty() {
                continue;
            }
            let class_key = ExportKey::new(module.file_id, heritage.export_name.clone());
            let member_targets = targets_by_class.entry(class_key).or_default();

            for (member_name, type_name) in &heritage.instance_bindings {
                let Some(seed_keys) = local_to_export_keys.get(type_name.as_str()) else {
                    continue;
                };
                let targets = member_targets.entry(member_name.clone()).or_default();
                for seed_key in seed_keys {
                    for key in export_key_with_origins(graph, seed_key) {
                        push_export_key(targets, key);
                    }
                }
            }
        }
    }

    targets_by_class
}

pub(super) fn chained_typed_instance_targets(
    graph: &ModuleGraph,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    seed_key: &ExportKey,
    segments: &[&str],
) -> Vec<ExportKey> {
    let mut current = export_key_with_origins(graph, seed_key);

    for segment in segments {
        let mut next = Vec::new();
        for class_key in &current {
            let Some(member_targets) = typed_instance_targets.get(class_key) else {
                continue;
            };
            let Some(targets) = member_targets.get(*segment) else {
                continue;
            };
            for target in targets {
                push_export_key(&mut next, target.clone());
            }
        }
        if next.is_empty() {
            return Vec::new();
        }
        current = next;
    }

    current
}

pub(super) fn resolve_typed_instance_chain_targets(
    graph: &ModuleGraph,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    object_name: &str,
) -> Vec<ExportKey> {
    let mut segments = object_name.split('.');
    let Some(root_local) = segments.next() else {
        return Vec::new();
    };
    let path_segments: Vec<&str> = segments.collect();
    if path_segments.is_empty() {
        return Vec::new();
    }
    let Some(root_keys) = local_to_export_keys.get(root_local) else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    for root_key in root_keys {
        for target_key in
            chained_typed_instance_targets(graph, typed_instance_targets, root_key, &path_segments)
        {
            push_export_key(&mut targets, target_key);
        }
    }
    targets
}

pub(super) fn propagate_accesses_through_typed_instance_bindings(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    let typed_instance_targets = build_typed_instance_binding_targets(graph, modules, indexes);
    if typed_instance_targets.is_empty() {
        return;
    }

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        propagate_typed_member_accesses(
            graph,
            resolved,
            &typed_instance_targets,
            local_to_export_keys,
            accessed_members,
        );
        propagate_typed_whole_object_uses(
            graph,
            resolved,
            &typed_instance_targets,
            local_to_export_keys,
            whole_object_used_exports,
        );
    }
}

/// Credit each ordinary member access in one module onto the typed-instance
/// chain's target export keys.
pub(super) fn propagate_typed_member_accesses(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for access in SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses)
        .ordinary_member_accesses()
    {
        for target_key in resolve_typed_instance_chain_targets(
            graph,
            typed_instance_targets,
            local_to_export_keys,
            &access.object,
        ) {
            accessed_members
                .entry(target_key)
                .or_default()
                .insert(access.member.clone());
        }
    }
}

/// Mark each ordinary whole-object use in one module as whole-object-used on the
/// typed-instance chain's target export keys.
pub(super) fn propagate_typed_whole_object_uses(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    for object_name in ordinary_whole_object_uses(&resolved.whole_object_uses) {
        for target_key in resolve_typed_instance_chain_targets(
            graph,
            typed_instance_targets,
            local_to_export_keys,
            object_name,
        ) {
            whole_object_used_exports.insert(target_key);
        }
    }
}
