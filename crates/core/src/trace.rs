use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::duplicates::{CloneInstance, DuplicationReport};
use crate::graph::{ModuleGraph, ReferenceKind};

/// Match a user-provided file path against a module's actual path.
///
/// Handles monorepo scenarios where module paths may be canonicalized
/// (symlinks resolved) while user-provided paths are not.
fn path_matches(module_path: &Path, root: &Path, user_path: &str) -> bool {
    let rel = module_path.strip_prefix(root).unwrap_or(module_path);
    let rel_str = rel.to_string_lossy();
    if rel_str == user_path || module_path.to_string_lossy() == user_path {
        return true;
    }
    if dunce::canonicalize(root).is_ok_and(|canonical_root| {
        module_path
            .strip_prefix(&canonical_root)
            .is_ok_and(|rel| rel.to_string_lossy() == user_path)
    }) {
        return true;
    }
    let module_str = module_path.to_string_lossy();
    module_str.ends_with(&format!("/{user_path}"))
}

/// Result of tracing an export: why is it considered used or unused?
#[derive(Debug, Serialize)]
pub struct ExportTrace {
    /// The file containing the export.
    pub file: PathBuf,
    /// The export name being traced.
    pub export_name: String,
    /// Whether the file is reachable from an entry point.
    pub file_reachable: bool,
    /// Whether the file is an entry point.
    pub is_entry_point: bool,
    /// Whether the export is considered used.
    pub is_used: bool,
    /// Files that reference this export directly.
    pub direct_references: Vec<ExportReference>,
    /// Re-export chains that pass through this export.
    pub re_export_chains: Vec<ReExportChain>,
    /// Reason summary.
    pub reason: String,
}

/// A direct reference to an export.
#[derive(Debug, Serialize)]
pub struct ExportReference {
    pub from_file: PathBuf,
    pub kind: String,
}

/// A re-export chain showing how an export is propagated.
#[derive(Debug, Serialize)]
pub struct ReExportChain {
    /// The barrel file that re-exports this symbol.
    pub barrel_file: PathBuf,
    /// The name it's re-exported as.
    pub exported_as: String,
    /// Number of references on the barrel's re-exported symbol.
    pub reference_count: usize,
}

/// Result of tracing all edges for a file.
#[derive(Debug, Serialize)]
pub struct FileTrace {
    /// The traced file.
    pub file: PathBuf,
    /// Whether this file is reachable from entry points.
    pub is_reachable: bool,
    /// Whether this file is an entry point.
    pub is_entry_point: bool,
    /// Exports declared by this file.
    pub exports: Vec<TracedExport>,
    /// Files that this file imports from.
    pub imports_from: Vec<PathBuf>,
    /// Files that import from this file.
    pub imported_by: Vec<PathBuf>,
    /// Re-exports declared by this file.
    pub re_exports: Vec<TracedReExport>,
}

/// An export with its usage info.
#[derive(Debug, Serialize)]
pub struct TracedExport {
    pub name: String,
    pub is_type_only: bool,
    pub reference_count: usize,
    pub referenced_by: Vec<ExportReference>,
}

/// A re-export with source info.
#[derive(Debug, Serialize)]
pub struct TracedReExport {
    pub source_file: PathBuf,
    pub imported_name: String,
    pub exported_name: String,
}

/// Result of tracing a dependency: where is it used?
#[derive(Debug, Serialize)]
pub struct DependencyTrace {
    /// The dependency name being traced.
    pub package_name: String,
    /// Files that import this dependency.
    pub imported_by: Vec<PathBuf>,
    /// Files that import this dependency with type-only imports.
    pub type_only_imported_by: Vec<PathBuf>,
    /// Whether the dependency is used at all.
    pub is_used: bool,
    /// Total import count.
    pub import_count: usize,
}

/// Pipeline performance timings.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineTimings {
    pub discover_files_ms: f64,
    pub file_count: usize,
    pub workspaces_ms: f64,
    pub workspace_count: usize,
    pub plugins_ms: f64,
    pub script_analysis_ms: f64,
    pub parse_extract_ms: f64,
    pub module_count: usize,
    /// Number of files whose parse results were loaded from cache (skipped parsing).
    pub cache_hits: usize,
    /// Number of files that required a full parse (new or changed content).
    pub cache_misses: usize,
    pub cache_update_ms: f64,
    pub entry_points_ms: f64,
    pub entry_point_count: usize,
    pub resolve_imports_ms: f64,
    pub build_graph_ms: f64,
    pub analyze_ms: f64,
    pub total_ms: f64,
}

