//! Reference-narrowing helpers for namespace imports and CSS module imports.
//!
//! These functions determine which exports should be marked as referenced
//! based on member access analysis, avoiding conservative "mark all" behavior
//! when specific member accesses can be identified.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedModule;
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, VisibilityTag};

use super::types::{ExportSymbol, ReExportEdge, ReferenceKind, SymbolReference};
use super::{ImportedSymbol, ModuleNode};

use super::build::{export_matches, is_css_module_path};

/// Check whether an import binding is unused in the source file.
///
/// Returns `true` if the binding should be skipped (unused).
pub(super) fn is_unused_import_binding(
    sym_local_name: &str,
    sym_imported_name: &ImportedName,
    source_mod: Option<&&ResolvedModule>,
) -> bool {
    !sym_local_name.is_empty()
        && !matches!(sym_imported_name, ImportedName::SideEffect)
        && source_mod.is_some_and(|m| m.unused_import_bindings.contains(sym_local_name))
}

/// Extract member access names for a given local variable from a resolved module.
pub(super) fn extract_accessed_members(
    source_mod: Option<&&ResolvedModule>,
    local_name: &str,
) -> Vec<String> {
    source_mod
        .map(|m| {
            m.member_accesses
                .iter()
                .filter(|ma| ma.object == local_name)
                .map(|ma| ma.member.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Mark all exports on a module as referenced by a given source file.
///
/// Deduplicates: skips exports already referenced by `source_id`.
pub(super) fn mark_all_exports_referenced(
    exports: &mut Vec<ExportSymbol>,
    source_id: FileId,
    import_span: oxc_span::Span,
    kind: ReferenceKind,
) {
    for export in exports {
        attach_reference(export, source_id, kind, import_span);
    }
}

fn attach_reference(
    export: &mut ExportSymbol,
    source_id: FileId,
    kind: ReferenceKind,
    import_span: oxc_span::Span,
) {
    if export.references.iter().all(|r| r.from_file != source_id) {
        export.references.push(SymbolReference {
            from_file: source_id,
            kind,
            import_span,
        });
    }
}

/// Mark only exports whose names appear in `accessed_members` as referenced.
///
/// Returns the set of member names that were found among the exports.
pub(super) fn mark_member_exports_referenced(
    exports: &mut [ExportSymbol],
    source_id: FileId,
    accessed_members: &[String],
    import_span: oxc_span::Span,
    kind: ReferenceKind,
) -> FxHashSet<String> {
    let member_set: FxHashSet<&str> = accessed_members.iter().map(String::as_str).collect();
    let mut found_members: FxHashSet<String> = FxHashSet::default();
    for export in exports {
        let name_str = match &export.name {
            fallow_types::extract::ExportName::Named(n) => n.as_str(),
            fallow_types::extract::ExportName::Default => "default",
        };
        if member_set.contains(name_str) {
            found_members.insert(name_str.to_owned());
            attach_reference(export, source_id, kind, import_span);
        }
    }
    found_members
}

/// Create synthetic `ExportSymbol` entries for members accessed via namespace import
/// that were not found among the target's own exports, but the target has `export *`
/// re-exports that may forward those names.
pub(super) fn create_synthetic_exports_for_star_re_exports(
    exports: &mut Vec<ExportSymbol>,
    re_exports: &[ReExportEdge],
    source_id: FileId,
    accessed_members: &[String],
    found_members: &FxHashSet<String>,
    import_span: oxc_span::Span,
) {
    let has_star_re_exports = re_exports.iter().any(|re| re.exported_name == "*");
    if !has_star_re_exports {
        return;
    }
    for member in accessed_members {
        if found_members.contains(member) {
            continue;
        }
        let export_name = if member == "default" {
            fallow_types::extract::ExportName::Default
        } else {
            fallow_types::extract::ExportName::Named(member.clone())
        };
        exports.push(ExportSymbol {
            name: export_name,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 0),
            references: vec![SymbolReference {
                from_file: source_id,
                kind: ReferenceKind::NamespaceImport,
                import_span,
            }],
            members: Vec::new(),
        });
    }
}

/// Handle namespace import narrowing for `import * as ns from './x'`.
///
/// If member accesses can be determined, only those exports are marked as used.
/// Otherwise, all exports are conservatively marked as referenced.
pub(super) fn narrow_namespace_references(
    module: &mut ModuleNode,
    source_id: FileId,
    sym_local_name: &str,
    sym_import_span: oxc_span::Span,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    entry_point_ids: &FxHashSet<FileId>,
) {
    let source_mod = module_by_id.get(&source_id);
    let accessed_members = extract_accessed_members(source_mod, sym_local_name);

    // Check if the namespace is consumed as a whole object
    // (Object.values, for..in, spread, destructuring with rest, etc.)
    let is_whole_object =
        source_mod.is_some_and(|m| m.whole_object_uses.iter().any(|n| n == sym_local_name));

    // Check if the namespace variable is re-exported (export { ns } or export default ns)
    // from a NON-entry-point file. If the importing file IS an entry point,
    // the re-export is for external consumption and doesn't prove internal usage.
    let is_re_exported_from_non_entry = source_mod.is_some_and(|m| {
        m.exports
            .iter()
            .any(|e| e.local_name.as_deref() == Some(sym_local_name))
    }) && !entry_point_ids.contains(&source_id);

    // For entry point files with no member accesses, the namespace
    // is purely re-exported for external use — don't mark all exports
    // as used internally. The `export *` path handles individual tracking.
    let is_entry_with_no_access =
        accessed_members.is_empty() && !is_whole_object && entry_point_ids.contains(&source_id);

    if is_whole_object
        || (!is_entry_with_no_access
            && (accessed_members.is_empty() || is_re_exported_from_non_entry))
    {
        // Can't narrow — mark all exports as referenced (conservative)
        mark_all_exports_referenced(
            &mut module.exports,
            source_id,
            sym_import_span,
            ReferenceKind::NamespaceImport,
        );
    } else {
        // Narrow: only mark accessed members as referenced
        let found_members = mark_member_exports_referenced(
            &mut module.exports,
            source_id,
            &accessed_members,
            sym_import_span,
            ReferenceKind::NamespaceImport,
        );

        // For members not found on the target (e.g., barrel with
        // `export *` that has no own exports for these names),
        // create synthetic ExportSymbol entries so that
        // resolve_re_export_chains can propagate them to the
        // actual source modules.
        create_synthetic_exports_for_star_re_exports(
            &mut module.exports,
            &module.re_exports,
            source_id,
            &accessed_members,
            &found_members,
            sym_import_span,
        );
    }
}

/// Handle CSS Module default-import narrowing.
///
/// `import styles from './Button.module.css'` — member accesses like `styles.primary`
/// mark the `primary` named export as referenced, since CSS module default imports act
/// as namespace objects where each property corresponds to a class name (named export).
pub(super) fn narrow_css_module_references(
    exports: &mut Vec<ExportSymbol>,
    source_id: FileId,
    sym_local_name: &str,
    sym_import_span: oxc_span::Span,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
) {
    let source_mod = module_by_id.get(&source_id);
    let is_whole_object =
        source_mod.is_some_and(|m| m.whole_object_uses.iter().any(|n| n == sym_local_name));
    let accessed_members = extract_accessed_members(source_mod, sym_local_name);

    if is_whole_object || accessed_members.is_empty() {
        mark_all_exports_referenced(
            exports,
            source_id,
            sym_import_span,
            ReferenceKind::DefaultImport,
        );
    } else {
        mark_member_exports_referenced(
            exports,
            source_id,
            &accessed_members,
            sym_import_span,
            ReferenceKind::DefaultImport,
        );
    }
}

/// Determine the `ReferenceKind` for an imported name.
pub(super) const fn reference_kind_for(imported_name: &ImportedName) -> ReferenceKind {
    match imported_name {
        ImportedName::Named(_) => ReferenceKind::NamedImport,
        ImportedName::Default => ReferenceKind::DefaultImport,
        ImportedName::Namespace => ReferenceKind::NamespaceImport,
        ImportedName::SideEffect => ReferenceKind::SideEffectImport,
    }
}

fn import_binding_has_type_usage(source_mod: Option<&&ResolvedModule>, local_name: &str) -> bool {
    !local_name.is_empty()
        && source_mod.is_some_and(|m| {
            m.type_referenced_import_bindings
                .iter()
                .any(|binding| binding == local_name)
        })
}

fn import_binding_has_value_usage(source_mod: Option<&&ResolvedModule>, local_name: &str) -> bool {
    !local_name.is_empty()
        && source_mod.is_some_and(|m| {
            m.value_referenced_import_bindings
                .iter()
                .any(|binding| binding == local_name)
        })
}

fn attach_direct_export_references(
    target_module: &mut ModuleNode,
    source_id: FileId,
    sym: &ImportedSymbol,
    source_mod: Option<&&ResolvedModule>,
    ref_kind: ReferenceKind,
) {
    let matching_exports: Vec<usize> = target_module
        .exports
        .iter()
        .enumerate()
        .filter_map(|(idx, export)| export_matches(&export.name, &sym.imported_name).then_some(idx))
        .collect();

    if matching_exports.is_empty() {
        return;
    }

    let type_exports: Vec<usize> = matching_exports
        .iter()
        .copied()
        .filter(|idx| target_module.exports[*idx].is_type_only)
        .collect();
    let value_exports: Vec<usize> = matching_exports
        .iter()
        .copied()
        .filter(|idx| !target_module.exports[*idx].is_type_only)
        .collect();

    let has_type_usage = import_binding_has_type_usage(source_mod, &sym.local_name);
    let has_value_usage = import_binding_has_value_usage(source_mod, &sym.local_name);

    let attach_type_exports = if type_exports.is_empty() {
        false
    } else if value_exports.is_empty() || sym.is_type_only {
        true
    } else {
        has_type_usage
    };

    let attach_value_exports = if value_exports.is_empty() {
        false
    } else if type_exports.is_empty() {
        true
    } else {
        has_value_usage
    };

    if attach_type_exports || attach_value_exports {
        for idx in &type_exports {
            if attach_type_exports {
                attach_reference(
                    &mut target_module.exports[*idx],
                    source_id,
                    ref_kind,
                    sym.import_span,
                );
            }
        }
        for idx in &value_exports {
            if attach_value_exports {
                attach_reference(
                    &mut target_module.exports[*idx],
                    source_id,
                    ref_kind,
                    sym.import_span,
                );
            }
        }
        return;
    }

    // No usage split available. Preserve the old behavior as a fallback, but
    // bias `import type` toward type exports when both namespaces exist.
    let fallback_idx = if sym.is_type_only {
        type_exports
            .first()
            .copied()
            .or_else(|| value_exports.first().copied())
    } else {
        value_exports
            .first()
            .copied()
            .or_else(|| type_exports.first().copied())
    };

    if let Some(idx) = fallback_idx {
        attach_reference(
            &mut target_module.exports[idx],
            source_id,
            ref_kind,
            sym.import_span,
        );
    }
}

/// Process a single imported symbol, attaching references to the target module's exports.
///
/// Handles: direct export matching, namespace import narrowing, and CSS module narrowing.
pub(super) fn attach_symbol_reference(
    target_module: &mut ModuleNode,
    source_id: FileId,
    sym: &ImportedSymbol,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    entry_point_ids: &FxHashSet<FileId>,
) {
    let ref_kind = reference_kind_for(&sym.imported_name);
    let source_mod = module_by_id.get(&source_id);

    // Skip references for import bindings that are never used in the importing file.
    if is_unused_import_binding(&sym.local_name, &sym.imported_name, source_mod) {
        return;
    }

    attach_direct_export_references(target_module, source_id, sym, source_mod, ref_kind);

    // Namespace imports: narrow to specific member accesses when possible,
    // otherwise conservatively mark all exports as used.
    if matches!(sym.imported_name, ImportedName::Namespace) {
        if sym.local_name.is_empty() {
            // No local name available — mark all (conservative)
            mark_all_exports_referenced(
                &mut target_module.exports,
                source_id,
                sym.import_span,
                ReferenceKind::NamespaceImport,
            );
        } else {
            narrow_namespace_references(
                target_module,
                source_id,
                &sym.local_name,
                sym.import_span,
                module_by_id,
                entry_point_ids,
            );
        }
    }

    // CSS Module default imports: member accesses like `styles.primary` mark
    // the `primary` named export as referenced.
    if matches!(sym.imported_name, ImportedName::Default)
        && !sym.local_name.is_empty()
        && is_css_module_path(&target_module.path)
    {
        narrow_css_module_references(
            &mut target_module.exports,
            source_id,
            &sym.local_name,
            sym.import_span,
            module_by_id,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::extract::{ExportName, VisibilityTag};

    use super::super::ModuleGraph;

    // ── is_unused_import_binding ────────────────────────────────────────

    #[test]
    fn is_unused_binding_true() {
        let resolved = ResolvedModule {
            path: std::path::PathBuf::from("/project/entry.ts"),
            unused_import_bindings: FxHashSet::from_iter(["unusedVar".to_string()]),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            ..Default::default()
        };
        assert!(is_unused_import_binding(
            "unusedVar",
            &ImportedName::Named("x".to_string()),
            Some(&&resolved),
        ));
    }

    #[test]
    fn is_unused_binding_false_when_used() {
        let resolved = ResolvedModule {
            path: std::path::PathBuf::from("/project/entry.ts"),
            unused_import_bindings: FxHashSet::from_iter(["otherVar".to_string()]),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            ..Default::default()
        };
        assert!(!is_unused_import_binding(
            "usedVar",
            &ImportedName::Named("x".to_string()),
            Some(&&resolved),
        ));
    }

    #[test]
    fn is_unused_binding_false_for_side_effect() {
        let resolved = ResolvedModule {
            path: std::path::PathBuf::from("/project/entry.ts"),
            unused_import_bindings: FxHashSet::from_iter(["x".to_string()]),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            ..Default::default()
        };
        // SideEffect imports are never "unused bindings"
        assert!(!is_unused_import_binding(
            "x",
            &ImportedName::SideEffect,
            Some(&&resolved),
        ));
    }

    #[test]
    fn is_unused_binding_false_for_empty_local_name() {
        let resolved = ResolvedModule {
            path: std::path::PathBuf::from("/project/entry.ts"),
            ..Default::default()
        };
        assert!(!is_unused_import_binding(
            "",
            &ImportedName::Named("x".to_string()),
            Some(&&resolved),
        ));
    }

    #[test]
    fn is_unused_binding_false_for_no_source_module() {
        assert!(!is_unused_import_binding(
            "x",
            &ImportedName::Named("x".to_string()),
            None,
        ));
    }

    // ── extract_accessed_members ─────────────────────────────────────────

    #[test]
    fn extract_accessed_members_found() {
        let resolved = ResolvedModule {
            path: std::path::PathBuf::from("/project/entry.ts"),
            member_accesses: vec![
                fallow_types::extract::MemberAccess {
                    object: "ns".to_string(),
                    member: "foo".to_string(),
                },
                fallow_types::extract::MemberAccess {
                    object: "ns".to_string(),
                    member: "bar".to_string(),
                },
                fallow_types::extract::MemberAccess {
                    object: "other".to_string(),
                    member: "baz".to_string(),
                },
            ],
            ..Default::default()
        };
        let members = extract_accessed_members(Some(&&resolved), "ns");
        assert_eq!(members, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn extract_accessed_members_none_module() {
        let members = extract_accessed_members(None, "ns");
        assert!(members.is_empty());
    }

    // ── mark_all_exports_referenced ─────────────────────────────────────

    #[test]
    fn mark_all_exports_referenced_adds_refs() {
        let mut exports = vec![
            ExportSymbol {
                name: ExportName::Named("a".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(0, 5),
                references: Vec::new(),
                members: Vec::new(),
            },
            ExportSymbol {
                name: ExportName::Named("b".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(10, 15),
                references: Vec::new(),
                members: Vec::new(),
            },
        ];
        mark_all_exports_referenced(
            &mut exports,
            FileId(5),
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );
        assert_eq!(exports[0].references.len(), 1);
        assert_eq!(exports[0].references[0].from_file, FileId(5));
        assert_eq!(exports[1].references.len(), 1);
    }

    #[test]
    fn mark_all_exports_referenced_deduplicates() {
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("a".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 5),
            references: vec![SymbolReference {
                from_file: FileId(5),
                kind: ReferenceKind::NamedImport,
                import_span: oxc_span::Span::new(0, 10),
            }],
            members: Vec::new(),
        }];
        // Same source file — should not add a duplicate
        mark_all_exports_referenced(
            &mut exports,
            FileId(5),
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );
        assert_eq!(exports[0].references.len(), 1);
    }

    // ── mark_member_exports_referenced ──────────────────────────────────

    #[test]
    fn mark_member_exports_referenced_only_accessed() {
        let mut exports = vec![
            ExportSymbol {
                name: ExportName::Named("foo".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(0, 5),
                references: Vec::new(),
                members: Vec::new(),
            },
            ExportSymbol {
                name: ExportName::Named("bar".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(10, 15),
                references: Vec::new(),
                members: Vec::new(),
            },
        ];
        let accessed = vec!["foo".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            FileId(0),
            &accessed,
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );

        assert_eq!(exports[0].references.len(), 1);
        assert!(exports[1].references.is_empty());
        assert!(found.contains("foo"));
        assert!(!found.contains("bar"));
    }

    // ── create_synthetic_exports_for_star_re_exports ────────────────────

    #[test]
    fn create_synthetic_exports_with_star_re_export() {
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("existing".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            members: Vec::new(),
        }];
        let re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        let accessed = vec!["missing".to_string()];
        let found = FxHashSet::default(); // nothing found among own exports

        create_synthetic_exports_for_star_re_exports(
            &mut exports,
            &re_exports,
            FileId(0),
            &accessed,
            &found,
            oxc_span::Span::new(0, 10),
        );

        assert_eq!(exports.len(), 2);
        assert_eq!(exports[1].name, ExportName::Named("missing".to_string()));
        assert_eq!(exports[1].references.len(), 1);
    }

    #[test]
    fn create_synthetic_exports_skips_already_found() {
        let mut exports = Vec::new();
        let re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        let accessed = vec!["already".to_string()];
        let mut found = FxHashSet::default();
        found.insert("already".to_string());

        create_synthetic_exports_for_star_re_exports(
            &mut exports,
            &re_exports,
            FileId(0),
            &accessed,
            &found,
            oxc_span::Span::new(0, 10),
        );

        assert!(
            exports.is_empty(),
            "should not create synthetic for already-found members"
        );
    }

    #[test]
    fn create_synthetic_exports_no_star_re_exports() {
        let mut exports = Vec::new();
        let re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "foo".to_string(),
            exported_name: "foo".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        let accessed = vec!["missing".to_string()];
        let found = FxHashSet::default();

        create_synthetic_exports_for_star_re_exports(
            &mut exports,
            &re_exports,
            FileId(0),
            &accessed,
            &found,
            oxc_span::Span::new(0, 10),
        );

        assert!(
            exports.is_empty(),
            "should not create synthetic without star re-exports"
        );
    }

    // ── reference_kind_for ──────────────────────────────────────────────

    #[test]
    fn reference_kind_for_named() {
        assert_eq!(
            reference_kind_for(&ImportedName::Named("x".to_string())),
            ReferenceKind::NamedImport,
        );
    }

    #[test]
    fn reference_kind_for_default() {
        assert_eq!(
            reference_kind_for(&ImportedName::Default),
            ReferenceKind::DefaultImport,
        );
    }

    #[test]
    fn reference_kind_for_namespace() {
        assert_eq!(
            reference_kind_for(&ImportedName::Namespace),
            ReferenceKind::NamespaceImport,
        );
    }

    #[test]
    fn reference_kind_for_side_effect() {
        assert_eq!(
            reference_kind_for(&ImportedName::SideEffect),
            ReferenceKind::SideEffectImport,
        );
    }

    // ── attach_symbol_reference (integration-level, through public build) ──

    #[test]
    fn attach_ref_skips_unused_binding() {
        // entry imports "foo" from utils, but "foo" is in unused_import_bindings
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
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
                unused_import_bindings: FxHashSet::from_iter(["foo".to_string()]),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Named("foo".to_string()),
                    local_name: Some("foo".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
        let foo_export = graph.modules[1]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "foo")
            .unwrap();
        assert!(
            foo_export.references.is_empty(),
            "unused binding should not create a reference"
        );
    }

    #[test]
    fn attach_ref_namespace_narrows_to_member_accesses() {
        // entry.ts: import * as utils from './utils'; uses utils.foo, not utils.bar
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Namespace,
                        local_name: "utils".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                member_accesses: vec![fallow_types::extract::MemberAccess {
                    object: "utils".to_string(),
                    member: "foo".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                exports: vec![
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("foo".to_string()),
                        local_name: Some("foo".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("bar".to_string()),
                        local_name: Some("bar".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let foo_export = graph.modules[1]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "foo")
            .unwrap();
        assert!(
            !foo_export.references.is_empty(),
            "foo should be referenced via namespace narrowing"
        );

        let bar_export = graph.modules[1]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "bar")
            .unwrap();
        assert!(
            bar_export.references.is_empty(),
            "bar should not be referenced when only foo is accessed"
        );
    }

    #[test]
    fn attach_ref_namespace_whole_object_marks_all() {
        // entry.ts: import * as utils from './utils'; Object.values(utils)
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./utils".to_string(),
                        imported_name: ImportedName::Namespace,
                        local_name: "utils".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                whole_object_uses: vec!["utils".to_string()],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/utils.ts"),
                exports: vec![
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("foo".to_string()),
                        local_name: Some("foo".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("bar".to_string()),
                        local_name: Some("bar".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        // Both exports should be referenced because the namespace is used as whole object
        for export in &graph.modules[1].exports {
            assert!(
                !export.references.is_empty(),
                "{} should be referenced when namespace is used as whole object",
                export.name
            );
        }
    }

    #[test]
    fn attach_ref_css_module_narrows_to_member_accesses() {
        // entry.ts: import styles from './Button.module.css'; uses styles.primary
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/Button.module.css"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./Button.module.css".to_string(),
                        imported_name: ImportedName::Default,
                        local_name: "styles".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: oxc_span::Span::new(0, 10),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                member_accesses: vec![fallow_types::extract::MemberAccess {
                    object: "styles".to_string(),
                    member: "primary".to_string(),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: std::path::PathBuf::from("/project/Button.module.css"),
                exports: vec![
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("primary".to_string()),
                        local_name: Some("primary".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    fallow_types::extract::ExportInfo {
                        name: ExportName::Named("secondary".to_string()),
                        local_name: Some("secondary".to_string()),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let primary = graph.modules[1]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "primary")
            .unwrap();
        assert!(
            !primary.references.is_empty(),
            "primary should be referenced via CSS module narrowing"
        );

        let secondary = graph.modules[1]
            .exports
            .iter()
            .find(|e| e.name.to_string() == "secondary")
            .unwrap();
        assert!(
            secondary.references.is_empty(),
            "secondary should not be referenced — only primary is accessed"
        );
    }

    #[test]
    fn attach_ref_default_import_creates_reference() {
        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: std::path::PathBuf::from("/project/component.ts"),
                size_bytes: 50,
            },
        ];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: std::path::PathBuf::from("/project/entry.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: fallow_types::extract::ImportInfo {
                        source: "./component".to_string(),
                        imported_name: ImportedName::Default,
                        local_name: "Component".to_string(),
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
                path: std::path::PathBuf::from("/project/component.ts"),
                exports: vec![fallow_types::extract::ExportInfo {
                    name: ExportName::Default,
                    local_name: Some("Component".to_string()),
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        let default_export = graph.modules[1]
            .exports
            .iter()
            .find(|e| matches!(e.name, ExportName::Default))
            .unwrap();
        assert_eq!(default_export.references.len(), 1);
        assert_eq!(
            default_export.references[0].kind,
            ReferenceKind::DefaultImport
        );
    }

    #[test]
    fn type_only_package_usage_tracked_through_build() {
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            size_bytes: 100,
        }];
        let entry_points = vec![fallow_types::discover::EntryPoint {
            path: std::path::PathBuf::from("/project/entry.ts"),
            source: fallow_types::discover::EntryPointSource::PackageJsonMain,
        }];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: std::path::PathBuf::from("/project/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: fallow_types::extract::ImportInfo {
                    source: "react".to_string(),
                    imported_name: ImportedName::Named("FC".to_string()),
                    local_name: "FC".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            }],
            ..Default::default()
        }];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

        assert!(graph.package_usage.contains_key("react"));
        assert!(graph.type_only_package_usage.contains_key("react"));
    }

    // ── mark_member_exports_referenced: edge cases ───────────────────

    #[test]
    fn mark_member_exports_referenced_default_export() {
        let mut exports = vec![ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            members: Vec::new(),
        }];
        let accessed = vec!["default".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            FileId(0),
            &accessed,
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );
        assert_eq!(exports[0].references.len(), 1);
        assert!(found.contains("default"));
    }

    #[test]
    fn mark_member_exports_referenced_deduplicates() {
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 5),
            references: vec![SymbolReference {
                from_file: FileId(0),
                kind: ReferenceKind::NamedImport,
                import_span: oxc_span::Span::new(0, 10),
            }],
            members: Vec::new(),
        }];
        let accessed = vec!["foo".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            FileId(0), // same file as existing reference
            &accessed,
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );
        // Should not add duplicate reference from same file
        assert_eq!(exports[0].references.len(), 1);
        assert!(found.contains("foo"));
    }

    #[test]
    fn mark_member_exports_referenced_empty_accessed() {
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            members: Vec::new(),
        }];
        let accessed: Vec<String> = vec![];
        let found = mark_member_exports_referenced(
            &mut exports,
            FileId(0),
            &accessed,
            oxc_span::Span::new(0, 10),
            ReferenceKind::NamespaceImport,
        );
        assert!(exports[0].references.is_empty());
        assert!(found.is_empty());
    }

    // ── create_synthetic_exports_for_star_re_exports: default export ──

    #[test]
    fn create_synthetic_exports_default_member() {
        let mut exports = Vec::new();
        let re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        let accessed = vec!["default".to_string()];
        let found = FxHashSet::default();

        create_synthetic_exports_for_star_re_exports(
            &mut exports,
            &re_exports,
            FileId(0),
            &accessed,
            &found,
            oxc_span::Span::new(0, 10),
        );

        assert_eq!(exports.len(), 1);
        assert!(matches!(exports[0].name, ExportName::Default));
    }
}
