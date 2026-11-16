//! Reach of the support entry points, such as files that package.json scripts
//! run. The graph build does not mark this reach; a caller computes it on
//! demand.

use fallow_types::discover::EntryPoint;
use fixedbitset::FixedBitSet;
use rustc_hash::FxHashSet;

use super::ModuleGraph;

impl ModuleGraph {
    /// Record the support entry points of the graph. A path that is not a
    /// module of the graph adds nothing.
    pub fn set_support_entry_points(&mut self, entry_points: &[EntryPoint]) {
        let paths: FxHashSet<&std::path::Path> =
            entry_points.iter().map(|ep| ep.path.as_path()).collect();
        self.support_entry_points = self
            .modules
            .iter()
            .filter(|module| paths.contains(module.path.as_path()))
            .map(|module| module.file_id)
            .collect();
    }

    /// The modules that a support entry point reaches, the entry points
    /// included, as a bitset indexed by `FileId`.
    #[must_use]
    pub fn support_reachable(&self) -> FixedBitSet {
        self.collect_reachable(&self.support_entry_points, self.modules.len())
    }
}
