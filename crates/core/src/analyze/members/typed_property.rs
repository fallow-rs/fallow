use super::*;

/// Resolve `name` (as seen from `module`) to the modules that DECLARE a
/// named-type property map for it: the module itself when it declares the
/// type, else every re-export-walked import origin whose `type_member_types`
/// carries the origin export name. A name that resolves to no declaring site
/// (a global, a class, a wrong annotation) contributes nothing.
fn typed_property_declaring_sites<'a>(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'a>,
    module: &'a ResolvedModule,
    name: &str,
) -> Vec<(&'a ResolvedModule, String)> {
    if module
        .type_member_types
        .iter()
        .any(|entry| entry.type_name == name)
    {
        return vec![(module, name.to_string())];
    }
    let Some(seed_keys) = indexes.local_keys(module.file_id).get(name) else {
        return Vec::new();
    };
    let mut sites = Vec::new();
    for seed in seed_keys {
        for origin in walk_re_export_origins(graph, seed.file_id, seed.export_name.as_str()) {
            let Some(origin_module) = indexes.module_by_id.get(&origin.file_id) else {
                continue;
            };
            // The origin export may be a same-file RENAME (`interface Foo
            // {...}; export { Foo as Bar }`): `origin.export_name` lives in
            // export-name space while `type_member_types.type_name` carries
            // the DECLARED local name, so resolve the export's local name
            // first (falling back to the export name when they coincide).
            let declared_name = origin_module
                .exports
                .iter()
                .find(|export| export.name.matches_str(origin.export_name.as_str()))
                .and_then(|export| export.local_name.as_deref())
                .unwrap_or(origin.export_name.as_str());
            if origin_module
                .type_member_types
                .iter()
                .any(|entry| entry.type_name == declared_name)
            {
                sites.push((*origin_module, declared_name.to_string()));
            }
        }
    }
    sites
}

/// Credit member accesses reached through a typed property hop whose named
/// type is not declared in the consumer file (`this.opts.c.optM()` where
/// `opts` is typed by an imported interface / alias). Mirrors
/// `propagate_factory_fn_accesses`'s chain-of-gates shape; a wrong resolution
/// at any link credits nothing (false-negative-preferring):
///
///   1. the fact's `type_name` resolves through the consumer's imports/exports
///      (re-export aware) to a module whose `type_member_types` declares the
///      type, the cross-module over-credit gate;
///   2. each `property_path` segment must be a named-reference-typed property
///      of the current type; a segment whose property type is itself imported
///      re-resolves through THAT declaring module's imports (depth bounded by
///      the segment count, each level deduped);
///   3. the terminal property type resolves through the last declaring
///      module's own imports/exports and must be a class with members
///      (`export_is_class_with_members`, reused via
///      `credit_factory_return_class_member`), OR (issue #1863) an INTERFACE
///      with implementers, in which case the member is credited on the
///      interface's export key so the later `propagate_interface_member_accesses`
///      pass carries it to every implementing class.
///
/// See issues #1785 and #1863.
pub(super) fn propagate_typed_property_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    interface_to_implementers: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    // Phase 1: walk every fact's property path to its terminal
    // (declaring module, terminal type local name, member) triples.
    let mut terminals: FxHashSet<(FileId, String, String)> = FxHashSet::default();
    for resolved in resolved_modules {
        for fact in typed_property_member_accesses(resolved) {
            let segments: Vec<&str> = fact
                .property_path
                .split('.')
                .filter(|segment| !segment.is_empty())
                .collect();
            if segments.is_empty() {
                continue;
            }
            let mut frontier: Vec<(&ResolvedModule, String)> =
                vec![(resolved, fact.type_name.clone())];
            for (idx, segment) in segments.iter().enumerate() {
                let mut next: Vec<(&ResolvedModule, String)> = Vec::new();
                let mut seen: FxHashSet<(FileId, String)> = FxHashSet::default();
                for (module, name) in frontier {
                    for (declaring, declared_name) in
                        typed_property_declaring_sites(graph, indexes, module, &name)
                    {
                        let Some(entry) = declaring.type_member_types.iter().find(|entry| {
                            entry.type_name == declared_name && entry.property == *segment
                        }) else {
                            continue;
                        };
                        let property_type = entry.property_type.clone();
                        if idx + 1 == segments.len() {
                            terminals.insert((
                                declaring.file_id,
                                property_type,
                                fact.member.clone(),
                            ));
                        } else if seen.insert((declaring.file_id, property_type.clone())) {
                            next.push((declaring, property_type));
                        }
                    }
                }
                frontier = next;
                if frontier.is_empty() {
                    break;
                }
            }
        }
    }

    // Phase 2: resolve each terminal type name through its declaring module's
    // own imports/exports to a class export and credit the member.
    let mut credit_context = FactoryReturnCreditContext {
        graph,
        indexes,
        accessed_members,
    };
    for (declaring_file_id, terminal_name, member) in terminals {
        if !credit_context
            .indexes
            .module_by_id
            .contains_key(&declaring_file_id)
        {
            continue;
        }
        credit_factory_return_class_member(
            &mut credit_context,
            declaring_file_id,
            terminal_name.as_str(),
            member.as_str(),
        );
        credit_typed_property_interface_terminal(
            &mut credit_context,
            interface_to_implementers,
            declaring_file_id,
            terminal_name.as_str(),
            member.as_str(),
        );
    }
}

/// Credit a typed-property-hop TERMINAL whose type is an INTERFACE (issue #1863).
///
/// `credit_factory_return_class_member` above resolves only class-with-members
/// terminals; an interface terminal (`deps.greeter: GreeterPort` where
/// `GreeterPort` is an interface) is filtered out by `export_is_class_with_members`
/// and would credit nothing. Resolving the interface to its canonical export key
/// (the same `local_keys` + `export_key_with_origins` resolution the
/// `interface_to_implementers` map is keyed by) and inserting the member there
/// makes the property-hop terminal behave exactly like the direct-parameter case:
/// the interface access is carried to every implementing class by the later
/// `propagate_interface_member_accesses` pass, which runs after this one (both
/// sit inside `collect_propagated_member_accesses`, the typed-property pass in
/// `propagate_common_member_accesses` first, then the interface pass). Purely
/// additive; a terminal that is not an interface with implementers credits
/// nothing, so it can only suppress a false positive, never create one.
fn credit_typed_property_interface_terminal(
    context: &mut FactoryReturnCreditContext<'_, '_>,
    interface_to_implementers: &FxHashMap<ExportKey, Vec<ExportKey>>,
    declaring_file_id: FileId,
    terminal_name: &str,
    member: &str,
) {
    let graph = context.graph;
    let indexes = context.indexes;
    let Some(seed_keys) = indexes.local_keys(declaring_file_id).get(terminal_name) else {
        return;
    };
    for seed_key in seed_keys {
        for interface_origin in export_key_with_origins(graph, seed_key) {
            if interface_to_implementers.contains_key(&interface_origin) {
                context
                    .accessed_members
                    .entry(interface_origin)
                    .or_default()
                    .insert(member.to_string());
            }
        }
    }
}
