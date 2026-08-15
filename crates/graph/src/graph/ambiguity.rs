//! Star-export collisions and the declarations that participate in them.
//!
//! When two different `export *` sources supply the same name, ECMA-262
//! ResolveExport returns `ambiguous` and the barrel does not export the name at
//! all. The effective export index records that as
//! [`EffectiveExportResolution::Ambiguous`] and every consumer abstains, which
//! leaves the contributing declarations with no credit and no explanation. This
//! module turns the abstention into data: which barrel collides on which name,
//! and which declarations lost their credit to it.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;

use super::{EffectiveExportBinding, EffectiveExportResolution, ExportNamespace, ModuleGraph};

/// One `export *` collision on a barrel module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbiguousStarExport {
    /// The barrel whose star sources collide on this name.
    pub barrel: FileId,
    /// The colliding exported name.
    pub name: Box<str>,
    /// The namespace the collision occurs in.
    pub namespace: ExportNamespace,
    /// Files declaring a colliding declaration, sorted and deduplicated.
    pub contributors: Box<[FileId]>,
}

/// Declarations whose export credit is lost to a star-export collision.
///
/// A participant is neither used nor provably dead: the barrel that would carry
/// its credit exports nothing under that name. Detectors abstain from reporting
/// participants rather than attributing a barrel mistake to the source files.
/// Names identify the canonical defining declaration, so a re-export alias such
/// as `default as Widget` records `default` on the component file.
#[derive(Debug, Default)]
pub struct AmbiguityParticipants {
    names_by_file: FxHashMap<(FileId, ExportNamespace), FxHashSet<Box<str>>>,
}

impl AmbiguityParticipants {
    /// Whether this declaration lost its credit to a star-export collision in
    /// one specific namespace.
    #[must_use]
    pub fn contains_in_namespace(
        &self,
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> bool {
        self.names_by_file
            .get(&(file_id, namespace))
            .is_some_and(|names| names.contains(name))
    }

    /// Whether an exported declaration lost its credit in any namespace it
    /// occupies.
    ///
    /// A plain value declaration also occupies type space: `class` and `enum`
    /// carry a type meaning, and a `export type *` barrel collides on that
    /// meaning alone while the value namespace stays clean. Such a declaration
    /// has lost its credit just as thoroughly as a value-space participant.
    #[must_use]
    pub fn contains_declaration(&self, file_id: FileId, name: &str, is_type_only: bool) -> bool {
        self.contains_in_namespace(file_id, name, ExportNamespace::Type)
            || (!is_type_only && self.contains_in_namespace(file_id, name, ExportNamespace::Value))
    }
}

impl ModuleGraph {
    /// Every `export *` collision in the project, sorted deterministically.
    #[must_use]
    pub fn ambiguous_star_exports(&self) -> Vec<AmbiguousStarExport> {
        if !self.has_star_re_exports() {
            return Vec::new();
        }
        self.collect_ambiguous_star_exports(self.effective_exports.ambiguous_names())
    }

    /// `export *` collisions on one module, for single-symbol queries.
    #[must_use]
    pub fn ambiguous_star_exports_on(&self, file_id: FileId) -> Vec<AmbiguousStarExport> {
        if !self.has_star_re_exports() {
            return Vec::new();
        }
        self.collect_ambiguous_star_exports(self.effective_exports.ambiguous_names_on(file_id))
    }

    /// Declarations that lost export credit to a collision anywhere in the project.
    #[must_use]
    pub fn ambiguity_participants(&self) -> AmbiguityParticipants {
        if !self.has_star_re_exports() {
            return AmbiguityParticipants::default();
        }

        let mut participants = AmbiguityParticipants::default();
        for (barrel, name, namespace) in self.effective_exports.ambiguous_names() {
            for binding in self.star_contributor_bindings(barrel, name, namespace) {
                let declaration = self
                    .export_binding_origin(binding)
                    .map(|origin| {
                        (
                            origin.file_id(),
                            Box::<str>::from(origin.export().name.to_string()),
                        )
                    })
                    .or_else(|| {
                        binding
                            .is_implicit_default()
                            .then(|| (binding.origin_file(), Box::<str>::from("default")))
                    });
                let Some((file_id, declaration_name)) = declaration else {
                    continue;
                };
                participants
                    .names_by_file
                    .entry((file_id, namespace))
                    .or_default()
                    .insert(declaration_name);
            }
        }
        participants
    }

    fn collect_ambiguous_star_exports(
        &self,
        ambiguous: Vec<(FileId, &str, ExportNamespace)>,
    ) -> Vec<AmbiguousStarExport> {
        let mut collisions: Vec<AmbiguousStarExport> = ambiguous
            .into_iter()
            .filter_map(|(barrel, name, namespace)| {
                let contributors = self.star_contributors(barrel, name, namespace);
                (!contributors.is_empty()).then(|| AmbiguousStarExport {
                    barrel,
                    name: Box::from(name),
                    namespace,
                    contributors,
                })
            })
            .collect();
        collisions.sort_by(|left, right| {
            (left.barrel.0, &left.name, namespace_order(left.namespace)).cmp(&(
                right.barrel.0,
                &right.name,
                namespace_order(right.namespace),
            ))
        });
        collisions
    }

