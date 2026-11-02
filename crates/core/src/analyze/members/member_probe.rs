//! Credit for class members that a runtime probe checks by name.
//!
//! `"detail" in cause` and `Reflect.has(cause, "detail")` check an untyped
//! value for a member with a static name. A class can supply that member only
//! so that the probe finds it, and no static access names the class.
//!
//! The credit is scoped to the probe module and its direct import neighbors:
//! the modules that the probe module imports, and the modules that import it.
//! A class in such a module has a direct link to the probe code, for example
//! a class that implements the contract of a helper which it imports. Probes
//! often check common names (`type`, `code`, `id`), so a project-wide credit
//! by name would hide unused members in unrelated classes.

use fallow_types::extract::SemanticFact;

use super::*;

/// Credit class members whose name a reachable probe module, or one of its
/// direct import neighbors, checks with a member name probe.
pub(super) fn propagate_member_name_probe_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let probes_by_file = collect_reachable_probes(graph, resolved_modules);
    if probes_by_file.is_empty() {
        return;
    }
    let credited_by_file = neighbor_probe_names(resolved_modules, &probes_by_file);
    for module in &graph.modules {
        let Some(names) = credited_by_file.get(&module.file_id) else {
            continue;
        };
        for export in &module.exports {
            for member in &export.members {
                if matches!(
                    member.kind,
                    MemberKind::ClassMethod | MemberKind::ClassProperty
                ) && names.contains(member.name.as_str())
                {
                    accessed_members
                        .entry(ExportKey::new(module.file_id, export.name.to_string()))
                        .or_default()
                        .insert(member.name.clone());
                }
            }
        }
    }
}

/// Probed member names per reachable module. A probe in an unreachable
/// module is dead code and gives no credit.
fn collect_reachable_probes<'a>(
    graph: &ModuleGraph,
    resolved_modules: &'a [ResolvedModule],
) -> FxHashMap<FileId, FxHashSet<&'a str>> {
    let mut probes_by_file: FxHashMap<FileId, FxHashSet<&'a str>> = FxHashMap::default();
    for module in resolved_modules {
        let reachable = graph
            .modules
            .get(module.file_id.0 as usize)
            .is_some_and(crate::graph::ModuleNode::is_reachable);
        if !reachable {
            continue;
        }
        for fact in module.semantic_facts.iter() {
            if let SemanticFact::MemberNameProbe(probe) = fact {
                probes_by_file
                    .entry(module.file_id)
                    .or_default()
                    .insert(probe.name.as_str());
            }
        }
    }
    probes_by_file
}

/// The probed names that each module receives: its own probes, the probes of
/// each module that it imports, and the probes of each module that imports it.
/// A type-only import carries no runtime value, so it is not a neighbor link.
fn neighbor_probe_names<'a>(
    resolved_modules: &[ResolvedModule],
    probes_by_file: &FxHashMap<FileId, FxHashSet<&'a str>>,
) -> FxHashMap<FileId, FxHashSet<&'a str>> {
    let mut credited: FxHashMap<FileId, FxHashSet<&'a str>> = probes_by_file.clone();
    for module in resolved_modules {
        let own_probes = probes_by_file.get(&module.file_id);
        let targets = module
            .resolved_imports
            .iter()
            .chain(module.resolved_dynamic_imports.iter())
            .filter(|import| !import.info.is_type_only)
            .filter_map(|import| import.target.internal_file_id())
            .filter(|target| *target != module.file_id);
        for target in targets {
            if let Some(target_probes) = probes_by_file.get(&target) {
                credited
                    .entry(module.file_id)
                    .or_default()
                    .extend(target_probes.iter().copied());
            }
            if let Some(own) = own_probes {
                credited
                    .entry(target)
                    .or_default()
                    .extend(own.iter().copied());
            }
        }
    }
    credited
}
