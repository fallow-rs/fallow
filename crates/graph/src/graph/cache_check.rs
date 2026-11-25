//! Bounds checks of a module graph that a cache file supplies.

use fallow_types::discover::FileId;

use super::ModuleGraph;
use super::types::ReferencePathNode;

impl ModuleGraph {
    /// True when every `FileId`, edge range and reference path id of the
    /// graph is in bounds. A graph that a cache file supplies can break these
    /// invariants, and the detectors index with them without a check.
    #[must_use]
    pub fn has_valid_structure(&self) -> bool {
        let module_count = self.modules.len();
        let in_range = |id: &FileId| (id.0 as usize) < module_count;
        let path_count = self.reference_paths.len();
        let path_in_range = |index: usize| index < path_count;
        let modules_valid = self.modules.iter().enumerate().all(|(index, module)| {
            module.file_id.0 as usize == index
                && module.edge_range.start <= module.edge_range.end
                && module.edge_range.end <= self.edges.len()
                && module
                    .re_exports
                    .iter()
                    .all(|edge| in_range(&edge.source_file))
                && module.exports.iter().all(|export| {
                    export
                        .references
                        .iter()
                        .all(|reference| in_range(&reference.from_file))
                        && export
                            .reference_paths
                            .iter()
                            .flatten()
                            .all(|path| path_in_range(path.index()))
                })
        });
        let paths_valid = self.reference_paths.iter().all(|node| {
            node.parent()
                .is_none_or(|parent| path_in_range(parent.index()))
                && match node {
                    ReferencePathNode::Hop { target, .. } => in_range(target),
                    ReferencePathNode::Route { graph, .. } => {
                        (graph.0 as usize) < self.reference_routes.graphs.len()
                    }
                }
        });
        let usage_valid = [
            &self.package_usage,
            &self.type_only_package_usage,
            &self.asset_package_usage,
        ]
        .into_iter()
        .flat_map(|usage| usage.values().flatten())
        .all(in_range);
        modules_valid
            && paths_valid
            && usage_valid
            && self.eager_package_imports.keys().all(in_range)
            && self
                .edges
                .iter()
                .all(|edge| in_range(&edge.source) && in_range(&edge.target))
            && self.reverse_deps.len() <= module_count
            && self.reverse_deps.iter().flatten().all(in_range)
            && self
                .entry_points
                .iter()
                .chain(&self.runtime_entry_points)
                .chain(&self.test_entry_points)
                .chain(&self.support_entry_points)
                .all(in_range)
    }
}
