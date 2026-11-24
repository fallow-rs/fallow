//! Reference-narrowing helpers for namespace imports and CSS module imports.
//!
//! These functions determine which exports should be marked as referenced
//! based on member access analysis, avoiding conservative "mark all" behavior
//! when specific member accesses can be identified.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::resolve::ResolvedModule;
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, SemanticFact, VisibilityTag};

use super::namespace_escapes::{NamespaceEscape, NamespaceEscapeReason};
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

/// The exact `(from_file, import_span, path, namespace)` sites attached to one export.
type AttachedSiteSet = FxHashSet<(
    FileId,
    oxc_span::Span,
    Option<ReferencePathId>,
    ExportNamespace,
)>;

/// Transient per-export index of already-attached reference sites,
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
    /// Targets whose whole namespace object a consumer observed in this pass
    /// (issues #2357, #2372, #2373): every site that credits all exports of a
    /// namespace target instead of narrowing to accessed members, except a
    /// binding the namespace-object alias phase narrows on the consumer's
    /// behalf. Seeds `ModuleGraph::collect_exposed_namespace_targets`, which
    /// extends the credit to the names the target only exposes through its
    /// own `export *` and `export * as ns` chains.
    pub(super) whole_module_targets: &'a mut super::re_exports::WholeModuleObservations,
}

