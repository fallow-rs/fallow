//! Exports-aware public-export-key computation (the hard 80% of 6.A).
//!
//! Given the set of *public-API entry points* (the `package.json` `exports`-mapped
//! modules plus the no-`exports` source-index fallback; computed in core's
//! `public_api_package_entry_points`, which already encodes rule R4), this
//! resolves the set of export symbols reachable through that public surface and
//! returns one stable `"<rel_path>::<name>"` key per public export.
//!
//! A name is public when it resolves to one unique value binding on a public-API
//! entry point. Candidates come from the entry's own exports and its `export *`
//! closure. Shadowed and ambiguous star names are not public bindings.
//!
//! Keying on the surface AS EXPOSED (the entry's own name, e.g. `index.js::pub`),
//! not the origin's internal name (`src/impl.ts::pub`), is what makes the delta
//! exports-aware and avoids double-counting one symbol on both the barrel and
//! the origin. A symbol re-exported only through an INTERNAL barrel that is not
//! in `exports` never resolves on a public entry, so it produces
//! ZERO public-API delta (the Aisha repro); one re-exported through the
//! `exports`-mapped entry lands on that entry once (exactly one). This mirrors
//! the exports-aware reachability the `unprovided-inject` and
//! `unrendered-component` detectors use, kept in the graph crate so the review
//! brief (cli) can call it directly off the retained graph.

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use rustc_hash::FxHashSet;

use super::{EffectiveExportBinding, EffectiveExportResolution, ExportNamespace, ModuleGraph};

/// Direct declaration exposed through at least one public package entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PublicExportOrigin {
    file_id: FileId,
    export_name: String,
}

impl PublicExportOrigin {
    /// Module that owns the public declaration.
    #[must_use]
    pub const fn file_id(&self) -> FileId {
        self.file_id
    }

    /// Name of the declaration in its owning module.
    #[must_use]
    pub fn export_name(&self) -> &str {
        &self.export_name
    }
}

impl ModuleGraph {
    /// Compute the set of public-export keys reachable through the given
    /// `public_api_entry_points` (an exports-aware set; see module docs).
    ///
    /// Keys are `"<root-relative forward-slashed path>::<export name>"`.
    /// Type-only exports are skipped: a type erased at build carries no runtime
    /// contract, so it never widens the public *value* surface that 6.A tracks.
    #[must_use]
    pub fn public_export_keys(
        &self,
        public_api_entry_points: &FxHashSet<FileId>,
        root: &Path,
    ) -> FxHashSet<String> {
        let candidate_names = self.public_value_export_names(public_api_entry_points);
        let mut keys: FxHashSet<String> = FxHashSet::default();

        for entry_id in public_api_entry_points {
            let Some(entry) = self.modules.get(entry_id.0 as usize) else {
                continue;
            };
            let rel = relativize(&entry.path, root);
            for name in &candidate_names {
                if matches!(
                    self.resolve_export(*entry_id, name, ExportNamespace::Value),
                    EffectiveExportResolution::Unique(_)
                ) {
                    keys.insert(format!("{rel}::{name}"));
                }
            }
        }
        keys
    }

    /// Resolve the direct declarations exposed through public package entries.
    /// Shadowed and ambiguous star candidates contribute no origins.
    #[must_use]
    pub fn public_export_origins(
        &self,
        public_api_entry_points: &FxHashSet<FileId>,
    ) -> FxHashSet<PublicExportOrigin> {
        self.public_export_bindings(public_api_entry_points)
            .into_iter()
            .filter_map(|binding| self.export_binding_origin(binding))
            .map(|origin| PublicExportOrigin {
                file_id: origin.file_id(),
                export_name: origin.export().name.to_string(),
            })
            .collect()
    }

    /// Unique value bindings exposed through at least one public package entry.
    #[must_use]
    pub fn public_export_bindings(
        &self,
        public_api_entry_points: &FxHashSet<FileId>,
    ) -> FxHashSet<EffectiveExportBinding> {
        let candidate_names = self.public_value_export_names(public_api_entry_points);
        public_api_entry_points
            .iter()
            .flat_map(|entry| {
                candidate_names.iter().filter_map(|name| {
                    match self.resolve_export(*entry, name, ExportNamespace::Value) {
                        EffectiveExportResolution::Unique(binding) => Some(binding),
                        EffectiveExportResolution::Missing
                        | EffectiveExportResolution::Ambiguous => None,
                    }
                })
            })
            .collect()
    }

