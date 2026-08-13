//! Effective outward re-export routes for one canonical binding.

use std::collections::VecDeque;

use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleLoadMechanism;

use super::effective_exports::{EffectiveExportBinding, EffectiveExportIndex};
use super::types::{
    ModuleNode, ReferencePathId, ReferencePathInterner, ReferenceRouteGraphId,
    ReferenceRouteGraphSpec, ReferenceRouteNodeId, ReferenceRouteNodeSpec,
};
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

pub(in crate::graph) struct EffectiveDeclarationRoute {
    pub(in crate::graph) binding: EffectiveExportBinding,
    graph: ReferenceRouteGraphSpec,
    start: ReferenceRouteNodeId,
    terminal: ReferenceRouteNodeId,
}

impl EffectiveDeclarationRoute {
    pub(in crate::graph) fn intern(
        self,
        reference_paths: &mut ReferencePathInterner,
    ) -> InternedEffectiveDeclarationRoute {
        let graph = reference_paths
            .tracks_provenance()
            .then(|| reference_paths.intern_route_graph(self.graph));
        InternedEffectiveDeclarationRoute {
            graph,
            start: self.start,
            terminal: self.terminal,
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::graph) struct InternedEffectiveDeclarationRoute {
    graph: Option<ReferenceRouteGraphId>,
    start: ReferenceRouteNodeId,
    terminal: ReferenceRouteNodeId,
}

impl InternedEffectiveDeclarationRoute {
    pub(in crate::graph) fn extend_path(
        &self,
        parent: Option<ReferencePathId>,
        reference_paths: &mut ReferencePathInterner,
    ) -> Option<ReferencePathId> {
        let graph = self.graph?;
        reference_paths.route(
            parent,
            graph,
            self.start,
            self.terminal,
            Some(ModuleLoadMechanism::EsModule),
        )
    }
}

pub(in crate::graph) fn effective_declaration_route(
    modules: &[ModuleNode],
    index: &EffectiveExportIndex,
    file: FileId,
    name: &str,
    namespace: ExportNamespace,
) -> Option<EffectiveDeclarationRoute> {
    let EffectiveExportResolution::Unique(binding) = index.resolve(file, name, namespace) else {
        return None;
    };
    binding.origin_slot()?;

    // States borrow their names from the module re-export edges, so the search
    // walks the topology without allocating one string per visited hop.
    let initial: (FileId, &str) = (file, name);
    let mut states = vec![initial];
    let mut state_ids: FxHashMap<(FileId, &str), usize> = FxHashMap::from_iter([(initial, 0)]);
    let mut successors: Vec<Vec<usize>> = vec![Vec::new()];
    let mut frontier = VecDeque::from([0_usize]);
    let mut terminal = None;

    while let Some(state_id) = frontier.pop_front() {
        let (current_file, current_name) = states[state_id];
        if current_file == binding.origin_file() {
            terminal = Some(state_id);
            continue;
        }
        let module = modules.get(current_file.0 as usize)?;
        for edge in &module.re_exports {
            let Some(source_name) = effective_source_name(edge, current_name, namespace) else {
                continue;
            };
            if index.resolve(edge.source_file, source_name, namespace)
                != EffectiveExportResolution::Unique(binding)
            {
                continue;
            }
            let state = (edge.source_file, source_name);
            let next_id = if let Some(next_id) = state_ids.get(&state) {
                *next_id
            } else {
                let next_id = states.len();
                states.push(state);
                state_ids.insert(state, next_id);
                successors.push(Vec::new());
                frontier.push_back(next_id);
                next_id
            };
            successors[state_id].push(next_id);
        }
    }

    let terminal = terminal?;
    for next in &mut successors {
        next.sort_unstable();
        next.dedup();
    }
    let nodes = states
        .iter()
        .zip(successors)
        .map(|((target, _), successors)| {
            ReferenceRouteNodeSpec::new(
                *target,
                ModuleLoadMechanism::EsModule,
                successors
                    .into_iter()
                    .map(|id| ReferenceRouteNodeId(id as u32))
                    .collect(),
            )
        })
        .collect();
    Some(EffectiveDeclarationRoute {
        binding,
        graph: ReferenceRouteGraphSpec::new(nodes),
        start: ReferenceRouteNodeId(0),
        terminal: ReferenceRouteNodeId(terminal as u32),
    })
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

fn effective_source_name<'a>(
    re_export: &'a super::ReExportEdge,
    exported_name: &'a str,
    namespace: ExportNamespace,
) -> Option<&'a str> {
    if namespace == ExportNamespace::Value && re_export.is_type_only {
        return None;
    }
    if re_export.exported_name == "*" {
        return (exported_name != "default").then_some(exported_name);
    }
    if re_export.exported_name != exported_name || re_export.imported_name == "*" {
        return None;
    }
    Some(&re_export.imported_name)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::extract::{ExportInfo, ExportName, ReExportInfo, VisibilityTag};
    use oxc_span::Span;

    use super::*;
    use crate::graph::ReExportEdge;
    use crate::graph::types::{ExportSymbol, ReferencePathNode};
    use crate::resolve::{ResolveResult, ResolvedModule, ResolvedReExport};

    fn source_export() -> ExportInfo {
        ExportInfo {
            name: ExportName::Named("foo".to_string()),
            local_name: Some("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(0, 3),
            members: Vec::new(),
            super_class: None,
        }
    }

    fn re_export(target: FileId, imported_name: &str, exported_name: &str) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: "./source".to_string(),
                imported_name: imported_name.to_string(),
                exported_name: exported_name.to_string(),
                is_type_only: false,
                span: Span::default(),
                statement_span: Span::default(),
                source_span: Span::default(),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn re_export_edge(
        source_file: FileId,
        imported_name: &str,
        exported_name: &str,
    ) -> ReExportEdge {
        ReExportEdge {
            source_file,
            imported_name: imported_name.to_string(),
            exported_name: exported_name.to_string(),
            is_type_only: false,
            span: Span::default(),
        }
    }

    fn module(
        file_id: FileId,
        path: &str,
        exports: Vec<ExportSymbol>,
        re_exports: Vec<ReExportEdge>,
    ) -> ModuleNode {
        ModuleNode {
            file_id,
            path: PathBuf::from(path),
            edge_range: 0..0,
            exports,
            re_exports,
            flags: 0,
        }
    }

    fn source_symbol() -> ExportSymbol {
        ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(0, 3),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        }
    }

    #[test]
    fn declaration_route_retains_star_surface_and_origin_hops() {
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                re_exports: vec![re_export(FileId(1), "*", "*")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                exports: vec![source_export()].into(),
                ..Default::default()
            },
        ];
        let mut modules = vec![
            module(
                FileId(0),
                "/project/inner.ts",
                Vec::new(),
                vec![re_export_edge(FileId(1), "*", "*")],
            ),
            module(
                FileId(1),
                "/project/source.ts",
                vec![source_symbol()],
                Vec::new(),
            ),
        ];
        let index = EffectiveExportIndex::build(&resolved);

        let route =
            effective_declaration_route(&modules, &index, FileId(0), "foo", ExportNamespace::Value)
                .expect("the star surface must retain a route to its declaration");

        assert_eq!(route.binding.origin_file(), FileId(1));
        let mut paths = ReferencePathInterner::new(true);
        let route = route.intern(&mut paths);
        let path = route
            .extend_path(None, &mut paths)
            .expect("tracked routes return one interned path");
        let finalized = paths.finalize(&mut modules);
        let ReferencePathNode::Route {
            graph,
            start,
            terminal,
            start_mechanism,
            ..
        } = finalized.paths[path.index()]
        else {
            panic!("effective declaration routes use the compact route contract");
        };
        assert_eq!(
            finalized
                .routes
                .canonical_hops(graph, start, terminal, start_mechanism),
            vec![
                (FileId(1), ModuleLoadMechanism::EsModule),
                (FileId(0), ModuleLoadMechanism::EsModule),
            ]
        );
    }

