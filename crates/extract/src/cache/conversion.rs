//! Conversion between [`ModuleInfo`](crate::ModuleInfo) and [`CachedModule`].
//!
//! Both functions convert between borrowed source structs and owned target structs
//! (`&CachedModule -> ModuleInfo`, `&ModuleInfo -> CachedModule`). All `String` clones
//! are structurally necessary: the cache store retains ownership of `CachedModule`
//! entries (for persistence), and `ModuleInfo` must outlive the cache for the
//! analysis pipeline. Eliminating these clones would require shared ownership
//! (`Arc<str>`) across the entire extraction + analysis pipeline.

use oxc_span::Span;

use crate::ExportName;
use fallow_types::extract::VisibilityTag;

use super::types::{
    CachedDynamicImport, CachedDynamicImportPattern, CachedExport, CachedImport,
    CachedLocalTypeDeclaration, CachedMember, CachedModule, CachedPublicSignatureTypeReference,
    CachedReExport, CachedRequireCall, CachedSuppression, IMPORT_KIND_DEFAULT, IMPORT_KIND_NAMED,
    IMPORT_KIND_NAMESPACE, IMPORT_KIND_SIDE_EFFECT,
};

/// Reconstruct a [`ModuleInfo`](crate::ModuleInfo) from a [`CachedModule`].
#[must_use]
pub fn cached_to_module(
    cached: &CachedModule,
    file_id: fallow_types::discover::FileId,
) -> crate::ModuleInfo {
    use crate::{
        DynamicImportInfo, ExportInfo, ImportInfo, ImportedName, LocalTypeDeclaration, MemberInfo,
        ModuleInfo, PublicSignatureTypeReference, ReExportInfo, RequireCallInfo,
    };

    let exports = cached
        .exports
        .iter()
        .map(|e| ExportInfo {
            name: if e.is_default {
                ExportName::Default
            } else {
                ExportName::Named(e.name.clone())
            },
            local_name: e.local_name.clone(),
            is_type_only: e.is_type_only,
            is_side_effect_used: e.is_side_effect_used,
            visibility: match e.visibility {
                1 => VisibilityTag::Public,
                2 => VisibilityTag::Internal,
                3 => VisibilityTag::Beta,
                4 => VisibilityTag::Alpha,
                5 => VisibilityTag::ExpectedUnused,
                _ => VisibilityTag::None,
            },
            span: Span::new(e.span_start, e.span_end),
            members: e
                .members
                .iter()
                .map(|m| MemberInfo {
                    name: m.name.clone(),
                    kind: m.kind,
                    span: Span::new(m.span_start, m.span_end),
                    has_decorator: m.has_decorator,
                })
                .collect(),
            super_class: e.super_class.clone(),
        })
        .collect();

    let imports = cached
        .imports
        .iter()
        .map(|i| ImportInfo {
            source: i.source.clone(),
            imported_name: match i.kind {
                IMPORT_KIND_DEFAULT => ImportedName::Default,
                IMPORT_KIND_NAMESPACE => ImportedName::Namespace,
                IMPORT_KIND_SIDE_EFFECT => ImportedName::SideEffect,
                // IMPORT_KIND_NAMED (0) and any unknown value default to Named
                _ => ImportedName::Named(i.imported_name.clone()),
            },
            local_name: i.local_name.clone(),
            is_type_only: i.is_type_only,
            from_style: i.from_style,
            span: Span::new(i.span_start, i.span_end),
            source_span: Span::new(i.source_span_start, i.source_span_end),
        })
        .collect();

    let re_exports = cached
        .re_exports
        .iter()
        .map(|r| ReExportInfo {
            source: r.source.clone(),
            imported_name: r.imported_name.clone(),
            exported_name: r.exported_name.clone(),
            is_type_only: r.is_type_only,
            span: Span::new(r.span_start, r.span_end),
        })
        .collect();

    let dynamic_imports = cached
        .dynamic_imports
        .iter()
        .map(|d| DynamicImportInfo {
            source: d.source.clone(),
            span: Span::new(d.span_start, d.span_end),
            destructured_names: d.destructured_names.clone(),
            local_name: d.local_name.clone(),
        })
        .collect();

    let require_calls = cached
        .require_calls
        .iter()
        .map(|r| RequireCallInfo {
            source: r.source.clone(),
            span: Span::new(r.span_start, r.span_end),
            destructured_names: r.destructured_names.clone(),
            local_name: r.local_name.clone(),
        })
        .collect();

    let dynamic_import_patterns = cached
        .dynamic_import_patterns
        .iter()
        .map(|p| crate::DynamicImportPattern {
            prefix: p.prefix.clone(),
            suffix: p.suffix.clone(),
            span: Span::new(p.span_start, p.span_end),
        })
        .collect();

    let suppressions = cached
        .suppressions
        .iter()
        .map(|s| crate::suppress::Suppression {
            line: s.line,
            comment_line: s.comment_line,
            kind: if s.kind == 0 {
                None
            } else {
                crate::suppress::IssueKind::from_discriminant(s.kind)
            },
        })
        .collect();

    ModuleInfo {
        file_id,
        exports,
        imports,
        re_exports,
        dynamic_imports,
        dynamic_import_patterns,
        require_calls,
        member_accesses: cached.member_accesses.clone(),
        whole_object_uses: cached.whole_object_uses.clone(),
        has_cjs_exports: cached.has_cjs_exports,
        content_hash: cached.content_hash,
        suppressions,
        unused_import_bindings: cached.unused_import_bindings.clone(),
        type_referenced_import_bindings: cached.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: cached.value_referenced_import_bindings.clone(),
        line_offsets: cached.line_offsets.clone(),
        complexity: cached.complexity.clone(),
        flag_uses: cached.flag_uses.clone(),
        class_heritage: cached.class_heritage.clone(),
        local_type_declarations: cached
            .local_type_declarations
            .iter()
            .map(|decl| LocalTypeDeclaration {
                name: decl.name.clone(),
                span: Span::new(decl.span_start, decl.span_end),
            })
            .collect(),
        public_signature_type_references: cached
            .public_signature_type_references
            .iter()
            .map(|reference| PublicSignatureTypeReference {
                export_name: reference.export_name.clone(),
                type_name: reference.type_name.clone(),
                span: Span::new(reference.span_start, reference.span_end),
            })
            .collect(),
    }
}