    fn public_value_export_names(
        &self,
        public_api_entry_points: &FxHashSet<FileId>,
    ) -> FxHashSet<String> {
        let star_targets = self.public_star_re_export_targets(public_api_entry_points);
        let mut names: FxHashSet<String> = public_api_entry_points
            .iter()
            .chain(star_targets.iter())
            .filter_map(|id| self.modules.get(id.0 as usize))
            .flat_map(|module| &module.exports)
            .filter(|export| !export.is_type_only)
            .map(|export| export.name.to_string())
            .collect();
        names.insert("default".to_string());
        names
    }

    /// The `export *` closure rooted at the public-API entry points: every module
    /// reachable through a chain of `export * from './x'` edges starting from a
    /// public entry. Their exported names are candidates for effective
    /// resolution on each public entry.
    fn public_star_re_export_targets(
        &self,
        public_api_entry_points: &FxHashSet<FileId>,
    ) -> FxHashSet<FileId> {
        let mut targets: FxHashSet<FileId> = public_api_entry_points
            .iter()
            .filter_map(|id| self.modules.get(id.0 as usize))
            .flat_map(|module| {
                module
                    .re_exports
                    .iter()
                    .filter(|re| re.exported_name == "*")
                    .map(|re| re.source_file)
            })
            .collect();

        let mut stack: Vec<FileId> = targets.iter().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(module) = self.modules.get(id.0 as usize) else {
                continue;
            };
            for re in module
                .re_exports
                .iter()
                .filter(|re| re.exported_name == "*")
            {
                if targets.insert(re.source_file) {
                    stack.push(re.source_file);
                }
            }
        }
        targets
    }
}

