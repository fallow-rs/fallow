//! Marks on entry modules that depend on how a file is loaded: the files that
//! the graph cannot follow member reads into, and the fixture entry points.

use rustc_hash::FxHashSet;

use crate::resolve::ResolvedModule;
use fallow_types::discover::FileId;

use super::ModuleGraph;

/// The files that a dynamic import, an import pattern or a `require()` loads.
#[must_use]
pub fn untracked_load_targets(resolved_modules: &[ResolvedModule]) -> FxHashSet<FileId> {
    let mut targets = FxHashSet::default();
    for module in resolved_modules {
        let required = module
            .resolved_imports
            .iter()
            .filter(|import| import.target.is_commonjs_require());
        for import in module.resolved_dynamic_imports.iter().chain(required) {
            if let Some(target) = import.target.internal_file_id() {
                targets.insert(target);
            }
        }
        for (_, files) in &module.resolved_dynamic_patterns {
            targets.extend(files.iter().copied());
        }
    }
    targets
}

impl ModuleGraph {
    /// Mark the consumer-checked modules in `file_ids` as fixture entry
    /// points. See [`ModuleNode::is_fixture_entry`](super::ModuleNode::is_fixture_entry).
    pub fn set_fixture_entries(&mut self, file_ids: &FxHashSet<FileId>) {
        for file_id in file_ids {
            if let Some(module) = self.modules.get_mut(file_id.0 as usize)
                && module.is_consumer_checked()
            {
                module.set_fixture_entry();
            }
        }
    }
}
