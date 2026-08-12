//! Reference-narrowing helpers for namespace imports and CSS module imports.
//!
//! These functions determine which exports should be marked as referenced
//! based on member access analysis, avoiding conservative "mark all" behavior
//! when specific member accesses can be identified.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedModule;
use fallow_types::discover::FileId;
#[cfg(test)]
use fallow_types::extract::ModuleLoadMechanism;
use fallow_types::extract::{ImportedName, VisibilityTag};

use super::types::{
    ExportSymbol, ReExportEdge, ReferenceKind, ReferencePathId, ReferencePathInterner,
    SymbolReference,
};
use super::{ExportNamespace, ImportedSymbol, ModuleNode};

use super::build::{ExportNameIndex, is_css_module_path};

/// Reference counts below this keep the linear duplicate scan in
/// `attach_reference`; hot exports (barrels, widely imported utilities with
/// thousands of consumers) cross it and switch to a hashed per-export set so
/// the k-th attach stays O(1) instead of scanning k-1 earlier references.
const REFERENCE_DEDUP_THRESHOLD: usize = 32;

/// The `(from_file, path)` pairs already attached to one export.
type AttachedSiteSet = FxHashSet<(FileId, Option<ReferencePathId>, ExportNamespace)>;

/// Transient per-export index of already-attached `(from_file, path)` pairs,
/// keyed by `(module file, export index)`.
///
/// Only valid while every reference push to the covered exports flows through
/// [`attach_reference`]: each attachment pass creates its own instance and
/// drops it before any other code (re-export propagation, cache merge) pushes
/// references directly. Sets are seeded lazily from the export's current
/// references at first touch, so a fresh instance is always correct.
/// Exports are append-only during these passes, so `(module, index)` keys
/// stay stable and synthetic exports created mid-pass get fresh keys.
#[derive(Default)]
pub(super) struct ReferenceDedup {
    seen: FxHashMap<(FileId, usize), AttachedSiteSet>,
}

/// Shared lookup state threaded from `populate_references` into the
/// symbol-attachment pipeline.
pub(super) struct AttachContext<'a> {
    pub(super) module_by_id: &'a FxHashMap<FileId, &'a ResolvedModule>,
    pub(super) entry_point_ids: &'a FxHashSet<FileId>,
    pub(super) export_index: &'a ExportNameIndex,
    pub(super) effective_exports: &'a super::effective_exports::EffectiveExportIndex,
    pub(super) dedup: &'a mut ReferenceDedup,
}