/// Strip `root` and forward-slash-normalize a module path (mirrors
/// `impact_closure::relativize` for cross-platform key parity).
fn relativize(path: &Path, root: &Path) -> String {
    let rel: PathBuf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
    };
    use std::path::PathBuf;

    fn file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 10,
        }
    }

    fn named_export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn re_export(imported: &str, exported: &str, target: FileId) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: "./impl".to_string(),
                imported_name: imported.to_string(),
                exported_name: exported.to_string(),
                is_type_only: false,
                span: oxc_span::Span::new(0, 10),
                statement_span: oxc_span::Span::new(0, 0),
                source_span: oxc_span::Span::new(0, 0),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn named_import(name: &str, target: FileId) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: "./x".to_string(),
                imported_name: ImportedName::Named(name.to_string()),
                local_name: name.to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    /// index (0, the exports entry) re-exports `pub` from impl (1, NOT public);
    /// internal-barrel (2) re-exports `priv` from impl. consumer (3) imports both.
    fn build_graph() -> (ModuleGraph, FxHashSet<FileId>) {
        let files = vec![
            file(0, "/p/index.js"),
            file(1, "/p/src/impl.ts"),
            file(2, "/p/src/internal.ts"),
            file(3, "/p/src/consumer.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/index.js"),
            source: EntryPointSource::PackageJsonExports,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/index.js"),
                re_exports: vec![re_export("pub", "pub", FileId(1))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/impl.ts"),
                exports: vec![named_export("pub"), named_export("priv")].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/internal.ts"),
                re_exports: vec![re_export("priv", "priv", FileId(1))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/p/src/consumer.ts"),
                resolved_imports: vec![
                    named_import("pub", FileId(0)),
                    named_import("priv", FileId(2)),
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        // The exports-mapped entry set: only index.js (PackageJsonExports).
        let public_entries: FxHashSet<FileId> = std::iter::once(FileId(0)).collect();
        (graph, public_entries)
    }

    #[test]
    fn export_reexported_through_exports_path_is_public() {
        let (graph, public_entries) = build_graph();
        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));
        // `pub` is re-exported through the exports-mapped index.js, so it appears
        // on the public surface keyed at the entry (the exposed name), not the
        // internal origin.
        assert!(
            keys.contains("index.js::pub"),
            "exports-reachable symbol must be public: {keys:?}"
        );
    }

    #[test]
    fn export_reexported_only_through_internal_barrel_is_not_public() {
        let (graph, public_entries) = build_graph();
        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));
        // `priv` reaches a consumer ONLY through the internal (non-exports)
        // barrel, so it is on no public-surface key (neither the entry nor a
        // star-target).
        assert!(
            !keys.iter().any(|k| k.ends_with("::priv")),
            "internal-barrel-only symbol must NOT be public: {keys:?}"
        );
    }

    /// Build the Aisha-repro graph parameterized by which impl symbols exist and
    /// which is re-exported through the exports-mapped `index.js`. `internal`
    /// (if present) is re-exported only through the non-exports internal barrel.
    fn build_aisha_graph(
        impl_exports: &[&str],
        exports_reexported: &[&str],
        internal_reexported: &[&str],
    ) -> (ModuleGraph, FxHashSet<FileId>) {
        let files = vec![
            file(0, "/p/index.js"),
            file(1, "/p/src/impl.ts"),
            file(2, "/p/src/internal.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/index.js"),
            source: EntryPointSource::PackageJsonExports,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/index.js"),
                re_exports: exports_reexported
                    .iter()
                    .map(|n| re_export(n, n, FileId(1)))
                    .collect(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/impl.ts"),
                exports: impl_exports.iter().map(|n| named_export(n)).collect(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/internal.ts"),
                re_exports: internal_reexported
                    .iter()
                    .map(|n| re_export(n, n, FileId(1)))
                    .collect(),
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let public_entries: FxHashSet<FileId> = std::iter::once(FileId(0)).collect();
        (graph, public_entries)
    }

    #[test]
    fn done_condition_internal_zero_exports_one() {
        let root = Path::new("/p");
        // Base: impl exports `pub`, re-exported through the exports-mapped index.
        let (base_graph, base_entries) = build_aisha_graph(&["pub"], &["pub"], &[]);
        let base = base_graph.public_export_keys(&base_entries, root);

        // Head A: add an internal-barrel symbol NOT in exports -> 0 public deltas.
        let (head_a_graph, head_a_entries) =
            build_aisha_graph(&["pub", "internalOnly"], &["pub"], &["internalOnly"]);
        let head_a = head_a_graph.public_export_keys(&head_a_entries, root);
        let internal_delta: Vec<_> = head_a.difference(&base).collect();
        assert!(
            internal_delta.is_empty(),
            "internal-barrel symbol must yield ZERO public-API delta: {internal_delta:?}"
        );

        // Head B: add a symbol reachable through the exports path -> exactly 1.
        let (head_b_graph, head_b_entries) =
            build_aisha_graph(&["pub", "widget"], &["pub", "widget"], &[]);
        let head_b = head_b_graph.public_export_keys(&head_b_entries, root);
        let exports_delta: Vec<_> = head_b.difference(&base).collect();
        assert_eq!(
            exports_delta.len(),
            1,
            "exports-reachable symbol must yield EXACTLY ONE public-API delta: {exports_delta:?}"
        );
        assert_eq!(exports_delta[0], "index.js::widget");
    }

    #[test]
    fn type_only_exports_are_skipped() {
        let files = vec![file(0, "/p/index.ts")];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/index.ts"),
            source: EntryPointSource::PackageJsonExports,
        }];
        let mut type_export = named_export("T");
        type_export.is_type_only = true;
        let resolved = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/p/index.ts"),
            exports: vec![type_export, named_export("v")].into(),
            ..Default::default()
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let public_entries: FxHashSet<FileId> = std::iter::once(FileId(0)).collect();
        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));
        assert!(keys.contains("index.ts::v"));
        assert!(!keys.contains("index.ts::T"), "type-only export skipped");
    }

    fn build_star_surface_graph(
        entry_re_exports: Vec<ResolvedReExport>,
        source_modules: Vec<ResolvedModule>,
    ) -> (ModuleGraph, FxHashSet<FileId>) {
        let mut files = vec![file(0, "/p/index.ts")];
        files.extend(source_modules.iter().map(|module| {
            file(
                module.file_id.0,
                module.path.to_str().expect("test path is UTF-8"),
            )
        }));
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/index.ts"),
            source: EntryPointSource::PackageJsonExports,
        }];
        let mut resolved = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/p/index.ts"),
            re_exports: entry_re_exports,
            ..Default::default()
        }];
        resolved.extend(source_modules);
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        (graph, std::iter::once(FileId(0)).collect())
    }

    #[test]
    fn explicit_export_hides_shadowed_star_binding_from_public_surface() {
        let (graph, public_entries) = build_star_surface_graph(
            vec![
                re_export("*", "*", FileId(1)),
                re_export("foo", "foo", FileId(2)),
            ],
            vec![
                ResolvedModule {
                    file_id: FileId(1),
                    path: PathBuf::from("/p/star-source.ts"),
                    exports: vec![named_export("foo")].into(),
                    ..Default::default()
                },
                ResolvedModule {
                    file_id: FileId(2),
                    path: PathBuf::from("/p/explicit-source.ts"),
                    exports: vec![named_export("foo")].into(),
                    ..Default::default()
                },
            ],
        );

        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));

        assert_eq!(keys, FxHashSet::from_iter(["index.ts::foo".to_string()]));
    }

    #[test]
    fn ambiguous_star_binding_is_absent_from_public_surface() {
        let (graph, public_entries) = build_star_surface_graph(
            vec![
                re_export("*", "*", FileId(1)),
                re_export("*", "*", FileId(2)),
            ],
            vec![
                ResolvedModule {
                    file_id: FileId(1),
                    path: PathBuf::from("/p/left.ts"),
                    exports: vec![named_export("foo")].into(),
                    ..Default::default()
                },
                ResolvedModule {
                    file_id: FileId(2),
                    path: PathBuf::from("/p/right.ts"),
                    exports: vec![named_export("foo")].into(),
                    ..Default::default()
                },
            ],
        );

        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));

        assert!(
            keys.is_empty(),
            "ambiguous foo is not a public binding: {keys:?}"
        );
    }

    #[test]
    fn convergent_star_diamond_has_one_exposed_public_key() {
        let (graph, public_entries) = build_star_surface_graph(
            vec![
                re_export("*", "*", FileId(1)),
                re_export("*", "*", FileId(2)),
            ],
            vec![
                ResolvedModule {
                    file_id: FileId(1),
                    path: PathBuf::from("/p/left.ts"),
                    re_exports: vec![re_export("*", "*", FileId(3))],
                    ..Default::default()
                },
                ResolvedModule {
                    file_id: FileId(2),
                    path: PathBuf::from("/p/right.ts"),
                    re_exports: vec![re_export("*", "*", FileId(3))],
                    ..Default::default()
                },
                ResolvedModule {
                    file_id: FileId(3),
                    path: PathBuf::from("/p/source.ts"),
                    exports: vec![named_export("foo")].into(),
                    ..Default::default()
                },
            ],
        );

        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));

        assert_eq!(keys, FxHashSet::from_iter(["index.ts::foo".to_string()]));
    }

    #[test]
    fn public_surface_preserves_aliases_default_and_value_namespace() {
        let mut default_export = named_export("Widget");
        default_export.name = ExportName::Default;
        let mut type_export = named_export("T");
        type_export.is_type_only = true;
        let mut type_re_export = re_export("T", "PublicType", FileId(1));
        type_re_export.info.is_type_only = true;
        let (graph, public_entries) = build_star_surface_graph(
            vec![
                re_export("default", "default", FileId(1)),
                re_export("default", "Widget", FileId(1)),
                type_re_export,
            ],
            vec![ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/widget.ts"),
                exports: vec![default_export, type_export].into(),
                ..Default::default()
            }],
        );

        let keys = graph.public_export_keys(&public_entries, Path::new("/p"));

        assert_eq!(
            keys,
            FxHashSet::from_iter([
                "index.ts::default".to_string(),
                "index.ts::Widget".to_string(),
            ])
        );
    }
}
