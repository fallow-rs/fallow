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

    augment_with_inherited_bindings(graph, modules, indexes, &mut targets_by_class);

    targets_by_class
}

/// Inherit a base class's instance-binding fields into its subclasses so a
/// `this.<inherited-field>.<member>()` access resolves to the field's terminal
/// class (issue #1910). A generic-typed base field (`client: TClient`) is
/// substituted with the subclass's concrete `extends Base<Concrete>` type
/// argument, resolved through the subclass's own imports; a non-generic field
/// copies the base's already-resolved targets. Additive and shadowing-aware: a
/// field the subclass binds itself is never overwritten, and an unresolvable
/// substitution credits nothing (false-negative-preferring).
fn augment_with_inherited_bindings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
    targets_by_class: &mut FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
) {
    let mut heritage_by_key: FxHashMap<ExportKey, &fallow_types::extract::ClassHeritageInfo> =
        FxHashMap::default();
    for module in modules {
        if !indexes.module_by_id.contains_key(&module.file_id) {
            continue;
        }
        for heritage in &module.class_heritage {
            heritage_by_key.insert(
                ExportKey::new(module.file_id, heritage.export_name.clone()),
                heritage,
            );
        }
    }

    // Compute every inherited-field augmentation while reading the fully-built
    // base map immutably, then apply them so own bindings always win.
    let mut augmentations: Vec<(ExportKey, FxHashMap<String, Vec<ExportKey>>)> = Vec::new();
    for module in modules {
        if !indexes.module_by_id.contains_key(&module.file_id) {
            continue;
        }
        for heritage in &module.class_heritage {
            if heritage.super_class.is_none() {
                continue;
            }
            let inherited = collect_inherited_bindings(
                graph,
                indexes,
                &heritage_by_key,
                targets_by_class,
                module.file_id,
                heritage,
            );
            if !inherited.is_empty() {
                let class_key = ExportKey::new(module.file_id, heritage.export_name.clone());
                augmentations.push((class_key, inherited));
            }
        }
    }

    for (class_key, inherited) in augmentations {
        let entry = targets_by_class.entry(class_key).or_default();
        for (field, targets) in inherited {
            entry.entry(field).or_insert(targets);
        }
    }
}

/// Walk a class's `extends` chain collecting the terminal export keys of each
/// inherited instance-binding field the class does not bind itself.
fn collect_inherited_bindings(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    heritage_by_key: &FxHashMap<ExportKey, &fallow_types::extract::ClassHeritageInfo>,
    targets_by_class: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    child_file_id: FileId,
    child: &fallow_types::extract::ClassHeritageInfo,
) -> FxHashMap<String, Vec<ExportKey>> {
    let mut inherited: FxHashMap<String, Vec<ExportKey>> = FxHashMap::default();

    // Fields the child binds itself shadow inherited ones and must be excluded.
    let mut owned_fields: FxHashSet<&str> = FxHashSet::default();
    for (field, _) in &child.instance_bindings {
        owned_fields.insert(field.as_str());
    }
    for (field, _) in &child.generic_instance_bindings {
        owned_fields.insert(field.as_str());
    }

    let mut visited: FxHashSet<ExportKey> = FxHashSet::default();
    let mut parent_local = child.super_class.clone();
    // `type_args` are the concrete `<...>` args passed to the current parent,
    // written in `type_args_file_id`'s scope (the child's file for the direct
    // parent); `resolver_file_id` is the scope that resolves `parent_local`.
    let mut type_args = child.super_class_type_args.clone();
    let mut type_args_file_id = child_file_id;
    let mut resolver_file_id = child_file_id;

    while let Some(local) = parent_local {
        let Some(parent_key) =
            resolve_parent_class_key(graph, indexes, heritage_by_key, resolver_file_id, &local)
        else {
            break;
        };
        if !visited.insert(parent_key.clone()) {
            break;
        }
        let Some(parent) = heritage_by_key.get(&parent_key) else {
            break;
        };

        // Generic fields: substitute with the concrete type arg for this level.
        let mut generic_fields: FxHashSet<&str> = FxHashSet::default();
        for (field, index) in &parent.generic_instance_bindings {
            generic_fields.insert(field.as_str());
            if owned_fields.contains(field.as_str()) || inherited.contains_key(field) {
                continue;
            }
            let Some(concrete) = type_args.get(*index) else {
                continue;
            };
            if concrete.is_empty() {
                continue;
            }
            let targets = resolve_type_targets(graph, indexes, type_args_file_id, concrete);
            if !targets.is_empty() {
                inherited.insert(field.clone(), targets);
            }
        }

        // Non-generic fields: copy the parent's already-resolved targets.
        if let Some(parent_targets) = targets_by_class.get(&parent_key) {
            for (field, _) in &parent.instance_bindings {
                if owned_fields.contains(field.as_str())
                    || inherited.contains_key(field)
                    || generic_fields.contains(field.as_str())
                {
                    continue;
                }
                if let Some(targets) = parent_targets.get(field) {
                    inherited.insert(field.clone(), targets.clone());
                }
            }
        }

        // Ascend to the grandparent. The next level's type args are written in
        // this parent's file, so both the resolver and type-arg scopes move there.
        let next_type_args = parent.super_class_type_args.clone();
        let next_parent_local = parent.super_class.clone();
        resolver_file_id = parent_key.file_id;
        type_args_file_id = parent_key.file_id;
        type_args = next_type_args;
        parent_local = next_parent_local;
    }

    inherited
}

