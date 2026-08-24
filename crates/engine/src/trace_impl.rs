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
use crate::graph::{EffectiveExportResolution, ExportNamespace, ModuleGraph, ReferenceKind};

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

/// Project graph-owned effective routes into the public trace contract.
fn collect_re_export_chains(
    graph: &ModuleGraph,
    root: &Path,
    target_file_id: crate::discover::FileId,
    export_name: &str,
    namespace: ExportNamespace,
) -> Vec<ReExportChain> {
    graph
        .effective_re_export_routes(target_file_id, export_name, namespace)
        .into_iter()
        .filter_map(|route| {
            let module = graph.modules.get(route.barrel_file().0 as usize)?;
            Some(ReExportChain {
                barrel_file: module
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&module.path)
                    .to_path_buf(),
                exported_as: route.exported_name().to_string(),
                reference_count: graph
                    .effective_export_surface_references(
                        route.barrel_file(),
                        route.exported_name(),
                        namespace,
                    )
                    .len(),
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

    let surface = select_export(graph, module, export_name)?;
    let (namespace, direct_references) =
        crediting_export_references(graph, root, module.file_id, export_name, surface);

    let re_export_chains =
        collect_re_export_chains(graph, root, module.file_id, export_name, namespace);

    let reference_count = direct_references.len();
    let is_used = reference_count > 0;
    let reason = export_trace_reason(module, reference_count, is_used, &re_export_chains);

    Some(ExportTrace {
        file: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        export_name: export_name.to_string(),
        namespace: match namespace {
            ExportNamespace::Type => fallow_types::semantic::SemanticNamespace::Type,
            ExportNamespace::Value => fallow_types::semantic::SemanticNamespace::Value,
        },
        file_reachable: module.is_reachable(),
        is_entry_point: module.is_entry_point(),
        is_used,
        direct_references,
        re_export_chains,
        reason,
        semantic: None,
    })
}

/// Distinct referencing files of one module export surface in one namespace.
fn direct_export_references(
    graph: &ModuleGraph,
    root: &Path,
    file_id: crate::discover::FileId,
    export_name: &str,
    namespace: ExportNamespace,
) -> Vec<ExportReference> {
    let mut referenced_files = FxHashSet::default();
    graph
        .effective_export_surface_references(file_id, export_name, namespace)
        .into_iter()
        .filter(|reference| referenced_files.insert(reference.from_file))
        .map(|r| reference_to_export_reference(graph, root, r))
        .collect()
}

/// References that credit the traced declaration, with the namespace that
/// carries them.
///
/// The preferred surface wins whenever its lane carries a reference. When it
/// carries none, the other namespace is consulted only if it resolves to the
/// same effective binding: that is the type lane falling back onto a
/// value-only declaration (`import type { helper }` of `export const helper`),
/// which the unused-export analyzer counts as a use regardless of namespace.
/// A distinct same-name declaration in the other namespace keeps the preferred
/// lane, because its references credit that other declaration and dead-code
/// still reports the traced one. See issue #2371.
fn crediting_export_references(
    graph: &ModuleGraph,
    root: &Path,
    file_id: crate::discover::FileId,
    export_name: &str,
    surface: crate::graph::EffectiveExportSurface<'_>,
) -> (ExportNamespace, Vec<ExportReference>) {
    let namespace = surface.namespace();
    let references = direct_export_references(graph, root, file_id, export_name, namespace);
    if !references.is_empty() {
        return (namespace, references);
    }
    let other = match namespace {
        ExportNamespace::Type => ExportNamespace::Value,
        ExportNamespace::Value => ExportNamespace::Type,
    };
    let same_binding = graph
        .effective_export_surface(file_id, export_name, other)
        .is_some_and(|candidate| candidate.binding() == surface.binding());
    if !same_binding {
        return (namespace, references);
    }
    let other_references = direct_export_references(graph, root, file_id, export_name, other);
    if other_references.is_empty() {
        return (namespace, references);
    }
    (other, other_references)
}

/// Resolve the exact source identity required by the semantic sidecar for a
/// graph export. This does not perform semantic analysis itself.
#[must_use]
pub fn semantic_symbol_for_export(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<fallow_types::semantic::SemanticSymbol> {
    use fallow_types::semantic::{SemanticNamespace, SemanticSymbol};

    let module = graph
        .modules
        .iter()
        .find(|module| path_matches(&module.path, root, file_path))?;
    let surface = select_export(graph, module, export_name)?;
    let namespace = surface.namespace();
    let (identity_module, span, identity_exported_name, local_name) = if let Some(re_export) =
        graph.effective_export_surface_re_export(module.file_id, export_name, namespace)
    {
        let local_name = if re_export.imported_name == "*" {
            export_name
        } else {
            re_export.imported_name.as_str()
        };
        (module, re_export.span, export_name, local_name)
    } else {
        let origin = surface.origin()?;
        let origin_module = graph.modules.get(origin.file_id().0 as usize)?;
        let origin_export = origin.export();
        let origin_name = match &origin_export.name {
            fallow_types::extract::ExportName::Named(name) => name.as_str(),
            fallow_types::extract::ExportName::Default => "default",
        };
        (origin_module, origin_export.span, origin_name, origin_name)
    };
    let source = std::fs::read_to_string(&identity_module.path).ok()?;
    let offsets = fallow_types::extract::compute_line_offsets(&source);
    let (line, col) = fallow_types::extract::byte_offset_to_line_col(&offsets, span.start);
    Some(SemanticSymbol {
        path: identity_module
            .path
            .strip_prefix(root)
            .unwrap_or(&identity_module.path)
            .to_path_buf(),
        namespace: match namespace {
            ExportNamespace::Type => SemanticNamespace::Type,
            ExportNamespace::Value => SemanticNamespace::Value,
        },
        declaration_kind: "export".to_string(),
        exported_name: identity_exported_name.to_string(),
        local_name: local_name.to_string(),
        owner: None,
        line,
        col,
    })
}

/// Resolve the source identity for a public class member semantic query.
#[must_use]
pub fn semantic_symbol_for_class_member(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    member_name: &str,
) -> Option<fallow_types::semantic::SemanticSymbol> {
    use fallow_types::extract::MemberKind;
    use fallow_types::semantic::{SemanticNamespace, SemanticSymbol};

    let module = graph
        .modules
        .iter()
        .find(|module| path_matches(&module.path, root, file_path))?;
    let (owner, member) = module
        .exports
        .iter()
        .filter_map(|export| {
            export
                .members
                .iter()
                .find(|member| member.name == member_name)
                .map(|member| (export, member))
        })
        .max_by_key(|(export, _)| (!export.references.is_empty(), !export.is_type_only))?;
    let declaration_kind = match member.kind {
        MemberKind::ClassMethod => "class_method",
        MemberKind::ClassProperty => "class_property",
        _ => return None,
    };
    let source = std::fs::read_to_string(&module.path).ok()?;
    let offsets = fallow_types::extract::compute_line_offsets(&source);
    let (line, col) = fallow_types::extract::byte_offset_to_line_col(&offsets, member.span.start);
    Some(SemanticSymbol {
        path: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        namespace: SemanticNamespace::Value,
        declaration_kind: declaration_kind.to_string(),
        exported_name: member_name.to_string(),
        local_name: member_name.to_string(),
        owner: Some(owner.name.to_string()),
        line,
        col,
    })
}

/// Stable reason why an exact class-method target cannot be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticClassMethodResolutionError {
    /// The requested file is not part of the retained module graph.
    FileNotFound,
    /// The requested owner or method does not exist in the file.
    SymbolNotFound,
    /// More than one declaration matches the exact owner and method.
    AmbiguousSymbol,
    /// The matching declaration is not a supported class method.
    UnsupportedSyntax,
}

impl std::fmt::Display for SemanticClassMethodResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reason = match self {
            Self::FileNotFound => "file-not-found",
            Self::SymbolNotFound => "unknown-symbol",
            Self::AmbiguousSymbol => "ambiguous-symbol",
            Self::UnsupportedSyntax => "unsupported-syntax",
        };
        formatter.write_str(reason)
    }
}

/// Resolve one exact exported class method without a name-based fallback.
pub fn semantic_symbol_for_exact_class_method(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    owner_name: &str,
    member_name: &str,
) -> Result<fallow_types::semantic::SemanticSymbol, SemanticClassMethodResolutionError> {
    use fallow_types::extract::MemberKind;
    use fallow_types::semantic::{SemanticNamespace, SemanticSymbol};

    let module = graph
        .modules
        .iter()
        .find(|module| path_matches(&module.path, root, file_path))
        .ok_or(SemanticClassMethodResolutionError::FileNotFound)?;
    let owners = module
        .exports
        .iter()
        .filter(|export| export.name.matches_str(owner_name))
        .collect::<Vec<_>>();
    if owners.len() != 1 {
        return Err(if owners.is_empty() {
            SemanticClassMethodResolutionError::SymbolNotFound
        } else {
            SemanticClassMethodResolutionError::AmbiguousSymbol
        });
    }
    let owner = owners[0];
    let members = owner
        .members
        .iter()
        .filter(|member| member.name == member_name)
        .collect::<Vec<_>>();
    if members.len() != 1 {
        return Err(if members.is_empty() {
            SemanticClassMethodResolutionError::SymbolNotFound
        } else {
            SemanticClassMethodResolutionError::AmbiguousSymbol
        });
    }
    let member = members[0];
    if member.kind != MemberKind::ClassMethod {
        return Err(SemanticClassMethodResolutionError::UnsupportedSyntax);
    }
    let source = std::fs::read_to_string(&module.path)
        .map_err(|_| SemanticClassMethodResolutionError::SymbolNotFound)?;
    let offsets = fallow_types::extract::compute_line_offsets(&source);
    let (line, col) = fallow_types::extract::byte_offset_to_line_col(&offsets, member.span.start);
    Ok(SemanticSymbol {
        path: module
            .path
            .strip_prefix(root)
            .unwrap_or(&module.path)
            .to_path_buf(),
        namespace: SemanticNamespace::Value,
        declaration_kind: "class_method".to_string(),
        exported_name: member_name.to_string(),
        local_name: member_name.to_string(),
        owner: Some(owner_name.to_string()),
        line,
        col,
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
        owner_namespace: owner_trace.namespace,
        owner_is_used: owner_trace.is_used,
        owner_file_reachable: owner_trace.file_reachable,
        owner_is_entry_point: owner_trace.is_entry_point,
        owner_direct_references: owner_trace.direct_references,
        owner_re_export_chains: owner_trace.re_export_chains,
        reason,
        semantic: None,
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

fn select_export<'graph>(
    graph: &'graph ModuleGraph,
    module: &'graph crate::graph::ModuleNode,
    export_name: &str,
) -> Option<crate::graph::EffectiveExportSurface<'graph>> {
    [ExportNamespace::Value, ExportNamespace::Type]
        .into_iter()
        .find_map(|namespace| {
            graph.effective_export_surface(module.file_id, export_name, namespace)
        })
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
        .map(|e| {
            let referenced_by: Vec<_> = e
                .physical_references()
                .map(|r| reference_to_export_reference(graph, root, r))
                .collect();
            TracedExport {
                name: e.name.to_string(),
                is_type_only: e.is_type_only,
                reference_count: referenced_by.len(),
                referenced_by,
            }
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
        spread: group.spread(),
        similarity: group.similarity,
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
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
    use fallow_types::extract::ReExportInfo;

    fn resolved_re_export(
        source: FileId,
        imported_name: &str,
        exported_name: &str,
    ) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: "./source".to_string(),
                imported_name: imported_name.to_string(),
                exported_name: exported_name.to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source),
        }
    }

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
                        is_type_only_star: false,
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
                ]
                .into(),
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
                }]
                .into(),
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
        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Value,
            "an unreferenced value export stays in the value namespace"
        );
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
    fn trace_reports_only_the_effective_re_export_origin() {
        let files: Vec<_> = ["entry", "barrel", "star-source", "explicit-source"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| DiscoveredFile {
                id: FileId(index as u32),
                path: PathBuf::from(format!("/project/src/{name}.ts")),
                size_bytes: 10,
            })
            .collect();
        let entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        let re_export = |source: FileId, imported: &str, exported: &str| ResolvedReExport {
            info: ReExportInfo {
                source: format!("./{}", source.0),
                imported_name: imported.to_string(),
                exported_name: exported.to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source),
        };
        let export = || ExportInfo {
            name: ExportName::Named("foo".to_string()),
            local_name: Some("foo".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 3),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        };
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./barrel".to_string(),
                        imported_name: ImportedName::Named("foo".to_string()),
                        local_name: "foo".to_string(),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::default(),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                re_exports: vec![
                    re_export(FileId(2), "*", "*"),
                    re_export(FileId(3), "foo", "foo"),
                ],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![export()].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                exports: vec![export()].into(),
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let shadowed = trace_export(&graph, Path::new("/project"), "src/star-source.ts", "foo")
            .expect("shadowed source export exists");
        let effective = trace_export(
            &graph,
            Path::new("/project"),
            "src/explicit-source.ts",
            "foo",
        )
        .expect("effective source export exists");

        assert!(shadowed.re_export_chains.is_empty());
        assert_eq!(effective.re_export_chains.len(), 1);
        assert_eq!(effective.re_export_chains[0].exported_as, "foo");
    }

    fn star_surface_trace_graph(root: &Path) -> ModuleGraph {
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create source directory");
        let paths: Vec<_> = ["source", "barrel-a", "barrel-b", "outer", "entry"]
            .into_iter()
            .map(|name| src.join(format!("{name}.ts")))
            .collect();
        std::fs::write(&paths[0], "\n\nexport const foo = 1;\n").expect("write source");
        for path in &paths[1..] {
            std::fs::write(path, "export {};\n").expect("write module");
        }
        let files: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(index as u32),
                path: path.clone(),
                size_bytes: 20,
            })
            .collect();
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: paths[0].clone(),
                exports: vec![ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(2, 5),
                    members: Vec::new(),
                    is_side_effect_used: false,
                    super_class: None,
                }]
                .into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: paths[1].clone(),
                re_exports: vec![resolved_re_export(FileId(0), "*", "*")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: paths[2].clone(),
                re_exports: vec![resolved_re_export(FileId(0), "*", "*")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: paths[3].clone(),
                re_exports: vec![
                    resolved_re_export(FileId(1), "foo", "left"),
                    resolved_re_export(FileId(1), "foo", "right"),
                ],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(4),
                path: paths[4].clone(),
                resolved_imports: vec![
                    ResolvedImport {
                        info: ImportInfo {
                            source: "./outer".to_string(),
                            imported_name: ImportedName::Named("left".to_string()),
                            local_name: "left".to_string(),
                            is_type_only: false,
                            is_type_only_star: false,
                            from_style: false,
                            span: oxc_span::Span::new(10, 20),
                            source_span: oxc_span::Span::default(),
                        },
                        target: ResolveResult::InternalModule(FileId(3)),
                    },
                    ResolvedImport {
                        info: ImportInfo {
                            source: "./barrel-b".to_string(),
                            imported_name: ImportedName::Named("foo".to_string()),
                            local_name: "otherFoo".to_string(),
                            is_type_only: false,
                            is_type_only_star: false,
                            from_style: false,
                            span: oxc_span::Span::new(30, 40),
                            source_span: oxc_span::Span::default(),
                        },
                        target: ResolveResult::InternalModule(FileId(2)),
                    },
                ],
                ..Default::default()
            },
        ];
        let entry_points = vec![EntryPoint {
            path: paths[4].clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn star_surface_trace_keeps_aliases_separate_and_uses_origin_identity() {
        let root = tempfile::tempdir().expect("temporary project");
        let graph = star_surface_trace_graph(root.path());

        let used = trace_export(&graph, root.path(), "src/barrel-a.ts", "foo")
            .expect("aliased barrel exposes foo");
        let sibling = trace_export(&graph, root.path(), "src/barrel-b.ts", "foo")
            .expect("sibling barrel exposes foo");
        assert!(used.is_used);
        assert_eq!(used.direct_references.len(), 1);
        assert!(sibling.is_used);
        assert_eq!(sibling.direct_references.len(), 1);

        let source_trace = trace_export(&graph, root.path(), "src/source.ts", "foo")
            .expect("source declaration is traceable");
        let chain_count = |file: &str, name: &str| {
            source_trace
                .re_export_chains
                .iter()
                .find(|chain| chain.barrel_file == Path::new(file) && chain.exported_as == name)
                .map(|chain| chain.reference_count)
        };
        assert_eq!(chain_count("src/barrel-a.ts", "foo"), Some(1));
        assert_eq!(chain_count("src/barrel-b.ts", "foo"), Some(1));
        assert_eq!(chain_count("src/outer.ts", "left"), Some(1));
        assert_eq!(chain_count("src/outer.ts", "right"), Some(0));

        let semantic = semantic_symbol_for_export(&graph, root.path(), "src/barrel-a.ts", "foo")
            .expect("star surface resolves to its declaration identity");
        assert_eq!(semantic.path, Path::new("src/source.ts"));
        assert_eq!(semantic.exported_name, "foo");
        assert_eq!(semantic.local_name, "foo");
        assert_eq!((semantic.line, semantic.col), (3, 0));

        let alias = semantic_symbol_for_export(&graph, root.path(), "src/outer.ts", "left")
            .expect("named re-export keeps its export-specifier identity");
        assert_eq!(alias.path, Path::new("src/outer.ts"));
        assert_eq!(alias.exported_name, "left");
        assert_eq!(alias.local_name, "foo");
    }

    #[test]
    fn trace_follows_renamed_and_convergent_re_export_routes() {
        let names = [
            "source",
            "renamed",
            "final",
            "left",
            "right",
            "diamond-entry",
        ];
        let files: Vec<_> = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| DiscoveredFile {
                id: FileId(index as u32),
                path: PathBuf::from(format!("/project/src/{name}.ts")),
                size_bytes: 10,
            })
            .collect();
        let re_export = |source: FileId, imported: &str, exported: &str| ResolvedReExport {
            info: ReExportInfo {
                source: format!("./{}", source.0),
                imported_name: imported.to_string(),
                exported_name: exported.to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source),
        };
        let mut resolved: Vec<_> = files
            .iter()
            .map(|file| ResolvedModule {
                file_id: file.id,
                path: file.path.clone(),
                ..Default::default()
            })
            .collect();
        resolved[0].exports = vec![ExportInfo {
            name: ExportName::Named("foo".to_string()),
            local_name: Some("foo".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 3),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        }]
        .into();
        resolved[1].re_exports = vec![re_export(FileId(0), "foo", "bar")];
        resolved[2].re_exports = vec![re_export(FileId(1), "bar", "baz")];
        resolved[3].re_exports = vec![re_export(FileId(0), "*", "*")];
        resolved[4].re_exports = vec![re_export(FileId(0), "*", "*")];
        resolved[5].re_exports = vec![
            re_export(FileId(3), "*", "*"),
            re_export(FileId(4), "*", "*"),
        ];
        let entry_points = vec![EntryPoint {
            path: files[5].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "foo")
            .expect("source export exists");
        let routes: FxHashSet<_> = trace
            .re_export_chains
            .iter()
            .map(|route| (route.barrel_file.as_path(), route.exported_as.as_str()))
            .collect();

        assert_eq!(routes.len(), 5);
        assert!(routes.contains(&(Path::new("src/renamed.ts"), "bar")));
        assert!(routes.contains(&(Path::new("src/final.ts"), "baz")));
        assert!(routes.contains(&(Path::new("src/left.ts"), "foo")));
        assert!(routes.contains(&(Path::new("src/right.ts"), "foo")));
        assert!(routes.contains(&(Path::new("src/diamond-entry.ts"), "foo")));
    }

    #[test]
    fn trace_prefers_the_value_namespace_independent_of_usage() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 10,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/source.ts"),
                size_bytes: 10,
            },
        ];
        let export = |is_type_only| ExportInfo {
            name: ExportName::Named("Foo".to_string()),
            local_name: Some("Foo".to_string()),
            is_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 3),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        };
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./source".to_string(),
                        imported_name: ImportedName::Named("Foo".to_string()),
                        local_name: "Foo".to_string(),
                        is_type_only: true,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::default(),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: vec![export(false), export(true)].into(),
                ..Default::default()
            },
        ];
        let entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "Foo")
            .expect("value export exists");

        // The type import credits the distinct `export type Foo` declaration,
        // so dead-code still reports the value `Foo`; the trace must agree and
        // must not borrow the type lane here (issue #2371). A declaration
        // merge that splits across lanes, `interface Foo` next to `class Foo`,
        // reaches the graph as this same pair of surfaces, so it is the shape
        // the documented gap names: dead-code credits the class through the
        // merge while the trace still reports it unused.
        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Value
        );
        assert!(!trace.is_used, "type usage must not select the type export");
    }

    /// Consumer `entry.ts` importing `Foo` from `source.ts`, whose exports are
    /// supplied by the caller.
    fn source_consumer_graph(
        source_exports: Vec<ExportInfo>,
        import_is_type_only: bool,
        classified_usage: bool,
    ) -> ModuleGraph {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 10,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/source.ts"),
                size_bytes: 10,
            },
        ];
        let classified = if classified_usage {
            vec!["Foo".to_string()]
        } else {
            Vec::new()
        };
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./source".to_string(),
                        imported_name: ImportedName::Named("Foo".to_string()),
                        local_name: "Foo".to_string(),
                        is_type_only: import_is_type_only,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                type_referenced_import_bindings: classified.clone(),
                value_referenced_import_bindings: classified,
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: source_exports.into(),
                ..Default::default()
            },
        ];
        let entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    /// Two consumers of `source.ts`: `entry.ts` imports `Foo` in value
    /// position and `typed.ts` imports the same name with `import type`, so
    /// one effective binding carries a reference in both lanes.
    fn dual_lane_consumer_graph(source_exports: Vec<ExportInfo>) -> ModuleGraph {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/src/entry.ts"),
                size_bytes: 10,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/src/source.ts"),
                size_bytes: 10,
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/src/typed.ts"),
                size_bytes: 10,
            },
        ];
        let consumer = |file_id: FileId, path: PathBuf, is_type_only: bool| ResolvedModule {
            file_id,
            path,
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./source".to_string(),
                    imported_name: ImportedName::Named("Foo".to_string()),
                    local_name: "Foo".to_string(),
                    is_type_only,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..Default::default()
        };
        let resolved = vec![
            consumer(FileId(0), files[0].path.clone(), false),
            ResolvedModule {
                file_id: FileId(1),
                path: files[1].path.clone(),
                exports: source_exports.into(),
                ..Default::default()
            },
            consumer(FileId(2), files[2].path.clone(), true),
        ];
        let entry_points = vec![
            EntryPoint {
                path: files[0].path.clone(),
                source: EntryPointSource::PackageJsonMain,
            },
            EntryPoint {
                path: files[2].path.clone(),
                source: EntryPointSource::PackageJsonMain,
            },
        ];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    fn named_foo_export(is_type_only: bool) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named("Foo".to_string()),
            local_name: Some("Foo".to_string()),
            is_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 3),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        }
    }

    #[test]
    fn trace_credits_a_value_only_export_through_the_type_lane() {
        // Issue #2371: `import type { Foo }` of `export const Foo` lands on the
        // value declaration through the type-lane fallback, and dead-code
        // counts that as a use. The trace reports the crediting lane.
        let graph = source_consumer_graph(vec![named_foo_export(false)], true, false);

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "Foo")
            .expect("value export exists");

        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Type,
            "the type lane carries the only credit"
        );
        assert!(
            trace.is_used,
            "a type-only import credits a value-only export"
        );
        assert_eq!(trace.direct_references.len(), 1);
        assert_eq!(
            trace.direct_references[0].from_file,
            PathBuf::from("src/entry.ts")
        );
        assert_eq!(trace.direct_references[0].kind, "named import");
        assert_eq!(trace.reason, "Used by 1 file(s)");
    }

    #[test]
    fn trace_keeps_the_value_lane_when_one_binding_carries_both_lanes() {
        // The preferred lane wins whenever it carries a reference, including
        // when the other lane resolves to the SAME binding and carries one
        // too: `export class Foo` consumed by a value importer and by an
        // `import type` importer must keep reporting the value consumer.
        // Without that rule the type lane would take over the payload.
        let graph = dual_lane_consumer_graph(vec![named_foo_export(false)]);

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "Foo")
            .expect("value export exists");

        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Value
        );
        assert!(trace.is_used);
        assert_eq!(trace.direct_references.len(), 1);
        assert_eq!(
            trace.direct_references[0].from_file,
            PathBuf::from("src/entry.ts"),
            "the value consumer stays the listed reference"
        );
    }

    #[test]
    fn trace_credits_a_declaration_merge_that_stays_one_binding() {
        // A merge whose parts share the value lane, `class Foo` next to
        // `namespace Foo`, is one effective binding in both lanes, so a bound
        // `import type` credits it and the trace reports the crediting lane.
        let graph = source_consumer_graph(
            vec![named_foo_export(false), named_foo_export(false)],
            true,
            false,
        );

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "Foo")
            .expect("value export exists");

        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Type,
            "the merged binding is reachable from the type lane"
        );
        assert!(trace.is_used);
        assert_eq!(trace.direct_references.len(), 1);
        assert_eq!(
            trace.direct_references[0].from_file,
            PathBuf::from("src/entry.ts")
        );
    }

    #[test]
    fn trace_keeps_the_value_namespace_when_lanes_hold_distinct_bindings() {
        // Deliberate negative control: two same-name declarations in opposite
        // lanes are two bindings, so the value lane is kept even though the
        // type lane also carries a reference. The value lane holds the
        // reference here, so this pins the preferred-lane rule;
        // `trace_prefers_the_value_namespace_independent_of_usage` is the test
        // that reaches and pins the binding-equality guard.
        let graph = source_consumer_graph(
            vec![named_foo_export(false), named_foo_export(true)],
            false,
            true,
        );

        let trace = trace_export(&graph, Path::new("/project"), "src/source.ts", "Foo")
            .expect("value export exists");

        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Value
        );
        assert!(trace.is_used);
        assert_eq!(trace.direct_references.len(), 1);
        assert_eq!(
            trace.direct_references[0].from_file,
            PathBuf::from("src/entry.ts")
        );
    }

    #[test]
    fn class_member_trace_inherits_the_type_lane_credit_of_its_owner() {
        use fallow_types::extract::{MemberInfo, MemberKind};

        // Issue #2371: the member trace is built from the owner's export
        // trace, so an owner credited only through the type lane reports a
        // used owner instead of the "referenced by no file" reason.
        let mut owner = named_foo_export(false);
        owner.members = vec![MemberInfo {
            name: "run".to_string(),
            kind: MemberKind::ClassMethod,
            span: oxc_span::Span::new(0, 3),
            has_decorator: false,
            decorator_names: vec![],
            is_instance_returning_static: false,
            is_self_returning: false,
        }];
        let graph = source_consumer_graph(vec![owner], true, false);

        let trace = trace_class_member(&graph, Path::new("/project"), "src/source.ts", "run")
            .expect("member of the traced export");

        assert!(trace.owner_is_used, "the type lane credits the owner");
        assert_eq!(
            trace.owner_namespace,
            fallow_types::semantic::SemanticNamespace::Type,
            "the member payload names the lane that credits its owner"
        );
        assert_eq!(trace.owner_direct_references.len(), 1);
        assert_eq!(
            trace.owner_direct_references[0].from_file,
            PathBuf::from("src/entry.ts")
        );
        assert!(
            trace.reason.contains("'Foo' is used by 1 file(s)"),
            "the reason must follow the owner's credit: {}",
            trace.reason
        );
    }

    #[test]
    fn trace_preserves_dual_namespace_named_re_exports() {
        let files: Vec<_> = ["entry", "barrel", "types", "values"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| DiscoveredFile {
                id: FileId(index as u32),
                path: PathBuf::from(format!("/project/src/{name}.ts")),
                size_bytes: 10,
            })
            .collect();
        let export = |is_type_only| ExportInfo {
            name: ExportName::Named("Foo".to_string()),
            local_name: Some("Foo".to_string()),
            is_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 3),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        };
        let re_export = |source: FileId, is_type_only| ResolvedReExport {
            info: ReExportInfo {
                source: format!("./{}", source.0),
                imported_name: "Foo".to_string(),
                exported_name: "Foo".to_string(),
                is_type_only,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source),
        };
        let mut resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: files[0].path.clone(),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./barrel".to_string(),
                        imported_name: ImportedName::Named("Foo".to_string()),
                        local_name: "Foo".to_string(),
                        is_type_only: false,
                        is_type_only_star: false,
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
                path: files[1].path.clone(),
                re_exports: vec![re_export(FileId(2), true), re_export(FileId(3), false)],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: files[2].path.clone(),
                exports: vec![export(true)].into(),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: files[3].path.clone(),
                exports: vec![export(false)].into(),
                ..Default::default()
            },
        ];
        let entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        resolved[1].re_exports.reverse();
        let reversed_graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let trace = trace_export(&graph, Path::new("/project"), "src/barrel.ts", "Foo")
            .expect("barrel exposes Foo in both namespaces");

        assert_eq!(
            trace.namespace,
            fallow_types::semantic::SemanticNamespace::Value
        );
        assert!(
            trace.is_used,
            "the value import must credit the value surface"
        );
        assert_eq!(trace.direct_references.len(), 1);
        let reversed_trace = trace_export(
            &reversed_graph,
            Path::new("/project"),
            "src/barrel.ts",
            "Foo",
        )
        .expect("reversed declarations expose the same surface");
        assert_eq!(
            serde_json::to_value(trace).expect("serialize trace"),
            serde_json::to_value(reversed_trace).expect("serialize reversed trace")
        );
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
                        is_type_only_star: false,
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
                }]
                .into(),
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

    #[test]
    fn exact_class_method_resolution_rejects_overloads_without_guessing() {
        use fallow_types::extract::{MemberInfo, MemberKind};

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = root.join("repository.ts");
        let source =
            "export class Repository {\n  save(): void;\n  save(): void {}\n  run(): void {}\n}\n";
        std::fs::write(&path, source).unwrap();
        let first = source.find("save").unwrap() as u32;
        let second = source.rfind("save").unwrap() as u32;
        let run = source.find("run").unwrap() as u32;
        let member = |name: &str, start| MemberInfo {
            name: name.to_string(),
            kind: MemberKind::ClassMethod,
            span: oxc_span::Span::new(start, start + 4),
            has_decorator: false,
            decorator_names: vec![],
            is_instance_returning_static: false,
            is_self_returning: false,
        };
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: path.clone(),
            size_bytes: source.len() as u64,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path,
            exports: vec![ExportInfo {
                name: ExportName::Named("Repository".to_string()),
                local_name: Some("Repository".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, source.len() as u32),
                members: vec![
                    member("save", first),
                    member("save", second),
                    member("run", run),
                ],
                is_side_effect_used: false,
                super_class: None,
            }]
            .into(),
            ..Default::default()
        }];
        let graph = ModuleGraph::build(&resolved_modules, &[], &files);

        assert_eq!(
            semantic_symbol_for_exact_class_method(
                &graph,
                root,
                "repository.ts",
                "Repository",
                "save",
            ),
            Err(SemanticClassMethodResolutionError::AmbiguousSymbol)
        );
        assert_eq!(
            semantic_symbol_for_exact_class_method(
                &graph,
                root,
                "repository.ts",
                "OtherRepository",
                "save",
            ),
            Err(SemanticClassMethodResolutionError::SymbolNotFound)
        );
        let resolved = semantic_symbol_for_exact_class_method(
            &graph,
            root,
            "repository.ts",
            "Repository",
            "run",
        )
        .unwrap();
        assert_eq!(resolved.owner.as_deref(), Some("Repository"));
        assert_eq!(resolved.local_name, "run");
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
                }]
                .into(),
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
                        is_type_only_star: false,
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
                ]
                .into(),
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
                    is_type_only_star: false,
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
                similarity: None,
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
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
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
                similarity: None,
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
                similarity: None,
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
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
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
                similarity: None,
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
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
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
                similarity: None,
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
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
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
            namespace: fallow_types::semantic::SemanticNamespace::Value,
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
            semantic: None,
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