    #[test]
    fn declaration_route_retains_rename_star_and_origin_hops() {
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                re_exports: vec![re_export(FileId(1), "foo", "alias")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                re_exports: vec![re_export(FileId(2), "*", "*")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                exports: vec![source_export()].into(),
                ..Default::default()
            },
        ];
        let mut modules = vec![
            module(
                FileId(0),
                "/project/rename.ts",
                Vec::new(),
                vec![re_export_edge(FileId(1), "foo", "alias")],
            ),
            module(
                FileId(1),
                "/project/inner.ts",
                Vec::new(),
                vec![re_export_edge(FileId(2), "*", "*")],
            ),
            module(
                FileId(2),
                "/project/source.ts",
                vec![source_symbol()],
                Vec::new(),
            ),
        ];
        let index = EffectiveExportIndex::build(&resolved);
        let route = effective_declaration_route(
            &modules,
            &index,
            FileId(0),
            "alias",
            ExportNamespace::Value,
        )
        .expect("the renamed star surface must retain its declaration route");

        assert_eq!(route.binding.origin_file(), FileId(2));
        let mut paths = ReferencePathInterner::new(true);
        let route = route.intern(&mut paths);
        let path = route
            .extend_path(None, &mut paths)
            .expect("tracked routes return one interned path");
        let finalized = paths.finalize(&mut modules);
        let ReferencePathNode::Route {
            graph,
            start,
            terminal,
            start_mechanism,
            ..
        } = finalized.paths[path.index()]
        else {
            panic!("effective declaration routes use the compact route contract");
        };
        assert_eq!(
            finalized
                .routes
                .canonical_hops(graph, start, terminal, start_mechanism),
            vec![
                (FileId(2), ModuleLoadMechanism::EsModule),
                (FileId(1), ModuleLoadMechanism::EsModule),
                (FileId(0), ModuleLoadMechanism::EsModule),
            ]
        );
    }
}
