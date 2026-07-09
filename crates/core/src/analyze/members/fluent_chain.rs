use super::*;

/// Validate a fluent chain against a single class export.
pub(super) fn export_validates_fluent_chain(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    root_method: &str,
    chain: &[&str],
) -> bool {
    if !export.name.matches_str(origin.export_name.as_str()) {
        return false;
    }
    let has_factory = export.members.iter().any(|member| {
        member.is_instance_returning_static
            && member.kind == MemberKind::ClassMethod
            && member.name == root_method
    });
    if !has_factory {
        return false;
    }
    chain.iter().all(|step| {
        export.members.iter().any(|member| {
            member.kind == MemberKind::ClassMethod
                && member.name == *step
                && member.is_self_returning
        })
    })
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
                    let chain = access.chain.iter().map(String::as_str).collect::<Vec<_>>();
                    let chain_valid = origin_module.exports.iter().any(|export| {
                        export_validates_fluent_chain(
                            export,
                            &origin,
                            access.root_method.as_str(),
                            &chain,
                        )
                    });
                    if !chain_valid {
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

/// Validate a constructor-rooted fluent chain against a single class export.
pub(super) fn export_validates_fluent_chain_new(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    chain: &[&str],
) -> bool {
    if !export.name.matches_str(origin.export_name.as_str()) {
        return false;
    }
    chain.iter().all(|step| {
        export.members.iter().any(|member| {
            member.kind == MemberKind::ClassMethod
                && member.name == *step
                && member.is_self_returning
        })
    })
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
                    let chain = access.chain.iter().map(String::as_str).collect::<Vec<_>>();
                    let chain_valid = origin_module
                        .exports
                        .iter()
                        .any(|export| export_validates_fluent_chain_new(export, &origin, &chain));
                    if !chain_valid {
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