impl AttachContext<'_> {
    fn reborrow(&mut self) -> AttachContext<'_> {
        AttachContext {
            module_by_id: self.module_by_id,
            entry_point_ids: self.entry_point_ids,
            export_index: self.export_index,
            effective_exports: self.effective_exports,
            dedup: self.dedup,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ReferenceSite {
    from_file: FileId,
    import_span: oxc_span::Span,
    path: Option<ReferencePathId>,
}

#[derive(Clone, Copy)]
#[cfg(test)]
pub(super) struct ReferenceTarget {
    pub(super) source_id: FileId,
    pub(super) target_id: FileId,
    pub(super) import_span: oxc_span::Span,
    pub(super) kind: ReferenceKind,
}

impl ReferenceSite {
    pub(super) const fn exact(
        from_file: FileId,
        import_span: oxc_span::Span,
        path: Option<ReferencePathId>,
    ) -> Self {
        Self {
            from_file,
            import_span,
            path,
        }
    }

    #[cfg(test)]
    fn esm(target: ReferenceTarget, reference_paths: &mut ReferencePathInterner) -> Self {
        Self {
            from_file: target.source_id,
            import_span: target.import_span,
            path: reference_paths.direct(target.target_id, ModuleLoadMechanism::EsModule),
        }
    }

    fn from_symbol(
        source_id: FileId,
        target_id: FileId,
        symbol: &ImportedSymbol,
        reference_paths: &mut ReferencePathInterner,
    ) -> Self {
        Self {
            from_file: source_id,
            import_span: symbol.import_span,
            path: reference_paths.direct(target_id, symbol.mechanism),
        }
    }
}

/// Check whether an import binding is unused in the source file.
///
/// Returns `true` if the binding should be skipped (unused).
fn is_unused_import_binding(
    sym_local_name: &str,
    sym_imported_name: &ImportedName,
    source_mod: Option<&&ResolvedModule>,
) -> bool {
    !sym_local_name.is_empty()
        && !matches!(sym_imported_name, ImportedName::SideEffect)
        && source_mod.is_some_and(|m| m.unused_import_bindings.contains(sym_local_name))
}

/// Extract member access names for a given local variable from a resolved module.
fn extract_accessed_members(source_mod: Option<&&ResolvedModule>, local_name: &str) -> Vec<String> {
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
/// Profiled reachability deduplicates by source and exact runtime path, so ESM
/// and CommonJS references remain distinct when replacements can affect them.
/// Legacy reachability deliberately retains the pre-profile source-only
/// behavior because no replacement mask can distinguish those paths.
#[cfg(test)]
pub(super) fn mark_all_exports_referenced(
    exports: &mut [ExportSymbol],
    target: ReferenceTarget,
    reference_paths: &mut ReferencePathInterner,
) {
    let site = ReferenceSite::esm(target, reference_paths);
    let mut dedup = ReferenceDedup::default();
    for (index, export) in exports.iter_mut().enumerate() {
        let namespace = if export.is_type_only {
            ExportNamespace::Type
        } else {
            ExportNamespace::Value
        };
        attach_reference(
            export,
            (target.target_id, index),
            site,
            target.kind,
            namespace,
            &mut dedup,
        );
    }
}

pub(super) fn mark_all_exports_referenced_at_site(
    exports: &mut [ExportSymbol],
    context: &mut NamespaceMarkContext<'_>,
) {
    let indices = effective_export_indices(
        exports,
        context.module_id,
        None,
        context.namespace,
        context.effective_exports,
    );
    for idx in indices {
        let export = &mut exports[idx];
        attach_reference(
            export,
            (context.module_id, idx),
            context.site,
            context.kind,
            context.namespace,
            context.dedup,
        );
    }
}

pub(super) struct NamespaceMarkContext<'a> {
    pub(super) module_id: FileId,
    pub(super) site: ReferenceSite,
    pub(super) kind: ReferenceKind,
    pub(super) namespace: ExportNamespace,
    pub(super) effective_exports: &'a super::effective_exports::EffectiveExportIndex,
    pub(super) dedup: &'a mut ReferenceDedup,
}

fn attach_reference(
    export: &mut ExportSymbol,
    export_key: (FileId, usize),
    site: ReferenceSite,
    kind: ReferenceKind,
    namespace: ExportNamespace,
    dedup: &mut ReferenceDedup,
) {
    let is_new = match dedup.seen.entry(export_key) {
        std::collections::hash_map::Entry::Occupied(entry) => {
            entry
                .into_mut()
                .insert((site.from_file, site.path, namespace))
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            if export.references.len() < REFERENCE_DEDUP_THRESHOLD {
                !export.has_reference_from(site.from_file, site.path, namespace)
            } else {
                let seen = entry.insert(
                    export
                        .routed_references()
                        .map(|routed| {
                            (
                                routed.reference.from_file,
                                routed.path,
                                routed.reference.namespace,
                            )
                        })
                        .collect(),
                );
                seen.insert((site.from_file, site.path, namespace))
            }
        }
    };
    if is_new {
        export.push_reference(
            SymbolReference {
                from_file: site.from_file,
                kind,
                namespace,
                import_span: site.import_span,
            },
            site.path,
        );
    }
}

/// Mark only exports whose names appear in `accessed_members` as referenced.
///
/// Returns the set of member names that were found among the exports.
#[cfg(test)]
pub(super) fn mark_member_exports_referenced(
    exports: &mut [ExportSymbol],
    target: ReferenceTarget,
    accessed_members: &[String],
    reference_paths: &mut ReferencePathInterner,
) -> FxHashSet<String> {
    let member_set: FxHashSet<&str> = accessed_members.iter().map(String::as_str).collect();
    let site = ReferenceSite::esm(target, reference_paths);
    let mut dedup = ReferenceDedup::default();
    let mut found = FxHashSet::default();
    for (index, export) in exports.iter_mut().enumerate() {
        let name = export.name.to_string();
        if member_set.contains(name.as_str()) {
            let namespace = if export.is_type_only {
                ExportNamespace::Type
            } else {
                ExportNamespace::Value
            };
            attach_reference(
                export,
                (target.target_id, index),
                site,
                target.kind,
                namespace,
                &mut dedup,
            );
            found.insert(name);
        }
    }
    found
}

pub(super) fn mark_member_exports_referenced_at_site(
    exports: &mut [ExportSymbol],
    accessed_members: &[String],
    context: &mut NamespaceMarkContext<'_>,
) -> FxHashSet<String> {
    let member_set: FxHashSet<&str> = accessed_members.iter().map(String::as_str).collect();
    let mut found_members: FxHashSet<String> = FxHashSet::default();
    let indices = effective_export_indices(
        exports,
        context.module_id,
        Some(&member_set),
        context.namespace,
        context.effective_exports,
    );
    for idx in indices {
        let export = &mut exports[idx];
        let name_str = match &export.name {
            fallow_types::extract::ExportName::Named(n) => n.as_str(),
            fallow_types::extract::ExportName::Default => "default",
        };
        if member_set.contains(name_str) {
            found_members.insert(name_str.to_owned());
            attach_reference(
                export,
                (context.module_id, idx),
                context.site,
                context.kind,
                context.namespace,
                context.dedup,
            );
        }
    }
    found_members
}

fn effective_export_indices(
    exports: &[ExportSymbol],
    module_id: FileId,
    names: Option<&FxHashSet<&str>>,
    namespace: ExportNamespace,
    effective_exports: &super::effective_exports::EffectiveExportIndex,
) -> Vec<usize> {
    let mut by_name: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (index, export) in exports.iter().enumerate() {
        let name = match &export.name {
            fallow_types::extract::ExportName::Named(name) => name.as_str(),
            fallow_types::extract::ExportName::Default => "default",
        };
        if names.is_none_or(|names| names.contains(name)) {
            by_name.entry(name).or_default().push(index);
        }
    }
    let mut selected = Vec::new();
    for (name, matching) in by_name {
        let super::EffectiveExportResolution::Unique(binding) =
            effective_exports.resolve(module_id, name, namespace)
        else {
            continue;
        };
        if binding.origin_file() != module_id || binding.origin_slot().is_none() {
            selected.extend(matching);
            continue;
        }
        let exact: Vec<_> = matching
            .iter()
            .copied()
            .filter(|&index| exports[index].is_type_only == (namespace == ExportNamespace::Type))
            .collect();
        if namespace == ExportNamespace::Type && exact.is_empty() {
            selected.extend(
                matching
                    .into_iter()
                    .filter(|&index| !exports[index].is_type_only),
            );
        } else {
            selected.extend(exact);
        }
    }
    selected.sort_unstable();
    selected
}

/// Create synthetic `ExportSymbol` entries for members accessed via namespace import
/// that were not found among the target's own exports, but the target has `export *`
/// re-exports that may forward those names.
#[cfg(test)]
pub(super) fn create_synthetic_exports_for_star_re_exports(
    exports: &mut Vec<ExportSymbol>,
    re_exports: &[ReExportEdge],
    target: ReferenceTarget,
    accessed_members: &[String],
    found_members: &FxHashSet<String>,
    reference_paths: &mut ReferencePathInterner,
) {
    create_synthetic_exports_for_star_re_exports_at_site(
        exports,
        re_exports,
        ReferenceSite::esm(target, reference_paths),
        accessed_members,
        found_members,
        ExportNamespace::Value,
    );
}

pub(super) fn create_synthetic_exports_for_star_re_exports_at_site(
    exports: &mut Vec<ExportSymbol>,
    re_exports: &[ReExportEdge],
    site: ReferenceSite,
    accessed_members: &[String],
    found_members: &FxHashSet<String>,
    namespace: ExportNamespace,
) {
    let has_star_re_exports = re_exports.iter().any(|re| re.exported_name == "*");
    if !has_star_re_exports {
        return;
    }
    for member in accessed_members {
        if member == "default" || found_members.contains(member) {
            continue;
        }
        if let Some(export) = exports
            .iter_mut()
            .find(|export| export.name.matches_str(member))
        {
            if !export.has_reference_from(site.from_file, site.path, namespace) {
                export.push_reference(
                    SymbolReference {
                        from_file: site.from_file,
                        kind: ReferenceKind::NamespaceImport,
                        namespace,
                        import_span: site.import_span,
                    },
                    site.path,
                );
            }
            continue;
        }
        exports.push(ExportSymbol {
            name: fallow_types::extract::ExportName::Named(member.clone()),
            is_type_only: namespace == ExportNamespace::Type,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 0),
            references: vec![SymbolReference {
                from_file: site.from_file,
                kind: ReferenceKind::NamespaceImport,
                namespace,
                import_span: site.import_span,
            }],
            reference_paths: site.path.map(|path| vec![Some(path)]).unwrap_or_default(),
            members: Vec::new(),
        });
    }
}