/// Convert a [`ModuleInfo`](crate::ModuleInfo) to a [`CachedModule`] for storage.
///
/// `mtime_secs` and `file_size` come from `std::fs::metadata()` at parse time
/// and enable fast cache validation on subsequent runs (skip file read when
/// mtime+size match).
#[must_use]
pub fn module_to_cached(
    module: &crate::ModuleInfo,
    mtime_secs: u64,
    file_size: u64,
) -> CachedModule {
    CachedModule {
        content_hash: module.content_hash,
        mtime_secs,
        file_size,
        exports: module
            .exports
            .iter()
            .map(|e| CachedExport {
                name: match &e.name {
                    ExportName::Named(n) => n.clone(),
                    ExportName::Default => String::new(),
                },
                is_default: matches!(e.name, ExportName::Default),
                is_type_only: e.is_type_only,
                is_side_effect_used: e.is_side_effect_used,
                visibility: e.visibility as u8,
                local_name: e.local_name.clone(),
                span_start: e.span.start,
                span_end: e.span.end,
                members: e
                    .members
                    .iter()
                    .map(|m| CachedMember {
                        name: m.name.clone(),
                        kind: m.kind,
                        span_start: m.span.start,
                        span_end: m.span.end,
                        has_decorator: m.has_decorator,
                    })
                    .collect(),
                super_class: e.super_class.clone(),
            })
            .collect(),
        imports: module
            .imports
            .iter()
            .map(|i| {
                let (kind, imported_name) = match &i.imported_name {
                    crate::ImportedName::Named(n) => (IMPORT_KIND_NAMED, n.clone()),
                    crate::ImportedName::Default => (IMPORT_KIND_DEFAULT, String::new()),
                    crate::ImportedName::Namespace => (IMPORT_KIND_NAMESPACE, String::new()),
                    crate::ImportedName::SideEffect => (IMPORT_KIND_SIDE_EFFECT, String::new()),
                };
                CachedImport {
                    source: i.source.clone(),
                    imported_name,
                    local_name: i.local_name.clone(),
                    is_type_only: i.is_type_only,
                    from_style: i.from_style,
                    kind,
                    span_start: i.span.start,
                    span_end: i.span.end,
                    source_span_start: i.source_span.start,
                    source_span_end: i.source_span.end,
                }
            })
            .collect(),
        re_exports: module
            .re_exports
            .iter()
            .map(|r| CachedReExport {
                source: r.source.clone(),
                imported_name: r.imported_name.clone(),
                exported_name: r.exported_name.clone(),
                is_type_only: r.is_type_only,
                span_start: r.span.start,
                span_end: r.span.end,
            })
            .collect(),
        dynamic_imports: module
            .dynamic_imports
            .iter()
            .map(|d| CachedDynamicImport {
                source: d.source.clone(),
                span_start: d.span.start,
                span_end: d.span.end,
                destructured_names: d.destructured_names.clone(),
                local_name: d.local_name.clone(),
            })
            .collect(),
        require_calls: module
            .require_calls
            .iter()
            .map(|r| CachedRequireCall {
                source: r.source.clone(),
                span_start: r.span.start,
                span_end: r.span.end,
                destructured_names: r.destructured_names.clone(),
                local_name: r.local_name.clone(),
            })
            .collect(),
        member_accesses: module.member_accesses.clone(),
        whole_object_uses: module.whole_object_uses.clone(),
        dynamic_import_patterns: module
            .dynamic_import_patterns
            .iter()
            .map(|p| CachedDynamicImportPattern {
                prefix: p.prefix.clone(),
                suffix: p.suffix.clone(),
                span_start: p.span.start,
                span_end: p.span.end,
            })
            .collect(),
        has_cjs_exports: module.has_cjs_exports,
        unused_import_bindings: module.unused_import_bindings.clone(),
        type_referenced_import_bindings: module.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: module.value_referenced_import_bindings.clone(),
        suppressions: module
            .suppressions
            .iter()
            .map(|s| CachedSuppression {
                line: s.line,
                comment_line: s.comment_line,
                kind: s
                    .kind
                    .map_or(0, crate::suppress::IssueKind::to_discriminant),
            })
            .collect(),
        line_offsets: module.line_offsets.clone(),
        complexity: module.complexity.clone(),
        flag_uses: module.flag_uses.clone(),
        class_heritage: module.class_heritage.clone(),
        local_type_declarations: module
            .local_type_declarations
            .iter()
            .map(|decl| CachedLocalTypeDeclaration {
                name: decl.name.clone(),
                span_start: decl.span.start,
                span_end: decl.span.end,
            })
            .collect(),
        public_signature_type_references: module
            .public_signature_type_references
            .iter()
            .map(|reference| CachedPublicSignatureTypeReference {
                export_name: reference.export_name.clone(),
                type_name: reference.type_name.clone(),
                span_start: reference.span.start,
                span_end: reference.span.end,
            })
            .collect(),
    }
}
