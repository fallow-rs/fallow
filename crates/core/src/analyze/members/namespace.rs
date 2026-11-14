//! Unused members of exported TypeScript namespaces: the credit of the
//! namespace member uses of the semantic pass, and the report filters.

use super::*;
use fallow_types::extract::NamespaceMemberUseFact;

/// Drop a member of a nested namespace that is reported itself: removing
/// `Inner` removes `Inner.x` too, so one finding is enough.
pub(super) fn drop_members_of_unused_namespaces(members: Vec<UnusedMember>) -> Vec<UnusedMember> {
    let reported: FxHashSet<(&Path, &str, &str)> = members
        .iter()
        .map(|m| {
            (
                m.path.as_path(),
                m.parent_name.as_str(),
                m.member_name.as_str(),
            )
        })
        .collect();
    let keep: Vec<bool> = members
        .iter()
        .map(|m| {
            !m.member_name.match_indices('.').any(|(index, _)| {
                reported.contains(&(
                    m.path.as_path(),
                    m.parent_name.as_str(),
                    &m.member_name[..index],
                ))
            })
        })
        .collect();
    members
        .into_iter()
        .zip(keep)
        .filter_map(|(member, keep)| keep.then_some(member))
        .collect()
}

/// Whether the dotted namespace member `name` (`Inner.x`) counts as used: the
/// member itself, or a whole use of the namespace (`*`) or of a namespace
/// above the member (`Inner.*`).
pub(super) fn namespace_member_credited(accessed: &FxHashSet<String>, name: &str) -> bool {
    if accessed.contains(name) || accessed.contains(NAMESPACE_WHOLE_USE) {
        return true;
    }
    name.match_indices('.')
        .map(|(index, _)| &name[..index])
        .chain(std::iter::once(name))
        .any(|prefix| accessed.contains(&format!("{prefix}.{NAMESPACE_WHOLE_USE}")))
}

/// The marker that `accessed_members` holds for a whole use of a namespace,
/// or (as `Inner.*`) of a nested namespace. No member has this name.
const NAMESPACE_WHOLE_USE: &str = "*";

/// Credit the namespace member uses of the semantic pass: a nested chain, a
/// whole use, and a reference inside the namespace body. Also mark the
/// escape of a whole enum.
pub(super) fn credit_namespace_member_uses(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    for fact in resolved.semantic_facts.iter() {
        let fallow_types::extract::SemanticFact::NamespaceMemberUse(namespace_use) = fact else {
            continue;
        };
        let namespace_files: Vec<FileId> =
            namespace_import_targets(resolved, &namespace_use.object)
                .chain(import_type_targets(resolved, &namespace_use.object))
                .collect();
        let targets: Vec<(ExportKey, &str)> = if namespace_files.is_empty() {
            local_to_export_keys
                .get(namespace_use.object.as_str())
                .into_iter()
                .flatten()
                .map(|key| (key.clone(), namespace_use.path.as_str()))
                .collect()
        } else {
            // `import * as ns`: the first segment names the export.
            let (export_name, rest) = namespace_use
                .path
                .split_once('.')
                .unwrap_or((namespace_use.path.as_str(), ""));
            if export_name.is_empty() {
                // The namespace import itself escapes: every export of the
                // target module can be read.
                if namespace_use.whole {
                    credit_whole_module_namespaces(graph, &namespace_files, accessed_members);
                }
                continue;
            }
            namespace_files
                .iter()
                .map(|file_id| (ExportKey::new(*file_id, export_name.to_string()), rest))
                .collect()
        };
        for (key, path) in targets {
            let credited = accessed_members.entry(key).or_default();
            credit_namespace_path(credited, path, namespace_use.whole);
            if namespace_use.whole && path.is_empty() && !namespace_use.class_field {
                credited.insert(ENUM_ESCAPE_USE.to_string());
            }
        }
    }
}

/// The marker that `accessed_members` holds when a whole enum leaves its
/// chain (`takesEnum(Shape)`, `{ ref: Size }`). Code can then read any
/// member, as with `Object.values(Shape)`. A class field alias
/// (`levels = Level`) does not set it: the visitor follows the reads of the
/// field, so only those members count as used. No member has this name.
const ENUM_ESCAPE_USE: &str = "(escape)";

/// Whether the enum member `name` counts as used.
pub(super) fn enum_member_credited(accessed: &FxHashSet<String>, name: &str) -> bool {
    accessed.contains(name) || accessed.contains(ENUM_ESCAPE_USE)
}

/// Project files that an inline import type `import("./x").N.a` names. The
/// semantic pass keys such a use by its source, not by a local binding.
fn import_type_targets<'r>(
    resolved: &'r ResolvedModule,
    object: &'r str,
) -> impl Iterator<Item = FileId> + 'r {
    let is_import_type_key = object.starts_with("import(");
    resolved
        .all_resolved_imports()
        .filter(move |import| {
            is_import_type_key
                && import.info.local_name.is_empty()
                && import.info.is_type_only
                && NamespaceMemberUseFact::import_type_object(&import.info.source) == object
        })
        .filter_map(|import| import.target.internal_file_id())
}

/// Credit every namespace member that an escaping namespace import can read:
/// the namespaces of the target modules, also behind their re-exports.
fn credit_whole_module_namespaces(
    graph: &ModuleGraph,
    files: &[FileId],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let mut pending = files.to_vec();
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    let mut credit = |key: ExportKey| {
        accessed_members
            .entry(key)
            .or_default()
            .insert(NAMESPACE_WHOLE_USE.to_string());
    };
    while let Some(file_id) = pending.pop() {
        if !visited.insert(file_id) {
            continue;
        }
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        for export in &module.exports {
            if export
                .members
                .iter()
                .any(|member| member.kind == MemberKind::NamespaceMember)
            {
                credit(ExportKey::new(file_id, export.name.to_string()));
            }
        }
        for edge in &module.re_exports {
            if edge.imported_name == "*" {
                pending.push(edge.source_file);
            } else {
                credit(ExportKey::new(edge.source_file, edge.imported_name.clone()));
            }
        }
    }
}

fn credit_namespace_path(credited: &mut FxHashSet<String>, path: &str, whole: bool) {
    for (index, _) in path.match_indices('.') {
        credited.insert(path[..index].to_string());
    }
    if !path.is_empty() {
        credited.insert(path.to_string());
    }
    if whole {
        credited.insert(if path.is_empty() {
            NAMESPACE_WHOLE_USE.to_string()
        } else {
            format!("{path}.{NAMESPACE_WHOLE_USE}")
        });
    }
}
