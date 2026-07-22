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

fn build_typed_instance_binding_targets(
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
#[derive(Clone)]
struct ScopedTypeArgument {
    name: String,
    file_id: FileId,
}

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
    let mut shadowed_fields: FxHashSet<String> = FxHashSet::default();
    for (field, _) in &child.instance_bindings {
        shadowed_fields.insert(field.clone());
    }
    for (field, _) in &child.generic_instance_bindings {
        shadowed_fields.insert(field.clone());
    }

    let mut visited: FxHashSet<ExportKey> = FxHashSet::default();
    let mut parent_local = child.super_class.clone();
    // Each argument retains the file scope where its concrete name was written.
    // That scope can survive several generic forwarding hops.
    let mut type_args = child
        .super_class_type_args
        .iter()
        .map(|name| {
            (!name.is_empty()).then(|| ScopedTypeArgument {
                name: name.clone(),
                file_id: child_file_id,
            })
        })
        .collect::<Vec<_>>();
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
            if shadowed_fields.contains(field) || inherited.contains_key(field) {
                continue;
            }
            let Some(Some(concrete)) = type_args.get(*index) else {
                continue;
            };
            let targets =
                resolve_type_targets(graph, indexes, concrete.file_id, concrete.name.as_str());
            if !targets.is_empty() {
                inherited.insert(field.clone(), targets);
            }
        }

        // Non-generic fields: copy the parent's already-resolved targets.
        if let Some(parent_targets) = targets_by_class.get(&parent_key) {
            for (field, _) in &parent.instance_bindings {
                if shadowed_fields.contains(field)
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

        // Every declaration on this inheritance level shadows same-named
        // fields farther up the chain, even when its type cannot be resolved.
        shadowed_fields.extend(
            parent
                .generic_instance_bindings
                .iter()
                .map(|(field, _)| field.clone()),
        );
        shadowed_fields.extend(
            parent
                .instance_bindings
                .iter()
                .map(|(field, _)| field.clone()),
        );

        // Ascend to the grandparent. A parent argument matching one of this
        // class's type parameters forwards the already-composed concrete arg;
        // an ordinary bare name remains scoped to the parent file.
        let next_type_args = compose_super_type_arguments(parent, &type_args, parent_key.file_id);
        let next_parent_local = parent.super_class.clone();
        resolver_file_id = parent_key.file_id;
        type_args = next_type_args;
        parent_local = next_parent_local;
    }

    inherited
}

fn compose_super_type_arguments(
    class: &fallow_types::extract::ClassHeritageInfo,
    current: &[Option<ScopedTypeArgument>],
    class_file_id: FileId,
) -> Vec<Option<ScopedTypeArgument>> {
    class
        .super_class_type_args
        .iter()
        .map(|name| {
            if name.is_empty() {
                return None;
            }
            if let Some(index) = class
                .type_parameters
                .iter()
                .position(|parameter| parameter == name)
            {
                return current.get(index).cloned().flatten();
            }
            Some(ScopedTypeArgument {
                name: name.clone(),
                file_id: class_file_id,
            })
        })
        .collect()
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

fn resolve_typed_instance_chain_targets(
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
    let root_keys = match local_to_export_keys.get(root_local) {
        Some(keys) => keys.as_slice(),
        None => return Vec::new(),
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

fn resolve_class_this_chain_targets(
    graph: &ModuleGraph,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    indexes: &MemberPassIndexes<'_>,
    file_id: FileId,
    class_local_name: &str,
    object_name: &str,
) -> Vec<ExportKey> {
    let Some(path) = object_name.strip_prefix("this.") else {
        return Vec::new();
    };
    let path_segments = path.split('.').collect::<Vec<_>>();
    if path_segments.is_empty() {
        return Vec::new();
    }

    let mut root_keys = indexes
        .local_keys(file_id)
        .get(class_local_name)
        .cloned()
        .unwrap_or_default();
    if root_keys.is_empty() && class_local_name == "default" {
        root_keys.push(ExportKey::new(file_id, "default"));
    }

    let mut targets = Vec::new();
    for root_key in root_keys {
        for target_key in
            chained_typed_instance_targets(graph, typed_instance_targets, &root_key, &path_segments)
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
            indexes,
            accessed_members,
        );
        propagate_typed_whole_object_uses(
            graph,
            resolved,
            &typed_instance_targets,
            local_to_export_keys,
            indexes,
            whole_object_used_exports,
        );
    }
}

/// Credit each ordinary member access in one module onto the typed-instance
/// chain's target export keys.
fn propagate_typed_member_accesses(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let facts = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    for access in facts
        .ordinary_member_accesses()
        .filter(|access| !access.object.starts_with("this."))
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

    for access in facts.class_this_member_accesses() {
        for target_key in resolve_class_this_chain_targets(
            graph,
            typed_instance_targets,
            indexes,
            resolved.file_id,
            &access.class_local_name,
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
fn propagate_typed_whole_object_uses(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    indexes: &MemberPassIndexes<'_>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    let facts = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    for object_name in ordinary_whole_object_uses(&resolved.whole_object_uses)
        .filter(|object| !object.starts_with("this."))
    {
        for target_key in resolve_typed_instance_chain_targets(
            graph,
            typed_instance_targets,
            local_to_export_keys,
            object_name,
        ) {
            whole_object_used_exports.insert(target_key);
        }
    }

    for use_fact in facts.class_this_whole_object_uses() {
        for target_key in resolve_class_this_chain_targets(
            graph,
            typed_instance_targets,
            indexes,
            resolved.file_id,
            &use_fact.class_local_name,
            &use_fact.object,
        ) {
            whole_object_used_exports.insert(target_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_type_arguments_preserve_forwarded_scope_and_parent_scope() {
        let class = fallow_types::extract::ClassHeritageInfo {
            export_name: "Intermediate".to_string(),
            super_class: Some("Base".to_string()),
            implements: Vec::new(),
            type_parameters: vec!["T".to_string(), "U".to_string()],
            instance_bindings: Vec::new(),
            super_class_type_args: vec![
                "T".to_string(),
                "LocalClient".to_string(),
                String::new(),
                "U".to_string(),
            ],
            generic_instance_bindings: Vec::new(),
        };
        let concrete_file = FileId(7);
        let parent_file = FileId(3);
        let current = vec![
            Some(ScopedTypeArgument {
                name: "ConcreteClient".to_string(),
                file_id: concrete_file,
            }),
            None,
        ];

        let composed = compose_super_type_arguments(&class, &current, parent_file);

        assert_eq!(
            composed[0]
                .as_ref()
                .map(|argument| (argument.name.as_str(), argument.file_id)),
            Some(("ConcreteClient", concrete_file))
        );
        assert_eq!(
            composed[1]
                .as_ref()
                .map(|argument| (argument.name.as_str(), argument.file_id)),
            Some(("LocalClient", parent_file))
        );
        assert!(composed[2].is_none());
        assert!(composed[3].is_none());
    }
}
