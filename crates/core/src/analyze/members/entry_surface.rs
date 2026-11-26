//! The member surface of entry files whose consumers are outside the project.

use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph, ModuleNode};

use super::ExportKey;

/// Whether the consumers of `module` are outside the project, so the members
/// of what it exports are public API: the module is an entry point that is
/// not consumer-checked, and `includeEntryExports` does not cover it.
pub(super) fn exposes_entry_members(
    module: &ModuleNode,
    entry_exports_config: Option<&fallow_config::ResolvedConfig>,
) -> bool {
    module.is_entry_point() && !requires_project_consumers(module, entry_exports_config)
}

/// Whether the exports of `module` need a consumer in the project: the module
/// is a consumer-checked entry point (`includeEntryExports`, or a plugin
/// fixture that a module imports by name), or `includeEntryExports` covers
/// its path.
fn requires_project_consumers(
    module: &ModuleNode,
    entry_exports_config: Option<&fallow_config::ResolvedConfig>,
) -> bool {
    module.is_consumer_checked()
        || entry_exports_config.is_some_and(|config| config.include_entry_exports_for(&module.path))
}

/// The public package entries whose consumers are outside the project:
/// `public_api_entry_points` without each file whose exports need a consumer
/// in the project. `None` means that no file is removed, so the caller can
/// reuse what it computed for the full set.
pub(super) fn public_member_entries(
    graph: &ModuleGraph,
    public_api_entry_points: &FxHashSet<FileId>,
    entry_exports_config: Option<&fallow_config::ResolvedConfig>,
) -> Option<FxHashSet<FileId>> {
    let removes = |file_id: &FileId| {
        graph
            .modules()
            .get(file_id.0 as usize)
            .is_some_and(|module| requires_project_consumers(module, entry_exports_config))
    };
    if !public_api_entry_points.iter().any(removes) {
        return None;
    }
    Some(
        public_api_entry_points
            .iter()
            .copied()
            .filter(|file_id| !removes(file_id))
            .collect(),
    )
}

/// The declarations that the `entries` publish in the value or the type
/// namespace, also through a re-export (`export *`, `export { E } from`,
/// `export * as ns`, or more barrels).
pub(super) fn public_declaration_keys(
    graph: &ModuleGraph,
    entries: &FxHashSet<FileId>,
) -> FxHashSet<ExportKey> {
    [ExportNamespace::Value, ExportNamespace::Type]
        .into_iter()
        .flat_map(|namespace| graph.public_export_origins_in_namespace(entries, namespace))
        .map(|origin| ExportKey::new(origin.file_id(), origin.export_name()))
        .collect()
}