    fn has_star_re_exports(&self) -> bool {
        self.modules
            .iter()
            .any(|module| module.re_exports.iter().any(is_star_re_export))
    }

    /// Files declaring a colliding declaration reachable from `barrel`.
    ///
    /// A star source that is itself ambiguous forwards the collision, so the
    /// walk descends into it to reach the declarations that actually lost
    /// credit. Star graphs may contain cycles, hence the visited set.
    fn star_contributors(
        &self,
        barrel: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> Box<[FileId]> {
        let mut contributors: Vec<FileId> = self
            .star_contributor_bindings(barrel, name, namespace)
            .into_iter()
            .map(|binding| binding.origin_file())
            .collect();
        contributors.sort_unstable_by_key(|file_id| file_id.0);
        contributors.dedup();
        contributors.into_boxed_slice()
    }

    /// Canonical bindings supplied by the star sources for one ambiguous name.
    fn star_contributor_bindings(
        &self,
        barrel: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> FxHashSet<EffectiveExportBinding> {
        if name == "default" {
            return FxHashSet::default();
        }
        let mut contributors: FxHashSet<EffectiveExportBinding> = FxHashSet::default();
        let mut visited: FxHashSet<FileId> = FxHashSet::from_iter([barrel]);
        let mut pending = vec![barrel];
        while let Some(current) = pending.pop() {
            let Some(module) = self.modules.get(current.0 as usize) else {
                continue;
            };
            for re_export in module.re_exports.iter().filter(|re| is_star_re_export(re)) {
                if namespace == ExportNamespace::Value && re_export.is_type_only {
                    continue;
                }
                let source = re_export.source_file;
                if !visited.insert(source) {
                    continue;
                }
                match self.resolve_export(source, name, namespace) {
                    EffectiveExportResolution::Unique(binding) => {
                        contributors.insert(binding);
                    }
                    EffectiveExportResolution::Ambiguous => pending.push(source),
                    EffectiveExportResolution::Missing => {}
                }
            }
        }
        contributors
    }
}

fn is_star_re_export(re_export: &super::ReExportEdge) -> bool {
    re_export.imported_name == "*" && re_export.exported_name == "*"
}

const fn namespace_order(namespace: ExportNamespace) -> u8 {
    match namespace {
        ExportNamespace::Type => 0,
        ExportNamespace::Value => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::discover::DiscoveredFile;
    use fallow_types::extract::{ExportInfo, ExportName, ReExportInfo, VisibilityTag};

    use super::{AmbiguousStarExport, ExportNamespace, FileId, ModuleGraph};
    use crate::resolve::{ResolveResult, ResolvedModule, ResolvedReExport};

    fn value_export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn default_export() -> ExportInfo {
        ExportInfo {
            name: ExportName::Default,
            local_name: Some("Component".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn star_re_export(target: u32) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: format!("./module-{target}"),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(target)),
        }
    }

    fn type_only_star_re_export(target: u32) -> ResolvedReExport {
        let mut re_export = star_re_export(target);
        re_export.info.is_type_only = true;
        re_export
    }

    fn named_re_export(target: u32, imported_name: &str, exported_name: &str) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: format!("./module-{target}"),
                imported_name: imported_name.to_string(),
                exported_name: exported_name.to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(target)),
        }
    }