/// Handle namespace import narrowing for `import * as ns from './x'`.
///
/// If member accesses can be determined, only those exports are marked as used.
/// Otherwise, all exports are conservatively marked as referenced.
fn narrow_namespace_references(
    module: &mut ModuleNode,
    site: ReferenceSite,
    sym_local_name: &str,
    namespaces: (bool, bool),
    ctx: &mut AttachContext<'_>,
) {
    let source_mod = ctx.module_by_id.get(&site.from_file);
    let accessed_members = extract_accessed_members(source_mod, sym_local_name);

    let is_whole_object =
        source_mod.is_some_and(|m| m.whole_object_uses.iter().any(|n| n == sym_local_name));

    let is_re_exported_from_non_entry = source_mod.is_some_and(|m| {
        m.exports
            .iter()
            .any(|e| e.local_name.as_deref() == Some(sym_local_name))
    }) && !ctx.entry_point_ids.contains(&site.from_file);

    let is_entry_with_no_access = accessed_members.is_empty()
        && !is_whole_object
        && ctx.entry_point_ids.contains(&site.from_file);

    for (namespace, is_used) in [
        (ExportNamespace::Type, namespaces.0),
        (ExportNamespace::Value, namespaces.1),
    ] {
        if !is_used {
            continue;
        }
        let mut mark = NamespaceMarkContext {
            module_id: module.file_id,
            site,
            kind: ReferenceKind::NamespaceImport,
            namespace,
            effective_exports: ctx.effective_exports,
            dedup: ctx.dedup,
        };
        if is_whole_object
            || (!is_entry_with_no_access
                && (accessed_members.is_empty() || is_re_exported_from_non_entry))
        {
            mark_all_exports_referenced_at_site(&mut module.exports, &mut mark);
        } else {
            let found_members = mark_member_exports_referenced_at_site(
                &mut module.exports,
                &accessed_members,
                &mut mark,
            );

            create_synthetic_exports_for_star_re_exports_at_site(
                &mut module.exports,
                &module.re_exports,
                site,
                &accessed_members,
                &found_members,
                namespace,
            );
        }
    }
}

