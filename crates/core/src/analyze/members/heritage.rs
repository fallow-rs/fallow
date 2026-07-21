use super::*;

pub(super) fn heritage_clause(rule: &ScopedUsedClassMemberRule) -> String {
    match (rule.extends.as_deref(), rule.implements.as_deref()) {
        (Some(extends), Some(implements)) => {
            format!("extends='{extends}', implements='{implements}'")
        }
        (Some(extends), None) => format!("extends='{extends}'"),
        (None, Some(implements)) => format!("implements='{implements}'"),
        (None, None) => "unconstrained heritage".to_string(),
    }
}

/// Credit one Angular external-template member access `<object>.<member>` whose
/// `object` is a component field bound to `type_name` (issue #920).
///
/// `type_name` is the field's binding target: a concrete class, an interface
/// (the typed `x: Greeter = inject(TOKEN)` form), or an `InjectionToken` const
/// (the untyped `x = inject(TOKEN)` form). The candidate interface set is the
/// binding name itself plus the interface declared by any `InjectionToken` the
/// binding resolves to; the member is credited on every class implementing any
/// candidate interface (and directly on the resolved export, harmless for a
/// memberless token const).
pub(super) struct AngularTokenChainCreditInput<'a, 'b> {
    graph: &'a ModuleGraph,
    type_name: &'a str,
    member: &'a str,
    local_to_export_keys: &'a FxHashMap<&'b str, Vec<ExportKey>>,
    token_to_interface: &'a FxHashMap<ExportKey, &'a str>,
    implementers_by_name: &'a FxHashMap<&'a str, Vec<ExportKey>>,
    accessed_members: &'a mut FxHashMap<ExportKey, FxHashSet<String>>,
}

pub(super) fn credit_angular_token_chain_member(input: &mut AngularTokenChainCreditInput<'_, '_>) {
    let mut interface_names: Vec<&str> = vec![input.type_name];
    if let Some(export_keys) = input.local_to_export_keys.get(input.type_name) {
        for export_key in export_keys {
            input
                .accessed_members
                .entry(export_key.clone())
                .or_default()
                .insert(input.member.to_string());
            for resolved in export_key_with_origins(input.graph, export_key) {
                if let Some(interface) = input.token_to_interface.get(&resolved) {
                    interface_names.push(interface);
                }
            }
        }
    }
    for interface in interface_names {
        let Some(implementers) = input.implementers_by_name.get(interface) else {
            continue;
        };
        for implementer_key in implementers {
            input
                .accessed_members
                .entry(implementer_key.clone())
                .or_default()
                .insert(input.member.to_string());
        }
    }
}

pub(super) fn build_angular_template_refs(
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<FileId, Vec<&str>> {
    resolved_modules
        .iter()
        .filter_map(|module| {
            let refs: Vec<&str> =
                SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                    .angular_template_member_names()
                    .collect();
            if refs.is_empty() {
                None
            } else {
                Some((module.file_id, refs))
            }
        })
        .collect()
}

pub(super) fn build_angular_template_chain_accesses(
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<FileId, Vec<(&str, &str)>> {
    resolved_modules
        .iter()
        .filter_map(|module| {
            if !SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                .has_angular_template_members()
            {
                return None;
            }
            let chains: Vec<(&str, &str)> =
                SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
                    .ordinary_member_accesses()
                    .filter(|access| access.object != "this")
                    .map(|access| (access.object.as_str(), access.member.as_str()))
                    .collect();
            if chains.is_empty() {
                None
            } else {
                Some((module.file_id, chains))
            }
        })
        .collect()
}

pub(super) struct AngularTemplateRefContext<'a, 'b> {
    refs: &'b FxHashMap<FileId, Vec<&'a str>>,
    self_accessed_members: &'b mut FxHashMap<FileId, FxHashSet<String>>,
}

impl AngularTemplateRefContext<'_, '_> {
    fn propagate(&mut self, resolved_modules: &[ResolvedModule]) {
        if self.refs.is_empty() {
            return;
        }

        for resolved in resolved_modules {
            if let Some(refs) = self.refs.get(&resolved.file_id) {
                let entry = self
                    .self_accessed_members
                    .entry(resolved.file_id)
                    .or_default();
                for &ref_name in refs {
                    entry.insert(ref_name.to_string());
                }
            }
            for import in resolved.all_resolved_imports() {
                if let Some(target_id) = import.target.internal_file_id()
                    && let Some(refs) = self.refs.get(&target_id)
                {
                    let entry = self
                        .self_accessed_members
                        .entry(resolved.file_id)
                        .or_default();
                    for &ref_name in refs {
                        entry.insert(ref_name.to_string());
                    }
                }
            }
        }
    }
}

pub(super) fn component_bindings(
    resolved: &ResolvedModule,
    class_heritage: &[fallow_types::extract::ClassHeritageInfo],
) -> FxHashMap<String, String> {
    let mut bindings: FxHashMap<String, String> = class_heritage
        .iter()
        .flat_map(|heritage| {
            heritage
                .instance_bindings
                .iter()
                .map(|(local, ty)| (local.clone(), ty.clone()))
        })
        .collect();
    for field in SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses)
        .angular_component_field_array_types()
    {
        bindings.entry(field.field).or_insert(field.element_class);
    }
    bindings
}

pub(super) struct MemberHeritageContext<'a> {
    pub(super) class_heritage_by_export: FxHashMap<ExportKey, (Option<String>, Vec<String>)>,
    pub(super) class_heritage_by_file:
        FxHashMap<FileId, &'a [fallow_types::extract::ClassHeritageInfo]>,
    pub(super) token_to_interface: FxHashMap<ExportKey, &'a str>,
    pub(super) implementers_by_name: FxHashMap<&'a str, Vec<ExportKey>>,
    pub(super) interface_to_implementers: FxHashMap<ExportKey, Vec<ExportKey>>,
}