/// Resolve a super-class local name (in `file_id`'s scope) to the canonical
/// export key of the class that actually declares heritage.
fn resolve_parent_class_key(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    heritage_by_key: &FxHashMap<ExportKey, &fallow_types::extract::ClassHeritageInfo>,
    file_id: FileId,
    local: &str,
) -> Option<ExportKey> {
    let seed_keys = indexes.local_keys(file_id).get(local)?;
    for seed_key in seed_keys {
        for key in export_key_with_origins(graph, seed_key) {
            if heritage_by_key.contains_key(&key) {
                return Some(key);
            }
        }
    }
    None
}

/// Resolve a concrete type name (in `file_id`'s scope) to its export keys.
fn resolve_type_targets(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    file_id: FileId,
    type_name: &str,
) -> Vec<ExportKey> {
    let mut targets = Vec::new();
    if let Some(seed_keys) = indexes.local_keys(file_id).get(type_name) {
        for seed_key in seed_keys {
            for key in export_key_with_origins(graph, seed_key) {
                push_export_key(&mut targets, key);
            }
        }
    }
    targets
}

/// The export keys of every class declaring heritage in each module, so a
/// `this.<field>...` chain can seed on the accessing module's classes.
fn build_this_root_keys(
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<FileId, Vec<ExportKey>> {
    let mut map: FxHashMap<FileId, Vec<ExportKey>> = FxHashMap::default();
    for module in modules {
        if !indexes.module_by_id.contains_key(&module.file_id) {
            continue;
        }
        for heritage in &module.class_heritage {
            map.entry(module.file_id)
                .or_default()
                .push(ExportKey::new(module.file_id, heritage.export_name.clone()));
        }
    }
    map
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
    this_root_keys: &[ExportKey],
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
    // A `this.<field>...` chain roots on the accessing module's classes (the
    // enclosing class is not recorded on the access, so every class in the file
    // is seeded; over-credit only, issue #1910). All other roots resolve through
    // the local binding table as before.
    let root_keys: &[ExportKey] = if root_local == "this" {
        this_root_keys
    } else {
        match local_to_export_keys.get(root_local) {
            Some(keys) => keys.as_slice(),
            None => return Vec::new(),
        }
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
    let this_root_keys_by_file = build_this_root_keys(modules, indexes);
    let empty_this_roots: Vec<ExportKey> = Vec::new();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        let this_root_keys = this_root_keys_by_file
            .get(&resolved.file_id)
            .unwrap_or(&empty_this_roots);
        propagate_typed_member_accesses(
            graph,
            resolved,
            &typed_instance_targets,
            local_to_export_keys,
            this_root_keys,
            accessed_members,
        );
        propagate_typed_whole_object_uses(
            graph,
            resolved,
            &typed_instance_targets,
            local_to_export_keys,
            this_root_keys,
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
    this_root_keys: &[ExportKey],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for access in SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses)
        .ordinary_member_accesses()
    {
        for target_key in resolve_typed_instance_chain_targets(
            graph,
            typed_instance_targets,
            local_to_export_keys,
            this_root_keys,
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
    this_root_keys: &[ExportKey],
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    for object_name in ordinary_whole_object_uses(&resolved.whole_object_uses) {
        for target_key in resolve_typed_instance_chain_targets(
            graph,
            typed_instance_targets,
            local_to_export_keys,
            this_root_keys,
            object_name,
        ) {
            whole_object_used_exports.insert(target_key);
        }
    }
}
