use std::path::{Path, PathBuf};

pub use fallow_types::trace::{
    ClassMemberTrace, CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace,
    ImpactClosureGap, ImpactClosureTrace, PipelineTimings, ReExportChain, TracedCloneGroup,
    TracedExport, TracedReExport,
};
use rustc_hash::FxHashSet;

use crate::duplicates::{
    CloneFingerprintSet, CloneGroup, CloneInstance, DuplicationReport, dominant_identifier,
    group_refactoring_suggestion,
};
use crate::graph::{ModuleGraph, ReferenceKind};

/// Match a user-provided file path against a module's actual path.
///
/// Handles monorepo scenarios where module paths may be canonicalized
/// (symlinks resolved) while user-provided paths are not.
fn path_matches(module_path: &Path, root: &Path, user_path: &str) -> bool {
    let user_path_norm = user_path.replace('\\', "/");
    let rel = module_path.strip_prefix(root).unwrap_or(module_path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let module_str = module_path.to_string_lossy().replace('\\', "/");
    if rel_str == user_path_norm || module_str == user_path_norm {
        return true;
    }
    if dunce::canonicalize(root).is_ok_and(|canonical_root| {
        module_path
            .strip_prefix(&canonical_root)
            .is_ok_and(|rel| rel.to_string_lossy().replace('\\', "/") == user_path_norm)
    }) {
        return true;
    }
    module_str.ends_with(&format!("/{user_path_norm}"))
}

/// Map a reference's `from_file` id to a root-relative [`ExportReference`].
fn reference_to_export_reference(
    graph: &ModuleGraph,
    root: &Path,
    r: &crate::graph::SymbolReference,
) -> ExportReference {
    let from_path = graph.modules.get(r.from_file.0 as usize).map_or_else(
        || PathBuf::from(format!("<unknown:{}>", r.from_file.0)),
        |m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf(),
    );
    ExportReference {
        from_file: from_path,
        kind: format_reference_kind(r.kind),
    }
}

/// Collect every re-export chain across the graph that re-exports `export_name`
/// from the module identified by `target_file_id`.
fn collect_re_export_chains(
    graph: &ModuleGraph,
    root: &Path,
    target_file_id: crate::discover::FileId,
    export_name: &str,
) -> Vec<ReExportChain> {
    graph
        .modules
        .iter()
        .flat_map(|m| {
            m.re_exports
                .iter()
                .filter(move |re| {
                    re.source_file == target_file_id
                        && (re.imported_name == export_name || re.imported_name == "*")
                })
                .map(move |re| {
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
        .collect()
}

/// Build the human-readable reason string explaining an export's used/unused state.
fn export_trace_reason(
    module: &crate::graph::ModuleNode,
    reference_count: usize,
    is_used: bool,
    re_export_chains: &[ReExportChain],
) -> String {
    if !module.is_reachable() {
        "File is unreachable from any entry point".to_string()
    } else if is_used {
        format!(
            "Used by {} file(s){}",
            reference_count,
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
        "No references found, export is unused".to_string()
    }
}

/// Trace why an export is considered used or unused.
#[must_use]
pub fn trace_export(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<ExportTrace> {
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    let export = module
        .exports
        .iter()
        .filter(|e| export_name_matches(e, export_name))
        .max_by_key(|e| (!e.references.is_empty(), !e.is_type_only))?;

    let direct_references: Vec<ExportReference> = export
        .references
        .iter()
        .map(|r| reference_to_export_reference(graph, root, r))
        .collect();

    let re_export_chains = collect_re_export_chains(graph, root, module.file_id, export_name);

    let is_used = !export.references.is_empty();
    let reason = export_trace_reason(module, export.references.len(), is_used, &re_export_chains);

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

/// Trace a class / enum / store MEMBER when `--trace FILE:NAME`'s `NAME` is not
/// a top-level export but a member declared on one (issue #1744). Runs on the
/// graph only, so it reports the OWNING export's reachability and usage (the
/// gating precondition for member crediting) plus a pointer to the right
/// `--unused-*-members` command, not per-member crediting provenance.
#[must_use]
pub fn trace_class_member(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    member_name: &str,
) -> Option<ClassMemberTrace> {
    use fallow_types::extract::MemberKind;

    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    // Find the export that declares this member. When several declare a member
    // of the same name (rare), prefer a used, non-type-only owner so the trace
    // reports the reachable one.
    let (owner, member_kind) = module
        .exports
        .iter()
        .filter_map(|export| {
            export
                .members
                .iter()
                .find(|member| member.name == member_name)
                .map(|member| (export, member.kind))
        })
        .max_by_key(|(export, _)| (!export.references.is_empty(), !export.is_type_only))?;

    let owner_name = owner.name.to_string();
    // Reuse the export trace to compute the owner's reachability / usage /
    // references consistently with a plain `--trace FILE:OWNER`. The `?` here is
    // a belt-and-suspenders guard: `owner` was just located in this module's
    // `exports`, so `trace_export` resolves it in practice; the fallthrough to
    // `None` (and the caller's "not found" error) is unreachable barring a graph
    // inconsistency.
    let owner_trace = trace_export(graph, root, file_path, &owner_name)?;

    let (kind_str, filter_flag) = match member_kind {
        MemberKind::ClassMethod => ("class-method", Some("--unused-class-members")),
        MemberKind::ClassProperty => ("class-property", Some("--unused-class-members")),
        MemberKind::EnumMember => ("enum-member", Some("--unused-enum-members")),
        MemberKind::StoreMember => ("store-member", Some("--unused-store-members")),
        MemberKind::NamespaceMember => ("namespace-member", None),
    };

    let reason = class_member_trace_reason(
        member_name,
        &owner_name,
        kind_str,
        filter_flag,
        file_path,
        &owner_trace,
    );

    Some(ClassMemberTrace {
        file: owner_trace.file,
        member_name: member_name.to_string(),
        member_kind: kind_str.to_string(),
        owner_export: owner_name,
        owner_is_used: owner_trace.is_used,
        owner_file_reachable: owner_trace.file_reachable,
        owner_is_entry_point: owner_trace.is_entry_point,
        owner_direct_references: owner_trace.direct_references,
        owner_re_export_chains: owner_trace.re_export_chains,
        reason,
    })
}

/// Build the human-readable reason for a class-member trace, keyed on the
/// owner's reachability / usage (the precondition that gates member crediting).
fn class_member_trace_reason(
    member_name: &str,
    owner_name: &str,
    kind_str: &str,
    filter_flag: Option<&str>,
    file_path: &str,
    owner_trace: &ExportTrace,
) -> String {
    let head =
        format!("'{member_name}' is a {kind_str} of '{owner_name}', not a top-level export. ");
    let body = if !owner_trace.file_reachable {
        format!(
            "The file is not reachable from any entry point, so '{owner_name}' and all its \
             members are dead (see the unused-file finding)."
        )
    } else if !owner_trace.is_used {
        format!(
            "'{owner_name}' is reachable but referenced by no file, so it is reported as an \
             unused export and its members are not judged individually."
        )
    } else {
        let refs = owner_trace.direct_references.len();
        match filter_flag {
            Some(flag) => format!(
                "'{owner_name}' is used by {refs} file(s); whether '{member_name}' itself is \
                 flagged depends on cross-file member-access resolution. Run \
                 `fallow dead-code {flag} --file {file_path}` to see the member finding."
            ),
            None => format!(
                "'{owner_name}' is used by {refs} file(s); '{member_name}' is credited through \
                 its namespace export."
            ),
        }
    };
    format!("{head}{body}")
}

fn export_name_matches(export: &crate::graph::ExportSymbol, export_name: &str) -> bool {
    let name_str = export.name.to_string();
    name_str == export_name || (export_name == "default" && name_str == "default")
}

/// Map a module's exports to [`TracedExport`] entries with relativized references.
fn traced_exports(
    graph: &ModuleGraph,
    root: &Path,
    module: &crate::graph::ModuleNode,
) -> Vec<TracedExport> {
    module
        .exports
        .iter()
        .map(|e| TracedExport {
            name: e.name.to_string(),
            is_type_only: e.is_type_only,
            reference_count: e.references.len(),
            referenced_by: e
                .references
                .iter()
                .map(|r| reference_to_export_reference(graph, root, r))
                .collect(),
        })
        .collect()
}

/// Collect the root-relative paths a file imports from (forward graph edges).
fn traced_imports_from(
    graph: &ModuleGraph,
    root: &Path,
    module: &crate::graph::ModuleNode,
) -> Vec<PathBuf> {
    graph
        .edges_for(module.file_id)
        .iter()
        .filter_map(|target_id| {
            graph
                .modules
                .get(target_id.0 as usize)
                .map(|m| m.path.strip_prefix(root).unwrap_or(&m.path).to_path_buf())
        })
        .collect()
}

/// Collect the root-relative paths that import a file (reverse graph edges).
fn traced_imported_by(
    graph: &ModuleGraph,
    root: &Path,
    module: &crate::graph::ModuleNode,
) -> Vec<PathBuf> {
    graph
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
        .unwrap_or_default()
}

/// Map a module's re-exports to [`TracedReExport`] entries with relativized source paths.
fn traced_re_exports(
    graph: &ModuleGraph,
    root: &Path,
    module: &crate::graph::ModuleNode,
) -> Vec<TracedReExport> {
    module
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
        .collect()
}

/// Trace all edges for a file.
#[must_use]
pub fn trace_file(graph: &ModuleGraph, root: &Path, file_path: &str) -> Option<FileTrace> {
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    Some(FileTrace {
        file: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        is_reachable: module.is_reachable(),
        is_entry_point: module.is_entry_point(),
        exports: traced_exports(graph, root, module),
        imports_from: traced_imports_from(graph, root, module),
        imported_by: traced_imported_by(graph, root, module),
        re_exports: traced_re_exports(graph, root, module),
    })
}

/// Trace where a dependency is used.
///
/// `script_used_packages` carries the package names recorded as binary invocations
/// in package.json scripts (`build: microbundle ...`) and CI configs
/// (`.github/workflows/*.yml`, `.gitlab-ci.yml`). The same set the unused-deps
/// detector consults; passing it in lets the trace output match the detector's
/// view of "used" instead of reporting `is_used=false` for tools invoked only
/// through scripts.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
#[must_use]
pub fn trace_dependency(
    graph: &ModuleGraph,
    root: &Path,
    package_name: &str,
    script_used_packages: &FxHashSet<String>,
) -> DependencyTrace {
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
    let used_in_scripts = script_used_packages.contains(package_name);
    DependencyTrace {
        package_name: package_name.to_string(),
        imported_by,
        type_only_imported_by,
        used_in_scripts,
        is_used: import_count > 0 || used_in_scripts,
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

/// Compute the impact closure for a single file as the seed.
///
/// Resolves `file_path` to a graph `FileId`, walks `reverse_deps` + re-export
/// chains to the transitive affected set, and reports the coordination gap (the
/// seed's exported contracts consumed by modules outside the seed). Returns
/// `None` when the file is not in the module graph.
#[must_use]
pub fn trace_impact_closure(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
) -> Option<ImpactClosureTrace> {
    let module = graph
        .modules
        .iter()
        .find(|m| path_matches(&m.path, root, file_path))?;

    let closure = graph.impact_closure(&[module.file_id]);
    let paths = graph.closure_with_paths(&closure, root);

    let seed = paths
        .in_diff
        .first()
        .cloned()
        .unwrap_or_else(|| file_path.replace('\\', "/"));

    let coordination_gap = paths
        .coordination_gap
        .into_iter()
        .map(|gap| ImpactClosureGap {
            consumer_file: gap.consumer_file,
            consumed_symbols: gap.consumed_symbols,
            note: "syntactic attention pointer, not a correctness proof".to_string(),
        })
        .collect();

    Some(ImpactClosureTrace {
        seed,
        affected_not_shown: paths.affected_not_shown,
        coordination_gap,
    })
}

/// Build a [`TracedCloneGroup`] from a raw clone group, computing the
/// fingerprint, group-level suggestion, and dominant-identifier name and
/// relativizing every instance path against `root`.
fn build_traced_group(
    group: &CloneGroup,
    root: &Path,
    fingerprints: &CloneFingerprintSet,
) -> TracedCloneGroup {
    TracedCloneGroup {
        fingerprint: fingerprints.fingerprint_for_group(group),
        token_count: group.token_count,
        line_count: group.line_count,
        instances: group
            .instances
            .iter()
            .map(|inst| relativize_instance(inst, root))
            .collect(),
        suggestion: group_refactoring_suggestion(group),
        suggested_name: dominant_identifier(group),
    }
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
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);

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
            clone_groups.push(build_traced_group(group, root, &fingerprints));
        }
    }

    CloneTrace {
        file: PathBuf::from(file_path),
        line,
        matched_instance,
        clone_groups,
    }
}

/// Trace a clone group by its stable content fingerprint.
///
/// Fingerprints are usually `dup:<8hex>` and widen only when needed to avoid a
/// collision inside the same report.
///
/// Returns a [`CloneTrace`] whose single `clone_groups` entry is the matched
/// group and whose `file` / `line` / `matched_instance` come from that group's
/// representative (first) instance. `matched_instance` is `None` (and
/// `clone_groups` empty) when no group matches the fingerprint.
#[must_use]
pub fn trace_clone_by_fingerprint(
    report: &DuplicationReport,
    root: &Path,
    fingerprint: &str,
) -> CloneTrace {
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    let matched = fingerprints.find_group(&report.clone_groups, fingerprint);

    let Some(group) = matched else {
        return CloneTrace {
            file: PathBuf::new(),
            line: 0,
            matched_instance: None,
            clone_groups: Vec::new(),
        };
    };

    let representative = group
        .instances
        .first()
        .map(|inst| relativize_instance(inst, root));
    let (file, line) = representative.as_ref().map_or_else(
        || (PathBuf::new(), 0),
        |inst| (inst.file.clone(), inst.start_line),
    );

    CloneTrace {
        file,
        line,
        matched_instance: representative,
        clone_groups: vec![build_traced_group(group, root, &fingerprints)],
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
                        from_style: false,
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
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    ExportInfo {
                        name: ExportName::Named("bar".to_string()),
                        local_name: Some("bar".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(21, 40),
                        members: vec![],
                        is_side_effect_used: false,
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 15),
                    members: vec![],
                    is_side_effect_used: false,
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

    fn build_class_member_graph() -> ModuleGraph {
        use fallow_types::extract::{MemberInfo, MemberKind};

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let method = |name: &str| MemberInfo {
            name: name.to_string(),
            kind: MemberKind::ClassMethod,
            span: oxc_span::Span::new(0, 4),
            has_decorator: false,
            decorator_names: vec![],
            is_instance_returning_static: false,
            is_self_returning: false,
        };
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./controller".to_string(),
                        imported_name: ImportedName::Named("Ctrl".to_string()),
                        local_name: "Ctrl".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                exports: vec![ExportInfo {
                    name: ExportName::Named("Ctrl".to_string()),
                    local_name: Some("Ctrl".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![method("used"), method("dead")],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];
        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn trace_class_member_reports_owner_class() {
        // #1744: `--trace FILE:MEMBER` on a class member reports the owning
        // class instead of erroring "export not found".
        let graph = build_class_member_graph();
        let root = Path::new("/project");

        let trace = trace_class_member(&graph, root, "src/controller.ts", "dead").unwrap();
        assert_eq!(trace.owner_export, "Ctrl");
        assert_eq!(trace.member_name, "dead");
        assert_eq!(trace.member_kind, "class-method");
        assert!(trace.owner_is_used);
        assert!(trace.owner_file_reachable);
        assert_eq!(trace.owner_direct_references.len(), 1);
        assert!(
            trace.reason.contains("--unused-class-members"),
            "reason should point at the member command: {}",
            trace.reason
        );
    }

    #[test]
    fn trace_class_member_absent_name_is_none() {
        // A name that is neither a top-level export nor a declared member falls
        // through so the caller emits the "not found" error.
        let graph = build_class_member_graph();
        let root = Path::new("/project");
        assert!(trace_class_member(&graph, root, "src/controller.ts", "nope").is_none());
    }

    /// Build a graph where the controller declaring `Ctrl` is NOT imported by
    /// the entry, so its file is unreachable and every member is dead.
    fn build_unreachable_class_member_graph() -> ModuleGraph {
        use fallow_types::extract::{MemberInfo, MemberKind};

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let method = |name: &str| MemberInfo {
            name: name.to_string(),
            kind: MemberKind::ClassMethod,
            span: oxc_span::Span::new(0, 4),
            has_decorator: false,
            decorator_names: vec![],
            is_instance_returning_static: false,
            is_self_returning: false,
        };
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                // Entry imports nothing, so controller.ts is unreachable.
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                exports: vec![ExportInfo {
                    name: ExportName::Named("Ctrl".to_string()),
                    local_name: Some("Ctrl".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![method("dead")],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];
        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    #[test]
    fn trace_class_member_unreachable_owner_reports_dead_reason() {
        // `!file_reachable` branch: the owning file is not reachable from any
        // entry point, so the reason states the class and its members are dead.
        let graph = build_unreachable_class_member_graph();
        let root = Path::new("/project");

        let trace = trace_class_member(&graph, root, "src/controller.ts", "dead").unwrap();
        assert!(!trace.owner_file_reachable);
        assert!(
            trace.reason.contains("not reachable"),
            "unreachable owner reason should say so: {}",
            trace.reason
        );
        // The unreachable branch does not point at a member command (the file is
        // dead wholesale via the unused-file finding).
        assert!(!trace.reason.contains("--unused-class-members"));
    }

    #[test]
    fn trace_class_member_prefers_used_owner_on_name_collision() {
        // Two exports declare a member of the same name; the tie-break in
        // `max_by_key` must prefer the used, non-type-only owner so the trace
        // reports the reachable class rather than a type-only shadow.
        use fallow_types::extract::{MemberInfo, MemberKind};

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/project/src/entry.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let method = |name: &str| MemberInfo {
            name: name.to_string(),
            kind: MemberKind::ClassMethod,
            span: oxc_span::Span::new(0, 4),
            has_decorator: false,
            decorator_names: vec![],
            is_instance_returning_static: false,
            is_self_returning: false,
        };
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./controller".to_string(),
                        imported_name: ImportedName::Named("UsedCtrl".to_string()),
                        local_name: "UsedCtrl".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/project/src/controller.ts"),
                exports: vec![
                    // Type-only, unreferenced owner declared FIRST: must lose the
                    // tie-break to the used, non-type-only owner below.
                    ExportInfo {
                        name: ExportName::Named("TypeCtrl".to_string()),
                        local_name: Some("TypeCtrl".to_string()),
                        is_type_only: true,
                        visibility: VisibilityTag::None,
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![method("shared")],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    ExportInfo {
                        name: ExportName::Named("UsedCtrl".to_string()),
                        local_name: Some("UsedCtrl".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![method("shared")],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let root = Path::new("/project");

        let trace = trace_class_member(&graph, root, "src/controller.ts", "shared").unwrap();
        assert_eq!(
            trace.owner_export, "UsedCtrl",
            "tie-break must prefer the used, non-type-only owner"
        );
        assert!(trace.owner_is_used);
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
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("lodash".to_string()),
            }],
            ..Default::default()
        }];

        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let root = Path::new("/project");

        let trace = trace_dependency(&graph, root, "lodash", &FxHashSet::default());
        assert!(trace.is_used);
        assert!(!trace.used_in_scripts);
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

        let trace = trace_dependency(&graph, root, "nonexistent-pkg", &FxHashSet::default());
        assert!(!trace.is_used);
        assert!(!trace.used_in_scripts);
        assert_eq!(trace.import_count, 0);
        assert!(trace.imported_by.is_empty());
    }

    #[test]
    fn trace_dependency_used_only_in_scripts() {
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
        let mut script_used = FxHashSet::default();
        script_used.insert("microbundle".to_string());

        let trace = trace_dependency(&graph, root, "microbundle", &script_used);
        assert!(
            trace.is_used,
            "is_used must be true when the package is referenced from package.json scripts"
        );
        assert!(trace.used_in_scripts);
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
                clone_groups_below_min_occurrences: 0,
            },
        };
        let trace = trace_clone(&report, Path::new("/project"), "src/a.ts", 15);
        assert!(trace.matched_instance.is_some());
        assert_eq!(trace.clone_groups.len(), 1);
        assert_eq!(trace.clone_groups[0].instances.len(), 2);
        assert!(trace.clone_groups[0].fingerprint.starts_with("dup:"));
        assert_eq!(trace.clone_groups[0].suggestion.estimated_savings, 11);
    }

    #[test]
    fn trace_clone_by_fingerprint_resolves_and_misses() {
        use crate::duplicates::{
            CloneGroup, CloneInstance, DuplicationReport, DuplicationStats, clone_fingerprint,
        };
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: PathBuf::from("/project/src/a.ts"),
                        start_line: 10,
                        end_line: 20,
                        start_col: 0,
                        end_col: 0,
                        fragment: "fn buildInvoice() {}".to_string(),
                    },
                    CloneInstance {
                        file: PathBuf::from("/project/src/b.ts"),
                        start_line: 5,
                        end_line: 15,
                        start_col: 0,
                        end_col: 0,
                        fragment: "fn buildInvoice() {}".to_string(),
                    },
                ],
                token_count: 60,
                line_count: 11,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };
        let fp = clone_fingerprint(&report.clone_groups[0].instances);

        let hit = trace_clone_by_fingerprint(&report, Path::new("/project"), &fp);
        assert!(hit.matched_instance.is_some());
        assert_eq!(hit.clone_groups.len(), 1);
        assert_eq!(hit.clone_groups[0].fingerprint, fp);
        assert_eq!(hit.line, 10);

        let miss = trace_clone_by_fingerprint(&report, Path::new("/project"), "dup:deadbeef");
        assert!(miss.matched_instance.is_none());
        assert!(miss.clone_groups.is_empty());
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
                clone_groups_below_min_occurrences: 0,
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
                clone_groups_below_min_occurrences: 0,
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
                clone_groups_below_min_occurrences: 0,
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

    /// Regression for the MCP e2e `trace_export` / `trace_file` Windows
    /// failures: the MCP layer passes forward-slashed user input
    /// (`src/utils.ts`) but `module_path` on Windows uses backslash
    /// separators (`D:\a\fallow\...\src\utils.ts`). The byte-level
    /// equality check missed every match. The helper now normalises
    /// both sides to forward slashes before comparing.
    #[test]
    fn path_matches_normalises_windows_module_path_against_posix_user_path() {
        let root = Path::new(r"D:\a\fallow\fallow\tests\fixtures\basic-project");
        let module_path =
            PathBuf::from(r"D:\a\fallow\fallow\tests\fixtures\basic-project\src\utils.ts");
        assert!(path_matches(&module_path, root, "src/utils.ts"));
        assert!(path_matches(&module_path, root, r"src\utils.ts"));
    }

    #[test]
    fn path_matches_ends_with_fallback_handles_mixed_separators() {
        let root = Path::new("/some/other/root");
        let module_path =
            PathBuf::from(r"D:\a\fallow\fallow\tests\fixtures\basic-project\src\utils.ts");
        assert!(path_matches(&module_path, root, "src/utils.ts"));
    }

    /// Regression for the MCP e2e trace_export / trace_file failures: even
    /// after `path_matches` correctly identified the file on Windows, the
    /// trace output struct's `file: PathBuf` field serialized the stored
    /// backslash-shaped path verbatim. JSON consumers (MCP agents, CI
    /// pipelines, the cross-platform trace_file assertion in
    /// `e2e_trace_file_returns_json`) expect forward-slash. Pin the
    /// contract via raw-string Windows-shaped `PathBuf::from` so the test
    /// runs cross-platform.
    #[test]
    fn export_trace_serializes_windows_path_with_forward_slashes() {
        let trace = ExportTrace {
            file: PathBuf::from(r"src\utils.ts"),
            export_name: "foo".to_string(),
            file_reachable: true,
            is_entry_point: false,
            is_used: true,
            direct_references: vec![ExportReference {
                from_file: PathBuf::from(r"src\entry.ts"),
                kind: "named import".to_string(),
            }],
            re_export_chains: vec![ReExportChain {
                barrel_file: PathBuf::from(r"src\index.ts"),
                exported_as: "foo".to_string(),
                reference_count: 1,
            }],
            reason: "ok".to_string(),
        };
        let json = serde_json::to_string(&trace).expect("serializes");
        assert!(
            json.contains("\"file\":\"src/utils.ts\""),
            "ExportTrace.file must serialize with forward slashes: {json}"
        );
        assert!(
            json.contains("\"from_file\":\"src/entry.ts\""),
            "ExportReference.from_file must serialize with forward slashes: {json}"
        );
        assert!(
            json.contains("\"barrel_file\":\"src/index.ts\""),
            "ReExportChain.barrel_file must serialize with forward slashes: {json}"
        );
        assert!(
            !json.contains(r"\\"),
            "no backslash sequence should remain anywhere in the JSON: {json}"
        );
    }

    #[test]
    fn file_trace_serializes_windows_paths_with_forward_slashes() {
        let trace = FileTrace {
            file: PathBuf::from(r"src\utils.ts"),
            is_reachable: true,
            is_entry_point: false,
            exports: vec![],
            imports_from: vec![PathBuf::from(r"src\helpers.ts")],
            imported_by: vec![PathBuf::from(r"src\entry.ts")],
            re_exports: vec![TracedReExport {
                source_file: PathBuf::from(r"src\source.ts"),
                imported_name: "foo".to_string(),
                exported_name: "foo".to_string(),
            }],
        };
        let json = serde_json::to_string(&trace).expect("serializes");
        assert!(json.contains("\"file\":\"src/utils.ts\""), "got {json}");
        assert!(
            json.contains("\"imports_from\":[\"src/helpers.ts\"]"),
            "got {json}"
        );
        assert!(
            json.contains("\"imported_by\":[\"src/entry.ts\"]"),
            "got {json}"
        );
        assert!(
            json.contains("\"source_file\":\"src/source.ts\""),
            "got {json}"
        );
        assert!(!json.contains(r"\\"), "no backslash should remain: {json}");
    }
}