pub(super) fn build_member_heritage_context<'a>(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &'a [ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
) -> MemberHeritageContext<'a> {
    let mut class_heritage_by_export: FxHashMap<ExportKey, (Option<String>, Vec<String>)> =
        FxHashMap::default();
    let mut class_heritage_by_file = FxHashMap::default();
    let mut token_to_interface: FxHashMap<ExportKey, &str> = FxHashMap::default();
    let mut implementers_by_name: FxHashMap<&str, Vec<ExportKey>> = FxHashMap::default();

    for module in modules {
        class_heritage_by_file.insert(module.file_id, module.class_heritage.as_slice());
        class_heritage_by_export.extend(module.class_heritage.iter().map(|heritage| {
            (
                ExportKey::new(module.file_id, heritage.export_name.clone()),
                (heritage.super_class.clone(), heritage.implements.clone()),
            )
        }));
        for (token_name, interface_name) in &module.injection_tokens {
            token_to_interface.insert(
                ExportKey::new(module.file_id, token_name.clone()),
                interface_name.as_str(),
            );
        }
        for heritage in &module.class_heritage {
            let implementer_key = ExportKey::new(module.file_id, heritage.export_name.clone());
            for interface_name in &heritage.implements {
                implementers_by_name
                    .entry(interface_name.as_str())
                    .or_default()
                    .push(implementer_key.clone());
            }
        }
    }

    let interface_to_implementers =
        build_interface_to_implementers(graph, resolved_modules, &class_heritage_by_file, indexes);

    MemberHeritageContext {
        class_heritage_by_export,
        class_heritage_by_file,
        token_to_interface,
        implementers_by_name,
        interface_to_implementers,
    }
}

pub(super) fn propagate_interface_member_accesses(
    interface_to_implementers: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    if interface_to_implementers.is_empty() {
        return;
    }

    let mut propagations: Vec<(ExportKey, Vec<String>)> = Vec::new();
    for (interface_key, implementer_keys) in interface_to_implementers {
        let Some(interface_accesses) = accessed_members.get(interface_key) else {
            continue;
        };
        let accesses: Vec<String> = interface_accesses.iter().cloned().collect();
        for implementer_key in implementer_keys {
            propagations.push((implementer_key.clone(), accesses.clone()));
        }
    }

    for (implementer_key, accesses) in propagations {
        accessed_members
            .entry(implementer_key)
            .or_default()
            .extend(accesses);
    }
}