    /// Barrel 0 `export type *`s 1 and 2, which both export `class Foo`.
    ///
    /// The collision exists only in the type-fallback lane: the type-only star
    /// drops the value lane and neither source declares a real type.
    fn type_only_colliding_star_graph() -> ModuleGraph {
        let files: Vec<_> = (0..3)
            .map(|index| DiscoveredFile {
                id: FileId(index),
                path: PathBuf::from(format!("/project/module-{index}.ts")),
                size_bytes: 10,
            })
            .collect();
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                re_exports: vec![type_only_star_re_export(1), type_only_star_re_export(2)],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![value_export("Foo")].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![value_export("Foo")].into(),
                ..Default::default()
            },
        ];
        ModuleGraph::build(&modules, &[], &files)
    }

    /// Barrel 0 stars 1 and 2, both of which export `foo`.
    fn colliding_star_graph(extra_barrel: bool) -> ModuleGraph {
        let count = if extra_barrel { 4 } else { 3 };
        let files: Vec<_> = (0..count)
            .map(|index| DiscoveredFile {
                id: FileId(index),
                path: PathBuf::from(format!("/project/module-{index}.ts")),
                size_bytes: 10,
            })
            .collect();
        let mut modules: Vec<ResolvedModule> = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                re_exports: vec![star_re_export(1), star_re_export(2)],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![value_export("foo")].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![value_export("foo"), value_export("bar")].into(),
                ..Default::default()
            },
        ];
        if extra_barrel {
            modules.push(ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                re_exports: vec![star_re_export(0)],
                ..Default::default()
            });
        }
        ModuleGraph::build(&modules, &[], &files)
    }

    #[test]
    fn colliding_star_sources_are_reported_with_both_contributors() {
        let graph = colliding_star_graph(false);

        let collisions = graph.ambiguous_star_exports();

        assert_eq!(
            collisions,
            vec![AmbiguousStarExport {
                barrel: FileId(0),
                name: Box::from("foo"),
                namespace: ExportNamespace::Value,
                contributors: Box::from([FileId(1), FileId(2)]),
            }]
        );
    }

    #[test]
    fn colliding_type_only_star_sources_are_reported_in_the_type_namespace() {
        let graph = type_only_colliding_star_graph();

        assert_eq!(
            graph.ambiguous_star_exports(),
            vec![AmbiguousStarExport {
                barrel: FileId(0),
                name: Box::from("Foo"),
                namespace: ExportNamespace::Type,
                contributors: Box::from([FileId(1), FileId(2)]),
            }]
        );
    }

    #[test]
    fn a_value_declaration_behind_a_type_only_star_collision_is_a_participant() {
        let participants = type_only_colliding_star_graph().ambiguity_participants();

        assert!(participants.contains_declaration(FileId(1), "Foo", false));
        assert!(participants.contains_declaration(FileId(2), "Foo", false));
        assert!(!participants.contains_declaration(FileId(1), "Bar", false));
    }

    #[test]
    fn aliased_default_origins_are_participants_under_their_declaration_name() {
        let files: Vec<_> = (0..5)
            .map(|index| DiscoveredFile {
                id: FileId(index),
                path: PathBuf::from(format!("/project/module-{index}.ts")),
                size_bytes: 10,
            })
            .collect();
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                re_exports: vec![star_re_export(1), star_re_export(2)],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                re_exports: vec![named_re_export(3, "default", "Widget")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                re_exports: vec![named_re_export(4, "default", "Widget")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                exports: vec![default_export()].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(4),
                path: files[4].path.clone(),
                exports: vec![default_export()].into(),
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&modules, &[], &files);

        let participants = graph.ambiguity_participants();

        assert!(participants.contains_declaration(FileId(3), "default", false));
        assert!(participants.contains_declaration(FileId(4), "default", false));
        assert!(!participants.contains_declaration(FileId(1), "Widget", false));
        assert_eq!(
            graph.ambiguous_star_exports()[0].contributors.as_ref(),
            &[FileId(3), FileId(4)]
        );
    }

    /// A value star collision on a barrel that also takes part in the
    /// type-fallback lane, because a type-only re-export reads the barrel.
    ///
    /// Both the value lane and the fallback lane collide here, and the fallback
    /// collision is the synthesized shadow of the value one.
    #[test]
    fn a_value_star_collision_is_not_reported_twice_across_namespaces() {
        let files: Vec<_> = (0..4)
            .map(|index| DiscoveredFile {
                id: FileId(index),
                path: PathBuf::from(format!("/project/module-{index}.ts")),
                size_bytes: 10,
            })
            .collect();
        let type_only_named = ResolvedReExport {
            info: ReExportInfo {
                source: "./module-0".to_string(),
                imported_name: "foo".to_string(),
                exported_name: "foo".to_string(),
                is_type_only: true,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(FileId(0)),
        };
        let modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                re_exports: vec![star_re_export(1), star_re_export(2)],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![value_export("foo")].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![value_export("foo")].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                re_exports: vec![type_only_named],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&modules, &[], &files);

        let collisions = graph.ambiguous_star_exports();

        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert_eq!(collisions[0].namespace, ExportNamespace::Value);
    }

    #[test]
    fn participants_cover_only_the_colliding_name() {
        let participants = colliding_star_graph(false).ambiguity_participants();

        assert!(participants.contains_in_namespace(FileId(1), "foo", ExportNamespace::Value));
        assert!(participants.contains_in_namespace(FileId(2), "foo", ExportNamespace::Value));
        assert!(!participants.contains_in_namespace(FileId(2), "bar", ExportNamespace::Value));
        assert!(!participants.contains_in_namespace(FileId(1), "foo", ExportNamespace::Type));
    }

    #[test]
    fn a_collision_forwarded_by_a_second_barrel_keeps_the_same_contributors() {
        let graph = colliding_star_graph(true);

        let forwarded = graph.ambiguous_star_exports_on(FileId(3));

        assert_eq!(
            forwarded,
            vec![AmbiguousStarExport {
                barrel: FileId(3),
                name: Box::from("foo"),
                namespace: ExportNamespace::Value,
                contributors: Box::from([FileId(1), FileId(2)]),
            }]
        );
    }

    #[test]
    fn a_project_without_star_re_exports_reports_nothing() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/only.ts"),
            size_bytes: 10,
        }];
        let modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: files[0].path.clone(),
            exports: vec![value_export("foo")].into(),
            ..Default::default()
        }];
        let graph = ModuleGraph::build(&modules, &[], &files);

        assert!(graph.ambiguous_star_exports().is_empty());
        assert!(
            !graph
                .ambiguity_participants()
                .contains_declaration(FileId(0), "foo", false)
        );
    }
}