/// Handle CSS Module default-import narrowing.
///
/// `import styles from './Button.module.css'` — member accesses like `styles.primary`
/// mark the `primary` named export as referenced, since CSS module default imports act
/// as namespace objects where each property corresponds to a class name (named export).
fn narrow_css_module_references(
    exports: &mut [ExportSymbol],
    module_id: FileId,
    site: ReferenceSite,
    sym_local_name: &str,
    ctx: &mut AttachContext<'_>,
) {
    let source_mod = ctx.module_by_id.get(&site.from_file);
    let is_whole_object =
        source_mod.is_some_and(|m| m.whole_object_uses.iter().any(|n| n == sym_local_name));
    let accessed_members = extract_accessed_members(source_mod, sym_local_name);
    let mut mark = NamespaceMarkContext {
        module_id,
        site,
        kind: ReferenceKind::DefaultImport,
        namespace: ExportNamespace::Value,
        effective_exports: ctx.effective_exports,
        dedup: ctx.dedup,
    };

    if is_whole_object || accessed_members.is_empty() {
        mark_all_exports_referenced_at_site(exports, &mut mark);
    } else {
        mark_member_exports_referenced_at_site(exports, &accessed_members, &mut mark);
    }
}

/// Determine the `ReferenceKind` for an imported name.
const fn reference_kind_for(imported_name: &ImportedName) -> ReferenceKind {
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
    site: ReferenceSite,
    sym: &ImportedSymbol,
    ref_kind: ReferenceKind,
    ctx: AttachContext<'_>,
) {
    let AttachContext {
        module_by_id,
        export_index,
        effective_exports,
        dedup,
        ..
    } = ctx;
    let source_mod = module_by_id.get(&site.from_file);
    let matching_exports: &[usize] = export_index.matches(&sym.imported_name);

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

    let (uses_type, uses_value) = desired_import_namespaces(sym, source_mod);
    let imported_name = match &sym.imported_name {
        ImportedName::Named(name) => name.as_str(),
        ImportedName::Default => "default",
        ImportedName::Namespace | ImportedName::SideEffect => return,
    };
    for (namespace, should_attach) in [
        (ExportNamespace::Type, uses_type),
        (ExportNamespace::Value, uses_value),
    ] {
        if !should_attach {
            continue;
        }
        let super::EffectiveExportResolution::Unique(binding) =
            effective_exports.resolve(target_module.file_id, imported_name, namespace)
        else {
            continue;
        };
        let indices: &[usize] =
            if binding.origin_file() != target_module.file_id || binding.origin_slot().is_none() {
                matching_exports
            } else if namespace == ExportNamespace::Type && !type_exports.is_empty() {
                &type_exports
            } else {
                &value_exports
            };
        for &idx in indices {
            attach_reference(
                &mut target_module.exports[idx],
                (target_module.file_id, idx),
                site,
                ref_kind,
                namespace,
                dedup,
            );
        }
    }
}