pub(super) fn propagate_angular_template_member_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    heritage_context: &MemberHeritageContext<'_>,
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: &mut FxHashMap<FileId, FxHashSet<String>>,
) {
    let angular_tpl_refs = build_angular_template_refs(resolved_modules);
    let mut angular_ref_context = AngularTemplateRefContext {
        refs: &angular_tpl_refs,
        self_accessed_members,
    };
    angular_ref_context.propagate(resolved_modules);

    let angular_tpl_chain_accesses = build_angular_template_chain_accesses(resolved_modules);
    let mut angular_chain_context = AngularTemplateChainContext {
        graph,
        class_heritage_by_file: &heritage_context.class_heritage_by_file,
        chain_accesses: &angular_tpl_chain_accesses,
        token_to_interface: &heritage_context.token_to_interface,
        implementers_by_name: &heritage_context.implementers_by_name,
        accessed_members,
    };
    angular_chain_context.propagate(resolved_modules, indexes);
}

pub(super) struct AngularTemplateChainContext<'a, 'b> {
    graph: &'b ModuleGraph,
    class_heritage_by_file: &'b FxHashMap<FileId, &'a [fallow_types::extract::ClassHeritageInfo]>,
    chain_accesses: &'b FxHashMap<FileId, Vec<(&'b str, &'b str)>>,
    token_to_interface: &'b FxHashMap<ExportKey, &'a str>,
    implementers_by_name: &'b FxHashMap<&'a str, Vec<ExportKey>>,
    accessed_members: &'b mut FxHashMap<ExportKey, FxHashSet<String>>,
}

pub(super) struct AngularTemplateComponentContext<'b> {
    component_bindings: FxHashMap<String, String>,
    local_to_export_keys: FxHashMap<&'b str, Vec<ExportKey>>,
}

impl AngularTemplateChainContext<'_, '_> {
    fn credit_members(
        &mut self,
        chains: &[(&str, &str)],
        component: &AngularTemplateComponentContext<'_>,
    ) {
        for (object, member) in chains {
            let Some(type_name) = component.component_bindings.get(*object) else {
                continue;
            };
            credit_angular_token_chain_member(&mut AngularTokenChainCreditInput {
                graph: self.graph,
                type_name,
                member,
                local_to_export_keys: &component.local_to_export_keys,
                token_to_interface: self.token_to_interface,
                implementers_by_name: self.implementers_by_name,
                accessed_members: self.accessed_members,
            });
        }
    }

    fn propagate(&mut self, resolved_modules: &[ResolvedModule], indexes: &MemberPassIndexes<'_>) {
        if self.chain_accesses.is_empty() {
            return;
        }

        for resolved in resolved_modules {
            let Some(class_heritage) = self.class_heritage_by_file.get(&resolved.file_id) else {
                continue;
            };
            // This context stores an OWNED key map (converting it to a borrow
            // fights the struct's lifetimes), so clone from the shared index; the
            // map is still built once per scan rather than per pass.
            let component = AngularTemplateComponentContext {
                component_bindings: component_bindings(resolved, class_heritage),
                local_to_export_keys: indexes.local_keys(resolved.file_id).clone(),
            };
            if component.component_bindings.is_empty() {
                continue;
            }
            if let Some(chains) = self.chain_accesses.get(&resolved.file_id) {
                self.credit_members(chains, &component);
            }
            for import in resolved.all_resolved_imports() {
                let Some(target_id) = import.target.internal_file_id() else {
                    continue;
                };
                let Some(chains) = self.chain_accesses.get(&target_id) else {
                    continue;
                };
                self.credit_members(chains, &component);
            }
        }
    }
}

/// Build `parent_export -> [child_export, ...]` from each exported class's
/// `extends` clause.
pub(super) fn build_parent_to_children(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut parent_to_children: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();
    // O(1) dedup across the whole build: a `contains` scan of the growing child
    // Vec is O(children^2) for a widely-extended base (issue #1843 follow-up).
    // Keyed on (parent key, child key); the Vec insertion order is unchanged.
    let mut seen_pairs: FxHashSet<(ExportKey, ExportKey)> = FxHashSet::default();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);

        for export in &resolved.exports {
            if let Some(super_local) = &export.super_class {
                let Some(parent_keys) = local_to_export_keys.get(super_local.as_str()) else {
                    continue;
                };
                let child_key = ExportKey::new(resolved.file_id, export.name.to_string());

                for parent_key in parent_keys {
                    for resolved_parent_key in export_key_with_origins(graph, parent_key) {
                        if seen_pairs.insert((resolved_parent_key.clone(), child_key.clone())) {
                            parent_to_children
                                .entry(resolved_parent_key)
                                .or_default()
                                .push(child_key.clone());
                        }
                    }
                }
            }
        }
    }

    parent_to_children
}

