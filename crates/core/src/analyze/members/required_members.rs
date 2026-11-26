//! The members that a named structural type requires, including the members
//! that it inherits through `interface X extends Y` (issue #5299).

use super::*;

/// The most interfaces that one `extends` closure visits. A real interface
/// tree stays far below it; the limit only bounds a pathological input.
const MAX_EXTENDS_CLOSURE_NODES: usize = 256;

#[derive(Default)]
pub(super) struct RequiredTypeMemberIndex {
    by_file: FxHashMap<FileId, FxHashMap<String, FxHashSet<String>>>,
    extends_by_file: FxHashMap<FileId, FxHashMap<String, Vec<String>>>,
}

impl RequiredTypeMemberIndex {
    pub(super) fn insert_module(&mut self, module: &ModuleInfo) {
        let facts = SemanticFactView::new(&module.semantic_facts, &module.member_accesses);
        for required in facts.required_type_members() {
            self.by_file
                .entry(module.file_id)
                .or_default()
                .entry(required.type_name.clone())
                .or_default()
                .insert(required.member.clone());
        }
        for edge in facts.interface_extends() {
            self.extends_by_file
                .entry(module.file_id)
                .or_default()
                .entry(edge.interface_name.clone())
                .or_default()
                .push(edge.parent_name.clone());
        }
    }

    /// The required members of `type_name` in `file_id` and of each interface
    /// that it extends. A parent name resolves to the same module and, through
    /// the imports of the module, to the module that declares it.
    pub(super) fn required_closure(
        &self,
        graph: &ModuleGraph,
        indexes: &MemberPassIndexes<'_>,
        file_id: FileId,
        type_name: &str,
    ) -> FxHashSet<String> {
        let mut required = FxHashSet::default();
        let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
        let mut pending = vec![(file_id, type_name.to_string())];
        while let Some((file, name)) = pending.pop() {
            if visited.len() >= MAX_EXTENDS_CLOSURE_NODES || !visited.insert((file, name.clone())) {
                continue;
            }
            if let Some(members) = self.by_file.get(&file).and_then(|types| types.get(&name)) {
                required.extend(members.iter().cloned());
            }
            let Some(parents) = self
                .extends_by_file
                .get(&file)
                .and_then(|types| types.get(&name))
            else {
                continue;
            };
            for parent in parents {
                pending.push((file, parent.clone()));
                let Some(seeds) = indexes.local_keys(file).get(parent.as_str()) else {
                    continue;
                };
                for seed in seeds {
                    for origin in export_key_with_origins(graph, seed) {
                        if let Some(local) = declared_local_name(indexes, &origin) {
                            pending.push((origin.file_id, local));
                        }
                    }
                }
            }
        }
        required
    }

    /// [`Self::required_closure`] for the type that an export key names.
    pub(super) fn required_closure_for_export(
        &self,
        graph: &ModuleGraph,
        indexes: &MemberPassIndexes<'_>,
        key: &ExportKey,
    ) -> FxHashSet<String> {
        let Some(local) = declared_local_name(indexes, key) else {
            return FxHashSet::default();
        };
        self.required_closure(graph, indexes, key.file_id, &local)
    }
}

/// The local declaration name behind an export key. An export can rename its
/// declaration (`export { Foo as Bar }`), and the facts carry the local name.
fn declared_local_name(indexes: &MemberPassIndexes<'_>, key: &ExportKey) -> Option<String> {
    let module = indexes.module_by_id.get(&key.file_id)?;
    let local = module
        .exports
        .iter()
        .find(|export| export.name.matches_str(key.export_name.as_str()))
        .and_then(|export| export.local_name.as_deref())
        .unwrap_or(key.export_name.as_str());
    Some(local.to_string())
}