/// Decide which semantic namespaces the imported binding uses.
fn desired_import_namespaces(
    sym: &ImportedSymbol,
    source_mod: Option<&&ResolvedModule>,
) -> (bool, bool) {
    if sym.is_type_only {
        return (true, false);
    }
    let uses_type = import_binding_has_type_usage(source_mod, &sym.local_name);
    let uses_value = import_binding_has_value_usage(source_mod, &sym.local_name);
    if uses_type || uses_value {
        (uses_type, uses_value)
    } else {
        (false, true)
    }
}

/// Process a single imported symbol, attaching references to the target module's exports.
///
/// Handles: direct export matching, namespace import narrowing, and CSS module narrowing.
pub(super) fn attach_symbol_reference(
    target_module: &mut ModuleNode,
    source_id: FileId,
    sym: &ImportedSymbol,
    reference_paths: &mut ReferencePathInterner,
    mut ctx: AttachContext<'_>,
) {
    let ref_kind = reference_kind_for(&sym.imported_name);
    let source_mod = ctx.module_by_id.get(&source_id);

    if is_unused_import_binding(&sym.local_name, &sym.imported_name, source_mod) {
        return;
    }

    let site = ReferenceSite::from_symbol(source_id, target_module.file_id, sym, reference_paths);
    attach_direct_export_references(target_module, site, sym, ref_kind, ctx.reborrow());

    if matches!(sym.imported_name, ImportedName::Namespace) {
        if sym.local_name.is_empty() {
            let namespaces = desired_import_namespaces(sym, source_mod);
            for (namespace, is_used) in [
                (ExportNamespace::Type, namespaces.0),
                (ExportNamespace::Value, namespaces.1),
            ] {
                if is_used {
                    let mut mark = NamespaceMarkContext {
                        module_id: target_module.file_id,
                        site,
                        kind: ReferenceKind::NamespaceImport,
                        namespace,
                        effective_exports: ctx.effective_exports,
                        dedup: ctx.dedup,
                    };
                    mark_all_exports_referenced_at_site(&mut target_module.exports, &mut mark);
                }
            }
        } else {
            narrow_namespace_references(
                target_module,
                site,
                &sym.local_name,
                desired_import_namespaces(sym, source_mod),
                &mut ctx,
            );
        }
    }

    if matches!(sym.imported_name, ImportedName::Default)
        && !sym.local_name.is_empty()
        && is_css_module_path(&target_module.path)
    {
        narrow_css_module_references(
            &mut target_module.exports,
            target_module.file_id,
            site,
            &sym.local_name,
            &mut ctx,
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

    fn namespace_target(source_id: FileId, target_id: FileId) -> ReferenceTarget {
        ReferenceTarget {
            source_id,
            target_id,
            import_span: oxc_span::Span::new(0, 10),
            kind: ReferenceKind::NamespaceImport,
        }
    }

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
            ]
            .into(),
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

    #[test]
    fn mark_all_exports_referenced_adds_refs() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![
            ExportSymbol {
                name: ExportName::Named("a".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 5),
                references: Vec::new(),
                reference_paths: Vec::new(),
                members: Vec::new(),
            },
            ExportSymbol {
                name: ExportName::Named("b".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(10, 15),
                references: Vec::new(),
                reference_paths: Vec::new(),
                members: Vec::new(),
            },
        ];
        mark_all_exports_referenced(
            &mut exports,
            namespace_target(FileId(5), FileId(9)),
            &mut reference_paths,
        );
        assert_eq!(exports[0].references.len(), 1);
        assert_eq!(exports[0].references[0].from_file, FileId(5));
        assert_eq!(exports[1].references.len(), 1);
    }

    #[test]
    fn mark_all_exports_referenced_deduplicates() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("a".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: vec![SymbolReference {
                from_file: FileId(5),
                kind: ReferenceKind::NamedImport,
                namespace: ExportNamespace::Value,
                import_span: oxc_span::Span::new(0, 10),
            }],
            reference_paths: vec![reference_paths.direct(FileId(9), ModuleLoadMechanism::EsModule)],
            members: Vec::new(),
        }];
        mark_all_exports_referenced(
            &mut exports,
            namespace_target(FileId(5), FileId(9)),
            &mut reference_paths,
        );
        assert_eq!(exports[0].references.len(), 1);
    }

    #[test]
    fn untracked_reference_sites_preserve_legacy_source_deduplication() {
        let mut export = ExportSymbol {
            name: ExportName::Named("a".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        };

        let mut dedup = ReferenceDedup::default();
        attach_reference(
            &mut export,
            (FileId(9), 0),
            ReferenceSite::exact(FileId(5), oxc_span::Span::new(0, 10), None),
            ReferenceKind::NamedImport,
            ExportNamespace::Value,
            &mut dedup,
        );
        attach_reference(
            &mut export,
            (FileId(9), 0),
            ReferenceSite::exact(FileId(5), oxc_span::Span::new(20, 30), None),
            ReferenceKind::NamespaceImport,
            ExportNamespace::Value,
            &mut dedup,
        );

        assert_eq!(export.references.len(), 1);
        assert_eq!(export.references[0].kind, ReferenceKind::NamedImport);
        assert_eq!(export.references[0].import_span, oxc_span::Span::new(0, 10));
    }

    #[test]
    fn attach_reference_dedup_matches_linear_scan_across_threshold() {
        let mut export = ExportSymbol {
            name: ExportName::Named("hot".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        };
        let mut dedup = ReferenceDedup::default();
        let total = u32::try_from(REFERENCE_DEDUP_THRESHOLD * 3).unwrap_or(u32::MAX);
        for _ in 0..2 {
            for consumer in 0..total {
                attach_reference(
                    &mut export,
                    (FileId(9), 0),
                    ReferenceSite::exact(FileId(consumer), oxc_span::Span::new(0, 10), None),
                    ReferenceKind::NamedImport,
                    ExportNamespace::Value,
                    &mut dedup,
                );
            }
        }

        assert_eq!(export.references.len(), total as usize);
        for (consumer, reference) in export.references.iter().enumerate() {
            assert_eq!(reference.from_file.0 as usize, consumer);
        }
    }

    #[test]
    fn attach_reference_dedup_seeds_from_existing_references() {
        let mut export = ExportSymbol {
            name: ExportName::Named("hot".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: (0..u32::try_from(REFERENCE_DEDUP_THRESHOLD).unwrap_or(u32::MAX))
                .map(|consumer| SymbolReference {
                    from_file: FileId(consumer),
                    kind: ReferenceKind::NamedImport,
                    namespace: ExportNamespace::Value,
                    import_span: oxc_span::Span::new(0, 10),
                })
                .collect(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        };

        let mut dedup = ReferenceDedup::default();
        attach_reference(
            &mut export,
            (FileId(9), 0),
            ReferenceSite::exact(FileId(0), oxc_span::Span::new(0, 10), None),
            ReferenceKind::NamedImport,
            ExportNamespace::Value,
            &mut dedup,
        );
        assert_eq!(export.references.len(), REFERENCE_DEDUP_THRESHOLD);

        attach_reference(
            &mut export,
            (FileId(9), 0),
            ReferenceSite::exact(FileId(999), oxc_span::Span::new(0, 10), None),
            ReferenceKind::NamedImport,
            ExportNamespace::Value,
            &mut dedup,
        );
        assert_eq!(export.references.len(), REFERENCE_DEDUP_THRESHOLD + 1);
    }

    #[test]
    fn mark_member_exports_referenced_only_accessed() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![
            ExportSymbol {
                name: ExportName::Named("foo".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 5),
                references: Vec::new(),
                reference_paths: Vec::new(),
                members: Vec::new(),
            },
            ExportSymbol {
                name: ExportName::Named("bar".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(10, 15),
                references: Vec::new(),
                reference_paths: Vec::new(),
                members: Vec::new(),
            },
        ];
        let accessed = vec!["foo".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &mut reference_paths,
        );

        assert_eq!(exports[0].references.len(), 1);
        assert!(exports[1].references.is_empty());
        assert!(found.contains("foo"));
        assert!(!found.contains("bar"));
    }

    #[test]
    fn create_synthetic_exports_with_star_re_export() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("existing".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
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
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &found,
            &mut reference_paths,
        );

        assert_eq!(exports.len(), 2);
        assert_eq!(exports[1].name, ExportName::Named("missing".to_string()));
        assert_eq!(exports[1].references.len(), 1);
    }

    #[test]
    fn create_synthetic_exports_skips_already_found() {
        let mut reference_paths = ReferencePathInterner::default();
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
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &found,
            &mut reference_paths,
        );

        assert!(
            exports.is_empty(),
            "should not create synthetic for already-found members"
        );
    }

    #[test]
    fn create_synthetic_exports_no_star_re_exports() {
        let mut reference_paths = ReferencePathInterner::default();
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
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &found,
            &mut reference_paths,
        );

        assert!(
            exports.is_empty(),
            "should not create synthetic without star re-exports"
        );
    }

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

    #[test]
    fn attach_ref_skips_unused_binding() {
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }]
                .into(),
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
                }]
                .into(),
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
                        expected_unused_reason: None,
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
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ]
                .into(),
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
                whole_object_uses: vec!["utils".to_string()].into(),
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
                        expected_unused_reason: None,
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
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ]
                .into(),
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

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
                }]
                .into(),
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
                        expected_unused_reason: None,
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
                        expected_unused_reason: None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ]
                .into(),
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
                    expected_unused_reason: None,
                    span: oxc_span::Span::new(0, 20),
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                }]
                .into(),
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

    #[test]
    fn mark_member_exports_referenced_default_export() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        }];
        let accessed = vec!["default".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &mut reference_paths,
        );
        assert_eq!(exports[0].references.len(), 1);
        assert!(found.contains("default"));
    }

    #[test]
    fn mark_member_exports_referenced_deduplicates() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: vec![SymbolReference {
                from_file: FileId(0),
                kind: ReferenceKind::NamedImport,
                namespace: ExportNamespace::Value,
                import_span: oxc_span::Span::new(0, 10),
            }],
            reference_paths: vec![reference_paths.direct(FileId(9), ModuleLoadMechanism::EsModule)],
            members: Vec::new(),
        }];
        let accessed = vec!["foo".to_string()];
        let found = mark_member_exports_referenced(
            &mut exports,
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &mut reference_paths,
        );
        assert_eq!(exports[0].references.len(), 1);
        assert!(found.contains("foo"));
    }

    #[test]
    fn mark_member_exports_referenced_empty_accessed() {
        let mut reference_paths = ReferencePathInterner::default();
        let mut exports = vec![ExportSymbol {
            name: ExportName::Named("foo".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 5),
            references: Vec::new(),
            reference_paths: Vec::new(),
            members: Vec::new(),
        }];
        let accessed: Vec<String> = vec![];
        let found = mark_member_exports_referenced(
            &mut exports,
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &mut reference_paths,
        );
        assert!(exports[0].references.is_empty());
        assert!(found.is_empty());
    }

    #[test]
    fn create_synthetic_exports_skips_default_member() {
        let mut reference_paths = ReferencePathInterner::default();
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
            namespace_target(FileId(0), FileId(9)),
            &accessed,
            &found,
            &mut reference_paths,
        );

        assert!(exports.is_empty());
    }
}