/// Trace why an export is considered used or unused.
#[must_use]
pub fn trace_export(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<ExportTrace> {
    // Find the file in the graph
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    // Find the export
    let export = module.exports.iter().find(|e| {
        let name_str = e.name.to_string();
        name_str == export_name || (export_name == "default" && name_str == "default")
    })?;

    let direct_references: Vec<ExportReference> = export
        .references
        .iter()
        .map(|r| {
            let from_path = graph.modules.get(r.from_file.0 as usize).map_or_else(
                || PathBuf::from(format!("<unknown:{}>", r.from_file.0)),
                |m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf(),
            );
            ExportReference {
                from_file: from_path,
                kind: format_reference_kind(r.kind),
            }
        })
        .collect();

    // Find re-export chains involving this export
    let re_export_chains: Vec<ReExportChain> = graph
        .modules
        .iter()
        .flat_map(|m| {
            m.re_exports
                .iter()
                .filter(|re| {
                    re.source_file == module.file_id
                        && (re.imported_name == export_name || re.imported_name == "*")
                })
                .map(|re| {
                    let barrel_export = m.exports.iter().find(|e| {
                        if re.exported_name == "*" {
                            e.name.to_string() == export_name
                        } else {
                            e.name.to_string() == re.exported_name
                        }
                    });
                    ReExportChain {
                        barrel_file: m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf(),
                        exported_as: re.exported_name.clone(),
                        reference_count: barrel_export.map_or(0, |e| e.references.len()),
                    }
                })
        })
        .collect();

    let is_used = !export.references.is_empty();
    let reason = if !module.is_reachable() {
        "File is unreachable from any entry point".to_string()
    } else if is_used {
        format!(
            "Used by {} file(s){}",
            export.references.len(),
            if re_export_chains.is_empty() {
                String::new()
            } else {
                format!(", re-exported through {} barrel(s)", re_export_chains.len())
            }
        )
    } else if module.is_entry_point() {
        "No internal references, but file is an entry point (export is externally accessible)"
            .to_string()
    } else if !re_export_chains.is_empty() {
        format!(
            "Re-exported through {} barrel(s) but no consumer imports it through the barrel",
            re_export_chains.len()
        )
    } else {
        "No references found — export is unused".to_string()
    };

    Some(ExportTrace {
        file: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        export_name: export_name.to_string(),
        file_reachable: module.is_reachable(),
        is_entry_point: module.is_entry_point(),
        is_used,
        direct_references,
        re_export_chains,
        reason,
    })
}

/// Trace all edges for a file.
#[must_use]
pub fn trace_file(graph: &ModuleGraph, root: &Path, file_path: &str) -> Option<FileTrace> {
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    let exports: Vec<TracedExport> = module
        .exports
        .iter()
        .map(|e| TracedExport {
            name: e.name.to_string(),
            is_type_only: e.is_type_only,
            reference_count: e.references.len(),
            referenced_by: e
                .references
                .iter()
                .map(|r| {
                    let from_path = graph.modules.get(r.from_file.0 as usize).map_or_else(
                        || PathBuf::from(format!("<unknown:{}>", r.from_file.0)),
                        |m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf(),
                    );
                    ExportReference {
                        from_file: from_path,
                        kind: format_reference_kind(r.kind),
                    }
                })
                .collect(),
        })
        .collect();

    // Edges FROM this file (what it imports)
    let imports_from: Vec<PathBuf> = graph
        .edges_for(module.file_id)
        .iter()
        .filter_map(|target_id| {
            graph
                .modules
                .get(target_id.0 as usize)
                .map(|m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf())
        })
        .collect();

    // Reverse deps: who imports this file
    let imported_by: Vec<PathBuf> = graph
        .reverse_deps
        .get(module.file_id.0 as usize)
        .map(|deps| {
            deps.iter()
                .filter_map(|fid| {
                    graph
                        .modules
                        .get(fid.0 as usize)
                        .map(|m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf())
                })
                .collect()
        })
        .unwrap_or_default();

    let re_exports: Vec<TracedReExport> = module
        .re_exports
        .iter()
        .map(|re| {
            let source_path = graph.modules.get(re.source_file.0 as usize).map_or_else(
                || PathBuf::from(format!("<unknown:{}>", re.source_file.0)),
                |m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf(),
            );
            TracedReExport {
                source_file: source_path,
                imported_name: re.imported_name.clone(),
                exported_name: re.exported_name.clone(),
            }
        })
        .collect();

    Some(FileTrace {
        file: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        is_reachable: module.is_reachable(),
        is_entry_point: module.is_entry_point(),
        exports,
        imports_from,
        imported_by,
        re_exports,
    })
}

/// Trace where a dependency is used.
#[must_use]
pub fn trace_dependency(graph: &ModuleGraph, root: &Path, package_name: &str) -> DependencyTrace {
    let imported_by: Vec<PathBuf> = graph
        .package_usage
        .get(package_name)
        .map(|ids| {
            ids.iter()
                .filter_map(|fid| {
                    graph
                        .modules
                        .get(fid.0 as usize)
                        .map(|m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf())
                })
                .collect()
        })
        .unwrap_or_default();

    let type_only_imported_by: Vec<PathBuf> = graph
        .type_only_package_usage
        .get(package_name)
        .map(|ids| {
            ids.iter()
                .filter_map(|fid| {
                    graph
                        .modules
                        .get(fid.0 as usize)
                        .map(|m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf())
                })
                .collect()
        })
        .unwrap_or_default();

    let import_count = imported_by.len();
    DependencyTrace {
        package_name: package_name.to_string(),
        imported_by,
        type_only_imported_by,
        is_used: import_count > 0,
        import_count,
    }
}

fn format_reference_kind(kind: ReferenceKind) -> String {
    match kind {
        ReferenceKind::NamedImport => "named import".to_string(),
        ReferenceKind::DefaultImport => "default import".to_string(),
        ReferenceKind::NamespaceImport => "namespace import".to_string(),
        ReferenceKind::ReExport => "re-export".to_string(),
        ReferenceKind::DynamicImport => "dynamic import".to_string(),
        ReferenceKind::SideEffectImport => "side-effect import".to_string(),
    }
}

/// Result of tracing a clone: all groups containing the code at a given location.
#[derive(Debug, Serialize)]
pub struct CloneTrace {
    pub file: PathBuf,
    pub line: usize,
    pub matched_instance: Option<CloneInstance>,
    pub clone_groups: Vec<TracedCloneGroup>,
}

#[derive(Debug, Serialize)]
pub struct TracedCloneGroup {
    pub token_count: usize,
    pub line_count: usize,
    pub instances: Vec<CloneInstance>,
}

#[must_use]
pub fn trace_clone(
    report: &DuplicationReport,
    root: &Path,
    file_path: &str,
    line: usize,
) -> CloneTrace {
    let resolved = root.join(file_path);
    let mut matched_instance = None;
    let mut clone_groups = Vec::new();

    for group in &report.clone_groups {
        let matching = group.instances.iter().find(|inst| {
            let inst_matches = inst.file == resolved
                || inst.file.strip_prefix(root).unwrap_or(&inst.file) == Path::new(file_path);
            inst_matches && inst.start_line <= line && line <= inst.end_line
        });

        if let Some(matched) = matching {
            if matched_instance.is_none() {
                matched_instance = Some(relativize_instance(matched, root));
            }
            clone_groups.push(TracedCloneGroup {
                token_count: group.token_count,
                line_count: group.line_count,
                instances: group
                    .instances
                    .iter()
                    .map(|inst| relativize_instance(inst, root))
                    .collect(),
            });
        }
    }

    CloneTrace {
        file: PathBuf::from(file_path),
        line,
        matched_instance,
        clone_groups,
    }
}

/// Return a copy of `inst` with `file` rewritten relative to `root` (forward-slash normalized
/// for cross-platform JSON parity with `serde_path::serialize`). If `inst.file` is already
/// outside `root`, the path is left unchanged.
fn relativize_instance(inst: &CloneInstance, root: &Path) -> CloneInstance {
    let rel = inst.file.strip_prefix(root).map_or_else(
        |_| inst.file.clone(),
        |p| PathBuf::from(p.to_string_lossy().replace('\\', "/")),
    );
    CloneInstance {
        file: rel,
        ..inst.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use crate::extract::{ExportInfo, ExportName, ImportInfo, ImportedName, VisibilityTag};
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};

    fn build_test_graph() -> ModuleGraph {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/utils.ts"),
                size_bytes: 50,
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/src/unused.ts"),
                size_bytes: 30,
            },
        ];

        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/utils.ts"),
                exports: vec![
                    ExportInfo {
                        name: ExportName::Named("foo".to_string()),
                        local_name: Some("foo".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        super_class: None,
                    },
                    ExportInfo {
                        name: ExportName::Named("bar".to_string()),
                        local_name: Some("bar".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(21, 40),
                        members: vec![],
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/project/src/unused.ts"),
                exports: vec![ExportInfo {
                    name: ExportName::Named("baz".to_string()),
                    local_name: Some("baz".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 15),
                    members: vec![],
                    super_class: None,
                }],
                ..Default::default()
            },
        ];

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn trace_used_export() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_export(&graph, root, "src/utils.ts", "foo").unwrap();
        assert!(trace.is_used);
        assert!(trace.file_reachable);
        assert_eq!(trace.direct_references.len(), 1);
        assert_eq!(
            trace.direct_references[0].from_file,
            PathBuf::from("src/entry.ts")
        );
        assert_eq!(trace.direct_references[0].kind, "named import");
    }

    #[test]
    fn trace_unused_export() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_export(&graph, root, "src/utils.ts", "bar").unwrap();
        assert!(!trace.is_used);
        assert!(trace.file_reachable);
        assert!(trace.direct_references.is_empty());
    }

    #[test]
    fn trace_unreachable_file_export() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_export(&graph, root, "src/unused.ts", "baz").unwrap();
        assert!(!trace.is_used);
        assert!(!trace.file_reachable);
        assert!(trace.reason.contains("unreachable"));
    }

    #[test]
    fn trace_nonexistent_export() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_export(&graph, root, "src/utils.ts", "nonexistent");
        assert!(trace.is_none());
    }

    #[test]
    fn trace_nonexistent_file() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_export(&graph, root, "src/nope.ts", "foo");
        assert!(trace.is_none());
    }

    #[test]
    fn trace_file_edges() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_file(&graph, root, "src/entry.ts").unwrap();
        assert!(trace.is_entry_point);
        assert!(trace.is_reachable);
        assert_eq!(trace.imports_from.len(), 1);
        assert_eq!(trace.imports_from[0], PathBuf::from("src/utils.ts"));
        assert!(trace.imported_by.is_empty());
    }

    #[test]
    fn trace_file_imported_by() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_file(&graph, root, "src/utils.ts").unwrap();
        assert!(!trace.is_entry_point);
        assert!(trace.is_reachable);
        assert_eq!(trace.exports.len(), 2);
        assert_eq!(trace.imported_by.len(), 1);
        assert_eq!(trace.imported_by[0], PathBuf::from("src/entry.ts"));
    }

    #[test]
    fn trace_unreachable_file() {
        let graph = build_test_graph();
        let root = Path::new("/project");

        let trace = trace_file(&graph, root, "src/unused.ts").unwrap();
        assert!(!trace.is_reachable);
        assert!(!trace.is_entry_point);
        assert!(trace.imported_by.is_empty());
    }

    #[test]
    fn trace_dependency_used() {
        // Build a graph with npm package usage
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/app.ts"),
            size_bytes: 100,
        }];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/app.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/app.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "lodash".to_string(),
                    imported_name: ImportedName::Named("get".to_string()),
                    local_name: "get".to_string(),
                    is_type_only: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("lodash".to_string()),
            }],
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let root = Path::new("/project");

        let trace = trace_dependency(&graph, root, "lodash");
        assert!(trace.is_used);
        assert_eq!(trace.import_count, 1);
        assert_eq!(trace.imported_by[0], PathBuf::from("src/app.ts"));
    }

    #[test]
    fn trace_dependency_unused() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/app.ts"),
            size_bytes: 100,
        }];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/app.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/app.ts"),
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let root = Path::new("/project");

        let trace = trace_dependency(&graph, root, "nonexistent-pkg");
        assert!(!trace.is_used);
        assert_eq!(trace.import_count, 0);
        assert!(trace.imported_by.is_empty());
    }

    #[test]
    fn trace_clone_finds_matching_group() {
        use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from("/project/src/a.ts"),
                        start_line: 10,
                        end_line: 20,
                        start_col: 0,
                        end_col: 0,
                        fragment: "fn foo() {}".to_string(),
                    },
                    CloneInstance {
                        file: PathBuf::from("/project/src/b.ts"),
                        start_line: 5,
                        end_line: 15,
                        start_col: 0,
                        end_col: 0,
                        fragment: "fn foo() {}".to_string(),
                    },
                ],
                token_count: 60,
                line_count: 11,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 22,
                total_tokens: 200,
                duplicated_tokens: 120,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 22.0,
            },
        };
        let trace = trace_clone(&report, Path::new("/project"), "src/a.ts", 15);
        assert!(trace.matched_instance.is_some());
        assert_eq!(trace.clone_groups.len(), 1);
        assert_eq!(trace.clone_groups[0].instances.len(), 2);
    }

    #[test]
    fn trace_clone_no_match() {
        use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: PathBuf::from("/project/src/a.ts"),
                    start_line: 10,
                    end_line: 20,
                    start_col: 0,
                    end_col: 0,
                    fragment: "fn foo() {}".to_string(),
                }],
                token_count: 60,
                line_count: 11,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 1,
                files_with_clones: 1,
                total_lines: 50,
                duplicated_lines: 11,
                total_tokens: 100,
                duplicated_tokens: 60,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 22.0,
            },
        };
        let trace = trace_clone(&report, Path::new("/project"), "src/a.ts", 25);
        assert!(trace.matched_instance.is_none());
        assert!(trace.clone_groups.is_empty());
    }

    #[test]
    fn trace_clone_line_boundary() {
        use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from("/project/src/a.ts"),
                        start_line: 10,
                        end_line: 20,
                        start_col: 0,
                        end_col: 0,
                        fragment: "code".to_string(),
                    },
                    CloneInstance {
                        file: PathBuf::from("/project/src/b.ts"),
                        start_line: 1,
                        end_line: 11,
                        start_col: 0,
                        end_col: 0,
                        fragment: "code".to_string(),
                    },
                ],
                token_count: 50,
                line_count: 11,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 22,
                total_tokens: 200,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 22.0,
            },
        };
        let root = Path::new("/project");
        assert!(
            trace_clone(&report, root, "src/a.ts", 10)
                .matched_instance
                .is_some()
        );
        assert!(
            trace_clone(&report, root, "src/a.ts", 20)
                .matched_instance
                .is_some()
        );
        assert!(
            trace_clone(&report, root, "src/a.ts", 21)
                .matched_instance
                .is_none()
        );
    }

    #[test]
    fn trace_clone_returns_relative_instance_paths() {
        use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from("/project/src/a.ts"),
                        start_line: 1,
                        end_line: 10,
                        start_col: 0,
                        end_col: 0,
                        fragment: "code".to_string(),
                    },
                    CloneInstance {
                        file: PathBuf::from("/project/src/b.ts"),
                        start_line: 1,
                        end_line: 10,
                        start_col: 0,
                        end_col: 0,
                        fragment: "code".to_string(),
                    },
                ],
                token_count: 50,
                line_count: 10,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 50,
                duplicated_lines: 20,
                total_tokens: 100,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 40.0,
            },
        };
        let trace = trace_clone(&report, Path::new("/project"), "src/a.ts", 5);
        let matched = trace.matched_instance.as_ref().expect("match expected");
        assert_eq!(matched.file, PathBuf::from("src/a.ts"));
        for group in &trace.clone_groups {
            for inst in &group.instances {
                let as_str = inst.file.to_string_lossy();
                assert!(
                    !as_str.starts_with('/'),
                    "instance file should be relative, got {as_str}",
                );
                assert!(
                    !as_str.contains(":\\") && !as_str.contains(":/"),
                    "instance file should not have a drive letter, got {as_str}",
                );
            }
        }

        let json = serde_json::to_string(&trace).expect("serializes");
        assert!(
            !json.contains("\"/project/"),
            "serialized trace should not leak absolute paths: {json}",
        );
    }
}
