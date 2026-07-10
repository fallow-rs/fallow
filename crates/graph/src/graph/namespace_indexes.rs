//! Ephemeral indexes shared by namespace propagation passes.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;
use fallow_types::extract::ImportedName;

use crate::resolve::{ResolvedImport, ResolvedModule};

use super::ModuleGraph;

#[derive(Default)]
struct ReExportTargets {
    named: FxHashMap<String, Vec<(FileId, String)>>,
    star_barrels: Vec<FileId>,
}

/// One resolved consumer import matching a reachable re-export name.
pub(super) struct ConsumerImport<'a> {
    pub(super) consumer: &'a ResolvedModule,
    pub(super) import: &'a ResolvedImport,
}

/// Build-only indexes used by both namespace propagation passes.
pub(super) struct NamespacePropagationIndexes<'a> {
    re_exports_by_source: FxHashMap<FileId, ReExportTargets>,
    consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>>,
}

impl<'a> NamespacePropagationIndexes<'a> {
    pub(super) fn new(
        graph: &ModuleGraph,
        module_by_id: &FxHashMap<FileId, &'a ResolvedModule>,
    ) -> Self {
        let mut re_exports_by_source: FxHashMap<FileId, ReExportTargets> = FxHashMap::default();
        for module in &graph.modules {
            for edge in &module.re_exports {
                let targets = re_exports_by_source.entry(edge.source_file).or_default();
                if edge.imported_name == "*" && edge.exported_name == "*" {
                    targets.star_barrels.push(module.file_id);
                } else if edge.imported_name != "*" {
                    targets
                        .named
                        .entry(edge.imported_name.clone())
                        .or_default()
                        .push((module.file_id, edge.exported_name.clone()));
                }
            }
        }

        let mut consumers_by_target: FxHashMap<FileId, FxHashMap<String, Vec<ConsumerImport<'a>>>> =
            FxHashMap::default();
        for consumer in module_by_id.values() {
            for import in &consumer.resolved_imports {
                let Some(target) = import.target.internal_file_id() else {
                    continue;
                };
                let imported_name = match &import.info.imported_name {
                    ImportedName::Named(name) => name.as_str(),
                    ImportedName::Default => "default",
                    _ => continue,
                };
                consumers_by_target
                    .entry(target)
                    .or_default()
                    .entry(imported_name.to_string())
                    .or_default()
                    .push(ConsumerImport { consumer, import });
            }
        }

        Self {
            re_exports_by_source,
            consumers_by_target,
        }
    }

    pub(super) fn enumerate_reachable_barrels(
        &self,
        seed_file: FileId,
        seed_name: &str,
    ) -> FxHashSet<(FileId, String)> {
        let mut reachable = FxHashSet::default();
        reachable.insert((seed_file, seed_name.to_string()));
        let mut frontier = vec![(seed_file, seed_name.to_string())];

        while let Some((source_file, source_name)) = frontier.pop() {
            let Some(targets) = self.re_exports_by_source.get(&source_file) else {
                continue;
            };
            if let Some(named) = targets.named.get(source_name.as_str()) {
                for pair in named {
                    if reachable.insert(pair.clone()) {
                        frontier.push(pair.clone());
                    }
                }
            }
            for &barrel_file in &targets.star_barrels {
                let pair = (barrel_file, source_name.clone());
                if reachable.insert(pair.clone()) {
                    frontier.push(pair);
                }
            }
        }

        reachable
    }

    pub(super) fn consumers_for(
        &self,
        target: FileId,
        imported_name: &str,
    ) -> &[ConsumerImport<'a>] {
        self.consumers_by_target
            .get(&target)
            .and_then(|by_name| by_name.get(imported_name))
            .map_or(&[], Vec::as_slice)
    }
}