impl AttachContext<'_> {
    /// Record that a consumer in this graph observed `target`'s whole
    /// namespace object.
    ///
    /// Every namespace mark-all site must call this or one of the ambient
    /// observation methods so the exposed namespace closure sees the same
    /// seed set the marks credited; the one
    /// deliberate exception is a binding the namespace-object alias phase
    /// narrows on the consumer's behalf. The seed is namespace-agnostic on
    /// purpose: the object exposes the same names in the type and value
    /// namespaces, and `typeof ns.member` keeps a value declaration reachable
    /// through a type-only observation.
    fn observe_whole_namespace_object(&mut self, target: FileId) {
        self.whole_module_targets.observe(target);
    }

    /// Record that an ambient `declare module '...'` body re-exports every
    /// name of `target` under an external module id (issue #2357).
    ///
    /// Separate from [`AttachContext::observe_whole_namespace_object`]
    /// because the observers live outside this graph: the seed stands however
    /// the shim and the target sit in it.
    fn observe_ambient_star(&mut self, target: FileId) {
        self.whole_module_targets.observe_ambient_star(target);
    }

    /// Record that an ambient `export * as ns` exposes `target`'s namespace
    /// object, including its default member.
    fn observe_ambient_namespace_object(&mut self, target: FileId) {
        self.whole_module_targets.observe_ambient_namespace(target);
    }

    fn reborrow(&mut self) -> AttachContext<'_> {
        AttachContext {
            module_by_id: self.module_by_id,
            entry_point_ids: self.entry_point_ids,
            export_index: self.export_index,
            effective_exports: self.effective_exports,
            dedup: self.dedup,
            whole_module_targets: self.whole_module_targets,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ReferenceSite {
    from_file: FileId,
    import_span: oxc_span::Span,
    path: Option<ReferencePathId>,
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

fn ambient_star_observes_namespace_object(
    sym: &ImportedSymbol,
    source_mod: Option<&&ResolvedModule>,
) -> bool {
    source_mod.is_some_and(|module| {
        module.resolved_imports.iter().any(|import| {
            import.info.span == sym.import_span
                && import.info.local_name.is_empty()
                && import.info.is_type_only
                && matches!(import.info.imported_name, ImportedName::Default)
        })
    })
}

fn is_single_static_cjs_object_map(module: &ResolvedModule) -> bool {
    module
        .semantic_facts
        .iter()
        .any(|fact| matches!(fact, SemanticFact::CjsSingleStaticObjectMap))
}

fn default_import_has_whole_object_use(module: &ResolvedModule, local_name: &str) -> bool {
    module.semantic_facts.iter().any(|fact| {
        matches!(
            fact,
            SemanticFact::DefaultImportWholeObjectUse(use_fact)
                if use_fact.local_name == local_name
        )
    })
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

/// Mark every export slot of the target, `default` included, as referenced
/// from `context.site`.
pub(super) fn mark_all_exports_referenced_at_site(
    exports: &mut [ExportSymbol],
    context: &mut NamespaceMarkContext<'_>,
) {
    mark_exports_referenced_at_site(exports, context, true);
}

/// Mark every named export as referenced: the surface an ES `export *`
/// forwards, which never includes `default`.
fn mark_star_surface_referenced_at_site(
    exports: &mut [ExportSymbol],
    context: &mut NamespaceMarkContext<'_>,
) {
    mark_exports_referenced_at_site(exports, context, false);
}

fn mark_exports_referenced_at_site(
    exports: &mut [ExportSymbol],
    context: &mut NamespaceMarkContext<'_>,
    include_default: bool,
) {
    for idx in 0..exports.len() {
        let name = match &exports[idx].name {
            fallow_types::extract::ExportName::Named(name) => name.as_str(),
            fallow_types::extract::ExportName::Default if include_default => "default",
            fallow_types::extract::ExportName::Default => continue,
        };
        if !context.effective_exports.is_declaration_slot(
            exports,
            context.module_id,
            name,
            context.namespace,
            idx,
        ) {
            continue;
        }
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
                .insert((site.from_file, site.import_span, site.path, namespace))
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            if export.references.len() < REFERENCE_DEDUP_THRESHOLD {
                !export.has_reference_from(site.from_file, site.import_span, site.path, namespace)
            } else {
                let seen = entry.insert(
                    export
                        .routed_references()
                        .map(|routed| {
                            (
                                routed.reference.from_file,
                                routed.reference.import_span,
                                routed.path,
                                routed.reference.namespace,
                            )
                        })
                        .collect(),
                );
                seen.insert((site.from_file, site.import_span, site.path, namespace))
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

/// Mark only the export slots whose names appear in `accessed_members`.
///
/// Returns the member names that matched an export declaration slot.
pub(super) fn mark_member_exports_referenced_at_site(
    exports: &mut [ExportSymbol],
    accessed_members: &[String],
    context: &mut NamespaceMarkContext<'_>,
) -> FxHashSet<String> {
    let member_set: FxHashSet<&str> = accessed_members.iter().map(String::as_str).collect();
    let mut found_members: FxHashSet<String> = FxHashSet::default();
    for idx in 0..exports.len() {
        let name_str = match &exports[idx].name {
            fallow_types::extract::ExportName::Named(n) => n.as_str(),
            fallow_types::extract::ExportName::Default => "default",
        };
        if !member_set.contains(name_str)
            || !context.effective_exports.is_declaration_slot(
                exports,
                context.module_id,
                name_str,
                context.namespace,
                idx,
            )
        {
            continue;
        }
        found_members.insert(name_str.to_owned());
        attach_reference(
            &mut exports[idx],
            (context.module_id, idx),
            context.site,
            context.kind,
            context.namespace,
            context.dedup,
        );
    }
    found_members
}

/// Create synthetic `ExportSymbol` entries for members accessed via namespace import
/// that were not found among the target's own exports, but the target has `export *`
/// re-exports that may forward those names.
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
            if !export.has_reference_from(site.from_file, site.import_span, site.path, namespace) {
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
            deprecated: false,
            deprecated_reason: None,
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
    let read_members = extract_accessed_members(source_mod, sym_local_name);
    let mut accessed_members = read_members.clone();
    if !accessed_members.is_empty() && module.forwards_default_as_module_object() {
        accessed_members.push("default".to_string());
    }

    let is_whole_object = source_mod.is_some_and(|module| {
        module
            .whole_object_uses
            .iter()
            .any(|name| name == sym_local_name)
    });

    // `export { ns }` hands the namespace object itself to consumers the graph
    // cannot enumerate, on an entry point as much as on any other module: the
    // binding is the public API there (issue #2373).
    let is_re_exported = source_mod.is_some_and(|m| {
        m.exports
            .iter()
            .any(|e| e.local_name.as_deref() == Some(sym_local_name))
    });
    let namespaces = if is_re_exported && namespaces.1 {
        (true, true)
    } else {
        namespaces
    };

    let is_entry_with_no_access = accessed_members.is_empty()
        && !is_whole_object
        && !is_re_exported
        && ctx.entry_point_ids.contains(&site.from_file);

    // The consumer observes the whole namespace object (a whole-object use,
    // no member access the graph can narrow to, or a binding re-exported to
    // consumers it cannot enumerate): every name on the object is credited,
    // including the names the target only exposes through its own `export *`
    // and `export * as ns` chains, which the exposed-namespace closure
    // handles downstream (issue #2372). A binding placed in an exported
    // object literal (`export const API = { ns }`) keeps the direct-export
    // credit but never seeds that closure: the namespace-object alias phase
    // follows `API.ns.<member>` accesses precisely, so the chain behind it
    // stays narrowed unless the binding is also used as a whole object or
    // exported under its own name.
    let observes_whole_module = is_whole_object
        || (!is_entry_with_no_access && (accessed_members.is_empty() || is_re_exported));
    let is_alias_source = || {
        source_mod.is_some_and(|m| {
            m.namespace_object_aliases
                .iter()
                .any(|alias| alias.namespace_local == sym_local_name)
        })
    };
    if observes_whole_module && (is_whole_object || is_re_exported || !is_alias_source()) {
        ctx.observe_whole_namespace_object(module.file_id);
    }
    record_namespace_escape(
        module.file_id,
        site,
        sym_local_name,
        EscapeShape {
            gets_full_credit: observes_whole_module && (namespaces.0 || namespaces.1),
            is_whole_object,
            is_re_exported,
        },
        &read_members,
        ctx,
    );

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
        if observes_whole_module {
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

/// How a namespace binding that gets the full credit is used.
#[derive(Clone, Copy)]
struct EscapeShape {
    /// The site credits every export in at least one namespace lane.
    gets_full_credit: bool,
    is_whole_object: bool,
    is_re_exported: bool,
}

/// Record the namespace binding of `site` as an escape of `target`.
///
/// `read_members` holds the members that the code reads by name. The full
/// credit does not hide them.
fn record_namespace_escape(
    target: FileId,
    site: ReferenceSite,
    local_name: &str,
    shape: EscapeShape,
    read_members: &[String],
    ctx: &mut AttachContext<'_>,
) {
    if !shape.gets_full_credit {
        return;
    }
    let reason = if shape.is_whole_object {
        NamespaceEscapeReason::WholeObjectUse
    } else if shape.is_re_exported {
        NamespaceEscapeReason::ReExportedBinding
    } else {
        NamespaceEscapeReason::NoMemberRead
    };
    ctx.whole_module_targets.record_escape(NamespaceEscape {
        consumer: site.from_file,
        import_span: site.import_span,
        local_name: local_name.to_string(),
        target,
        reason,
        read_members: read_members.to_vec(),
    });
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
    let is_whole_object = source_mod.is_some_and(|module| {
        module
            .whole_object_uses
            .iter()
            .any(|name| name == sym_local_name)
            || default_import_has_whole_object_use(module, sym_local_name)
    });
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
    target_name: &ImportedName,
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
    let matching_exports: &[usize] = export_index.matches(target_name);

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

    let usage = desired_import_namespaces(sym, source_mod);
    let imported_name = match target_name {
        ImportedName::Named(name) => name.as_str(),
        ImportedName::Default => "default",
        ImportedName::Namespace | ImportedName::SideEffect => return,
    };
    let (uses_type, uses_value) = resolved_import_namespaces(
        usage,
        effective_exports,
        target_module.file_id,
        imported_name,
    );
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
            } else if namespace == ExportNamespace::Type
                && !effective_exports
                    .declaration_group_slots(binding)
                    .is_empty()
            {
                effective_exports.declaration_group_slots(binding)
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

/// Which semantic namespaces an imported binding uses.
///
/// `classified` is false when the extractor recorded no type or value usage for
/// the binding and the value namespace is only the default guess.
#[derive(Clone, Copy)]
struct ImportNamespaceUse {
    uses_type: bool,
    uses_value: bool,
    classified: bool,
}

impl ImportNamespaceUse {
    const fn namespaces(self) -> (bool, bool) {
        (self.uses_type, self.uses_value)
    }
}

/// Decide which semantic namespaces the imported binding uses.
///
/// A type-only import credits type space: `import type { x }` narrows its
/// binding with the `type` keyword, and a type-only import without a binding
/// (an ambient named re-export, an `import()` type reference) stays there as
/// well. The one exception is the plain ambient star form (issue #2357):
/// `export *` inside a `declare module '...'` body forwards every export of
/// the target in both meanings, so it credits both namespaces. Its
/// `export type *` spelling (issue #2375) forwards the same names with every
/// value meaning erased, so it stays in type space with the rest.
fn desired_import_namespaces(
    sym: &ImportedSymbol,
    source_mod: Option<&&ResolvedModule>,
) -> ImportNamespaceUse {
    if sym.is_type_only {
        return ImportNamespaceUse {
            uses_type: true,
            uses_value: sym.is_value_bearing_ambient_star(),
            classified: true,
        };
    }
    let uses_type = import_binding_has_type_usage(source_mod, &sym.local_name);
    let uses_value = import_binding_has_value_usage(source_mod, &sym.local_name);
    if uses_type || uses_value {
        return ImportNamespaceUse {
            uses_type,
            uses_value,
            classified: true,
        };
    }
    ImportNamespaceUse {
        uses_type: false,
        uses_value: true,
        classified: false,
    }
}

/// Keep an unclassified binding on the only namespace that actually resolves.
///
/// The value guess would otherwise attach nothing when the imported name exists
/// only as a type, silently un-referencing a type-only export.
fn resolved_import_namespaces(
    usage: ImportNamespaceUse,
    effective_exports: &super::effective_exports::EffectiveExportIndex,
    file_id: FileId,
    imported_name: &str,
) -> (bool, bool) {
    if usage.classified {
        return usage.namespaces();
    }
    let resolves = |namespace| {
        matches!(
            effective_exports.resolve(file_id, imported_name, namespace),
            super::EffectiveExportResolution::Unique(_)
        )
    };
    if !resolves(ExportNamespace::Value) && resolves(ExportNamespace::Type) {
        return (true, false);
    }
    usage.namespaces()
}

/// Whether a named import of `imported_name` from `target_module` reads a
/// member of the default export that an `export = x` forward makes the module
/// object: `import { run } from './fwd'` or `const { run } = require('./fwd')`
/// (issue #3392). A name that the module exports itself resolves directly.
fn reads_member_of_forwarded_default(
    target_module: &ModuleNode,
    imported_name: &ImportedName,
    ctx: &AttachContext<'_>,
) -> bool {
    matches!(imported_name, ImportedName::Named(name) if name != "default")
        && target_module.forwards_default_as_module_object()
        && ctx.export_index.matches(imported_name).is_empty()
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
    let is_default_import = matches!(sym.imported_name, ImportedName::Default)
        || matches!(&sym.imported_name, ImportedName::Named(name) if name == "default");
    let narrows_default_object_map = is_default_import
        && !sym.local_name.is_empty()
        && ctx
            .module_by_id
            .get(&target_module.file_id)
            .is_some_and(|module| is_single_static_cjs_object_map(module));
    if !narrows_default_object_map {
        attach_direct_export_references(
            target_module,
            site,
            sym,
            &sym.imported_name,
            ref_kind,
            ctx.reborrow(),
        );
    }
    if reads_member_of_forwarded_default(target_module, &sym.imported_name, &ctx) {
        attach_direct_export_references(
            target_module,
            site,
            sym,
            &ImportedName::Default,
            ref_kind,
            ctx.reborrow(),
        );
    }

    if matches!(sym.imported_name, ImportedName::Namespace) {
        if sym.local_name.is_empty() {
            // A runtime whole-module edge (a dynamic-import pattern) hands the
            // consumer the module namespace object, `default` included. The
            // type-only form is `export *` inside an ambient module body
            // (issue #2357), which forwards the ES star surface: every named
            // export and never `default`. The `export * as ns` form records
            // the namespace object's `default` member as a separate import.
            // Its `export type *` spelling forwards the same surface in the
            // type namespace alone (issue #2375), a lane
            // `desired_import_namespaces` decides; the surface itself, and the
            // closure seed below, are the same either way.
            // Both observe the whole module, so the target's own `export *`
            // and `export * as ns` chains are credited downstream through the
            // exposed-namespace closure (issue #2372 for the runtime form).
            // The ambient form seeds that closure at any reachability: its
            // observers are importers of the declared module id, not files in
            // this graph.
            if sym.is_ambient_star() {
                if ambient_star_observes_namespace_object(sym, source_mod) {
                    ctx.observe_ambient_namespace_object(target_module.file_id);
                } else {
                    ctx.observe_ambient_star(target_module.file_id);
                }
            } else {
                ctx.observe_whole_namespace_object(target_module.file_id);
            }
            let namespaces = desired_import_namespaces(sym, source_mod).namespaces();
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
                    if sym.is_ambient_star() {
                        mark_star_surface_referenced_at_site(&mut target_module.exports, &mut mark);
                    } else {
                        mark_all_exports_referenced_at_site(&mut target_module.exports, &mut mark);
                    }
                }
            }
        } else {
            narrow_namespace_references(
                target_module,
                site,
                &sym.local_name,
                desired_import_namespaces(sym, source_mod).namespaces(),
                &mut ctx,
            );
        }
    }

    if is_default_import
        && !sym.local_name.is_empty()
        && (is_css_module_path(&target_module.path) || narrows_default_object_map)
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
#[path = "narrowing_tests.rs"]
mod tests;
