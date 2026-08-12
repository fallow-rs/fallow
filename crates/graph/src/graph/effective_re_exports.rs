//! Effective outward re-export routes for one canonical binding.

use std::collections::VecDeque;

use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{EffectiveExportResolution, ExportNamespace, ModuleGraph};

/// One module/name pair that effectively exposes a traced binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveReExportRoute {
    barrel_file: FileId,
    exported_name: String,
}

impl EffectiveReExportRoute {
    /// Module exposing the binding at this step.
    #[must_use]
    pub const fn barrel_file(&self) -> FileId {
        self.barrel_file
    }

    /// Name exposed by this module.
    #[must_use]
    pub fn exported_name(&self) -> &str {
        &self.exported_name
    }
}

impl ModuleGraph {
    /// Every effective outward route from one exported binding.
    ///
    /// Routes carry aliases across named re-exports, omit ambiguous and
    /// shadowed paths, deduplicate convergent diamonds, and terminate on cycles.
    #[must_use]
    pub fn effective_re_export_routes(
        &self,
        source_file: FileId,
        source_name: &str,
        namespace: ExportNamespace,
    ) -> Vec<EffectiveReExportRoute> {
        let EffectiveExportResolution::Unique(source_binding) =
            self.resolve_export(source_file, source_name, namespace)
        else {
            return Vec::new();
        };

        let mut re_exports_by_source: FxHashMap<FileId, Vec<(FileId, usize)>> =
            FxHashMap::default();
        for module in &self.modules {
            for (index, re_export) in module.re_exports.iter().enumerate() {
                re_exports_by_source
                    .entry(re_export.source_file)
                    .or_default()
                    .push((module.file_id, index));
            }
        }

        let initial = (source_file, source_name.to_string());
        let mut visited = FxHashSet::from_iter([initial.clone()]);
        let mut queue = VecDeque::from([initial]);
        let mut routes = Vec::new();

        while let Some((current_file, current_name)) = queue.pop_front() {
            let Some(re_exports) = re_exports_by_source.get(&current_file) else {
                continue;
            };
            for &(barrel_file, re_export_index) in re_exports {
                let re_export = &self.modules[barrel_file.0 as usize].re_exports[re_export_index];
                let Some(exported_name) =
                    effective_destination_name(re_export, &current_name, namespace)
                else {
                    continue;
                };
                if self.resolve_export(barrel_file, exported_name, namespace)
                    != EffectiveExportResolution::Unique(source_binding)
                {
                    continue;
                }

                let destination = (barrel_file, exported_name.to_string());
                if !visited.insert(destination.clone()) {
                    continue;
                }
                routes.push(EffectiveReExportRoute {
                    barrel_file,
                    exported_name: destination.1.clone(),
                });
                queue.push_back(destination);
            }
        }

        routes
    }
}

fn effective_destination_name<'a>(
    re_export: &'a super::ReExportEdge,
    source_name: &'a str,
    namespace: ExportNamespace,
) -> Option<&'a str> {
    if namespace == ExportNamespace::Value && re_export.is_type_only {
        return None;
    }
    if re_export.exported_name == "*" {
        return (source_name != "default").then_some(source_name);
    }
    if re_export.imported_name == "*" || re_export.imported_name != source_name {
        return None;
    }
    Some(&re_export.exported_name)
}