/// Propagate member accesses through `extends` chains in both directions.
pub(super) fn propagate_class_inheritance(
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: &mut FxHashMap<FileId, FxHashSet<String>>,
) {
    if parent_to_children.is_empty() {
        return;
    }

    let mut propagations: Vec<(FileId, Vec<String>)> = Vec::new();

    for (parent_key, children) in parent_to_children {
        collect_self_access_inheritance_propagations(
            parent_key,
            children,
            self_accessed_members,
            &mut propagations,
        );
        propagate_member_accesses_through_inheritance(parent_key, children, accessed_members);
    }

    for (file_id, members) in propagations {
        let entry = self_accessed_members.entry(file_id).or_default();
        for member in members {
            entry.insert(member);
        }
    }
}

pub(super) fn collect_self_access_inheritance_propagations(
    parent_key: &ExportKey,
    children: &[ExportKey],
    self_accessed_members: &FxHashMap<FileId, FxHashSet<String>>,
    propagations: &mut Vec<(FileId, Vec<String>)>,
) {
    if let Some(parent_self_accesses) = self_accessed_members.get(&parent_key.file_id) {
        let accesses: Vec<String> = parent_self_accesses.iter().cloned().collect();
        for child_key in children {
            propagations.push((child_key.file_id, accesses.clone()));
        }
    }

    let mut child_self_accesses_for_parent: FxHashSet<String> = FxHashSet::default();
    for child_key in children {
        if let Some(child_self_accesses) = self_accessed_members.get(&child_key.file_id) {
            child_self_accesses_for_parent.extend(child_self_accesses.iter().cloned());
        }
    }
    if !child_self_accesses_for_parent.is_empty() {
        propagations.push((
            parent_key.file_id,
            child_self_accesses_for_parent.into_iter().collect(),
        ));
    }
}

pub(super) fn propagate_member_accesses_through_inheritance(
    parent_key: &ExportKey,
    children: &[ExportKey],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let parent_accesses = accessed_members.get(parent_key).cloned();
    let mut child_accesses_to_propagate: FxHashSet<String> = FxHashSet::default();

    for child_key in children {
        if let Some(child_accesses) = accessed_members.get(child_key) {
            child_accesses_to_propagate.extend(child_accesses.iter().cloned());
        }
    }

    if let Some(ref parent_acc) = parent_accesses {
        for child_key in children {
            accessed_members
                .entry(child_key.clone())
                .or_default()
                .extend(parent_acc.iter().cloned());
        }
    }

    if !child_accesses_to_propagate.is_empty() {
        accessed_members
            .entry(parent_key.clone())
            .or_default()
            .extend(child_accesses_to_propagate);
    }
}

pub(super) fn build_interface_to_implementers(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    class_heritage_by_file: &FxHashMap<FileId, &[fallow_types::extract::ClassHeritageInfo]>,
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut interface_to_implementers: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();
    // O(1) dedup across the whole build: a `contains` scan of the growing
    // implementer Vec is O(implementers^2) for a widely-implemented interface
    // (issue #1843 follow-up). Keyed on (interface key, implementer key); the Vec
    // insertion order is unchanged.
    let mut seen_pairs: FxHashSet<(ExportKey, ExportKey)> = FxHashSet::default();
    for resolved in resolved_modules {
        let Some(class_heritage) = class_heritage_by_file.get(&resolved.file_id) else {
            continue;
        };
        if class_heritage.is_empty() {
            continue;
        }

        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for heritage in *class_heritage {
            if heritage.implements.is_empty() {
                continue;
            }

            let implementer_key = ExportKey::new(resolved.file_id, heritage.export_name.clone());
            for interface_name in &heritage.implements {
                let Some(interface_keys) = local_to_export_keys.get(interface_name.as_str()) else {
                    continue;
                };
                for interface_key in interface_keys {
                    for resolved_interface_key in export_key_with_origins(graph, interface_key) {
                        if seen_pairs
                            .insert((resolved_interface_key.clone(), implementer_key.clone()))
                        {
                            interface_to_implementers
                                .entry(resolved_interface_key)
                                .or_default()
                                .push(implementer_key.clone());
                        }
                    }
                }
            }
        }
    }
    interface_to_implementers
}
