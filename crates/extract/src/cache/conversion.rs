//! Conversion between [`ModuleInfo`](crate::ModuleInfo) and [`CachedModule`].
//!
//! Both functions convert between borrowed source structs and owned target structs
//! (`&CachedModule -> ModuleInfo`, `&ModuleInfo -> CachedModule`). All `String` clones
//! are structurally necessary: the cache store retains ownership of `CachedModule`
//! entries (for persistence), and `ModuleInfo` must outlive the cache for the
//! analysis pipeline. Eliminating these clones would require shared ownership
//! (`Arc<str>`) across the entire extraction + analysis pipeline.

use std::time::{SystemTime, UNIX_EPOCH};

use oxc_span::Span;

use crate::ExportName;
use fallow_types::extract::{NamespaceObjectAlias, VisibilityTag};
use fallow_types::suppress::{PolicyRuleSuppression, SuppressionTarget};

/// Seconds-since-Unix-epoch from the wall clock, saturating to 0 if the
/// system clock is set before the epoch. Used as the LRU bookkeeping
/// timestamp on `CachedModule.last_access_secs`. Wall-clock (not monotonic)
/// is the right source here because the value persists across process
/// invocations.
#[must_use]
pub fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

use super::types::{
    CachedDynamicImport, CachedDynamicImportPattern, CachedExport, CachedImport,
    CachedLocalTypeDeclaration, CachedMember, CachedModule, CachedNamespaceObjectAlias,
    CachedPublicSignatureTypeReference, CachedReExport, CachedRequireCall, CachedSuppression,
    CachedUnknownSuppressionKind, IMPORT_KIND_DEFAULT, IMPORT_KIND_NAMED, IMPORT_KIND_NAMESPACE,
    IMPORT_KIND_SIDE_EFFECT,
};

/// Reconstruct a [`ModuleInfo`](crate::ModuleInfo) from a [`CachedModule`].
#[must_use]
pub fn cached_to_module(
    cached: &CachedModule,
    file_id: fallow_types::discover::FileId,
) -> crate::ModuleInfo {
    cached_to_module_opts(cached, file_id, true)
}

fn cached_exports_to_module(exports: &[CachedExport]) -> Vec<crate::ExportInfo> {
    exports
        .iter()
        .map(|export| crate::ExportInfo {
            name: if export.is_default {
                ExportName::Default
            } else {
                ExportName::Named(export.name.clone())
            },
            local_name: export.local_name.clone(),
            is_type_only: export.is_type_only,
            is_side_effect_used: export.is_side_effect_used,
            visibility: match export.visibility {
                1 => VisibilityTag::Public,
                2 => VisibilityTag::Internal,
                3 => VisibilityTag::Beta,
                4 => VisibilityTag::Alpha,
                5 => VisibilityTag::ExpectedUnused,
                _ => VisibilityTag::None,
            },
            span: Span::new(export.span_start, export.span_end),
            members: export
                .members
                .iter()
                .map(|member| crate::MemberInfo {
                    name: member.name.clone(),
                    kind: member.kind,
                    span: Span::new(member.span_start, member.span_end),
                    has_decorator: member.has_decorator,
                    decorator_names: member.decorator_names.clone(),
                    is_instance_returning_static: member.is_instance_returning_static,
                    is_self_returning: member.is_self_returning,
                })
                .collect(),
            super_class: export.super_class.clone(),
        })
        .collect()
}

fn cached_imports_to_module(imports: &[CachedImport]) -> Vec<crate::ImportInfo> {
    imports
        .iter()
        .map(|import| crate::ImportInfo {
            source: import.source.clone(),
            imported_name: match import.kind {
                IMPORT_KIND_DEFAULT => crate::ImportedName::Default,
                IMPORT_KIND_NAMESPACE => crate::ImportedName::Namespace,
                IMPORT_KIND_SIDE_EFFECT => crate::ImportedName::SideEffect,
                _ => crate::ImportedName::Named(import.imported_name.clone()),
            },
            local_name: import.local_name.clone(),
            is_type_only: import.is_type_only,
            from_style: import.from_style,
            span: Span::new(import.span_start, import.span_end),
            source_span: Span::new(import.source_span_start, import.source_span_end),
        })
        .collect()
}

fn cached_re_exports_to_module(re_exports: &[CachedReExport]) -> Vec<crate::ReExportInfo> {
    re_exports
        .iter()
        .map(|re_export| crate::ReExportInfo {
            source: re_export.source.clone(),
            imported_name: re_export.imported_name.clone(),
            exported_name: re_export.exported_name.clone(),
            is_type_only: re_export.is_type_only,
            span: Span::new(re_export.span_start, re_export.span_end),
        })
        .collect()
}

fn cached_dynamic_imports_to_module(
    dynamic_imports: &[CachedDynamicImport],
) -> Vec<crate::DynamicImportInfo> {
    dynamic_imports
        .iter()
        .map(|dynamic_import| crate::DynamicImportInfo {
            source: dynamic_import.source.clone(),
            span: Span::new(dynamic_import.span_start, dynamic_import.span_end),
            destructured_names: dynamic_import.destructured_names.clone(),
            local_name: dynamic_import.local_name.clone(),
            is_speculative: dynamic_import.is_speculative,
        })
        .collect()
}

fn cached_require_calls_to_module(
    require_calls: &[CachedRequireCall],
) -> Vec<crate::RequireCallInfo> {
    require_calls
        .iter()
        .map(|require_call| crate::RequireCallInfo {
            source: require_call.source.clone(),
            span: Span::new(require_call.span_start, require_call.span_end),
            source_span: Span::new(require_call.source_span_start, require_call.source_span_end),
            destructured_names: require_call.destructured_names.clone(),
            local_name: require_call.local_name.clone(),
        })
        .collect()
}

fn cached_dynamic_patterns_to_module(
    dynamic_import_patterns: &[CachedDynamicImportPattern],
) -> Vec<crate::DynamicImportPattern> {
    dynamic_import_patterns
        .iter()
        .map(|pattern| crate::DynamicImportPattern {
            prefix: pattern.prefix.clone(),
            suffix: pattern.suffix.clone(),
            span: Span::new(pattern.span_start, pattern.span_end),
        })
        .collect()
}

fn cached_suppressions_to_module(
    suppressions: &[CachedSuppression],
) -> Vec<crate::suppress::Suppression> {
    suppressions
        .iter()
        .map(|suppression| {
            let target = if suppression.kind == 0 {
                None
            } else if suppression.kind
                == crate::suppress::IssueKind::PolicyViolation.to_discriminant()
                && !suppression.policy_pack.is_empty()
                && !suppression.policy_rule_id.is_empty()
            {
                Some(SuppressionTarget::PolicyRule(PolicyRuleSuppression::new(
                    suppression.policy_pack.clone(),
                    suppression.policy_rule_id.clone(),
                )))
            } else {
                crate::suppress::IssueKind::from_discriminant(suppression.kind)
                    .map(SuppressionTarget::Issue)
            };
            crate::suppress::Suppression {
                line: suppression.line,
                comment_line: suppression.comment_line,
                target,
            }
        })
        .collect()
}

fn cached_unknown_suppressions_to_module(
    unknown_suppression_kinds: &[CachedUnknownSuppressionKind],
) -> Vec<fallow_types::suppress::UnknownSuppressionKind> {
    unknown_suppression_kinds
        .iter()
        .map(|unknown| fallow_types::suppress::UnknownSuppressionKind {
            comment_line: unknown.comment_line,
            is_file_level: unknown.is_file_level,
            token: unknown.token.clone(),
        })
        .collect()
}

fn cached_local_types_to_module(
    local_type_declarations: &[CachedLocalTypeDeclaration],
) -> Vec<crate::LocalTypeDeclaration> {
    local_type_declarations
        .iter()
        .map(|declaration| crate::LocalTypeDeclaration {
            name: declaration.name.clone(),
            span: Span::new(declaration.span_start, declaration.span_end),
        })
        .collect()
}

fn cached_signature_refs_to_module(
    public_signature_type_references: &[CachedPublicSignatureTypeReference],
) -> Vec<crate::PublicSignatureTypeReference> {
    public_signature_type_references
        .iter()
        .map(|reference| crate::PublicSignatureTypeReference {
            export_name: reference.export_name.clone(),
            type_name: reference.type_name.clone(),
            span: Span::new(reference.span_start, reference.span_end),
        })
        .collect()
}

fn cached_namespace_aliases_to_module(
    namespace_object_aliases: &[CachedNamespaceObjectAlias],
) -> Vec<NamespaceObjectAlias> {
    namespace_object_aliases
        .iter()
        .map(|alias| NamespaceObjectAlias {
            via_export_name: alias.via_export_name.clone(),
            suffix: alias.suffix.clone(),
            namespace_local: alias.namespace_local.clone(),
        })
        .collect()
}

fn module_exports_to_cached(exports: &[crate::ExportInfo]) -> Vec<CachedExport> {
    exports
        .iter()
        .map(|export| CachedExport {
            name: match &export.name {
                ExportName::Named(name) => name.clone(),
                ExportName::Default => String::new(),
            },
            is_default: matches!(export.name, ExportName::Default),
            is_type_only: export.is_type_only,
            is_side_effect_used: export.is_side_effect_used,
            visibility: export.visibility as u8,
            local_name: export.local_name.clone(),
            span_start: export.span.start,
            span_end: export.span.end,
            members: export
                .members
                .iter()
                .map(|member| CachedMember {
                    name: member.name.clone(),
                    kind: member.kind,
                    span_start: member.span.start,
                    span_end: member.span.end,
                    has_decorator: member.has_decorator,
                    decorator_names: member.decorator_names.clone(),
                    is_instance_returning_static: member.is_instance_returning_static,
                    is_self_returning: member.is_self_returning,
                })
                .collect(),
            super_class: export.super_class.clone(),
        })
        .collect()
}

fn module_imports_to_cached(imports: &[crate::ImportInfo]) -> Vec<CachedImport> {
    imports
        .iter()
        .map(|import| {
            let (kind, imported_name) = match &import.imported_name {
                crate::ImportedName::Named(name) => (IMPORT_KIND_NAMED, name.clone()),
                crate::ImportedName::Default => (IMPORT_KIND_DEFAULT, String::new()),
                crate::ImportedName::Namespace => (IMPORT_KIND_NAMESPACE, String::new()),
                crate::ImportedName::SideEffect => (IMPORT_KIND_SIDE_EFFECT, String::new()),
            };
            CachedImport {
                source: import.source.clone(),
                imported_name,
                local_name: import.local_name.clone(),
                is_type_only: import.is_type_only,
                from_style: import.from_style,
                kind,
                span_start: import.span.start,
                span_end: import.span.end,
                source_span_start: import.source_span.start,
                source_span_end: import.source_span.end,
            }
        })
        .collect()
}

fn module_re_exports_to_cached(re_exports: &[crate::ReExportInfo]) -> Vec<CachedReExport> {
    re_exports
        .iter()
        .map(|re_export| CachedReExport {
            source: re_export.source.clone(),
            imported_name: re_export.imported_name.clone(),
            exported_name: re_export.exported_name.clone(),
            is_type_only: re_export.is_type_only,
            span_start: re_export.span.start,
            span_end: re_export.span.end,
        })
        .collect()
}

fn module_dynamic_imports_to_cached(
    dynamic_imports: &[crate::DynamicImportInfo],
) -> Vec<CachedDynamicImport> {
    dynamic_imports
        .iter()
        .map(|dynamic_import| CachedDynamicImport {
            source: dynamic_import.source.clone(),
            span_start: dynamic_import.span.start,
            span_end: dynamic_import.span.end,
            destructured_names: dynamic_import.destructured_names.clone(),
            local_name: dynamic_import.local_name.clone(),
            is_speculative: dynamic_import.is_speculative,
        })
        .collect()
}

fn module_require_calls_to_cached(
    require_calls: &[crate::RequireCallInfo],
) -> Vec<CachedRequireCall> {
    require_calls
        .iter()
        .map(|require_call| CachedRequireCall {
            source: require_call.source.clone(),
            span_start: require_call.span.start,
            span_end: require_call.span.end,
            source_span_start: require_call.source_span.start,
            source_span_end: require_call.source_span.end,
            destructured_names: require_call.destructured_names.clone(),
            local_name: require_call.local_name.clone(),
        })
        .collect()
}

fn module_dynamic_patterns_to_cached(
    dynamic_import_patterns: &[crate::DynamicImportPattern],
) -> Vec<CachedDynamicImportPattern> {
    dynamic_import_patterns
        .iter()
        .map(|pattern| CachedDynamicImportPattern {
            prefix: pattern.prefix.clone(),
            suffix: pattern.suffix.clone(),
            span_start: pattern.span.start,
            span_end: pattern.span.end,
        })
        .collect()
}

fn module_suppressions_to_cached(
    suppressions: &[crate::suppress::Suppression],
) -> Vec<CachedSuppression> {
    suppressions
        .iter()
        .map(|suppression| {
            let (kind, policy_pack, policy_rule_id) = match &suppression.target {
                None => (0, String::new(), String::new()),
                Some(SuppressionTarget::Issue(kind)) => {
                    (kind.to_discriminant(), String::new(), String::new())
                }
                Some(SuppressionTarget::PolicyRule(target)) => (
                    crate::suppress::IssueKind::PolicyViolation.to_discriminant(),
                    target.pack.clone(),
                    target.rule_id.clone(),
                ),
            };
            CachedSuppression {
                line: suppression.line,
                comment_line: suppression.comment_line,
                kind,
                policy_pack,
                policy_rule_id,
            }
        })
        .collect()
}

fn module_unknown_suppressions_to_cached(
    unknown_suppression_kinds: &[fallow_types::suppress::UnknownSuppressionKind],
) -> Vec<CachedUnknownSuppressionKind> {
    unknown_suppression_kinds
        .iter()
        .map(|unknown| CachedUnknownSuppressionKind {
            comment_line: unknown.comment_line,
            is_file_level: unknown.is_file_level,
            token: unknown.token.clone(),
        })
        .collect()
}

fn module_local_types_to_cached(
    local_type_declarations: &[crate::LocalTypeDeclaration],
) -> Vec<CachedLocalTypeDeclaration> {
    local_type_declarations
        .iter()
        .map(|declaration| CachedLocalTypeDeclaration {
            name: declaration.name.clone(),
            span_start: declaration.span.start,
            span_end: declaration.span.end,
        })
        .collect()
}

fn module_signature_refs_to_cached(
    public_signature_type_references: &[crate::PublicSignatureTypeReference],
) -> Vec<CachedPublicSignatureTypeReference> {
    public_signature_type_references
        .iter()
        .map(|reference| CachedPublicSignatureTypeReference {
            export_name: reference.export_name.clone(),
            type_name: reference.type_name.clone(),
            span_start: reference.span.start,
            span_end: reference.span.end,
        })
        .collect()
}

fn module_namespace_aliases_to_cached(
    namespace_object_aliases: &[NamespaceObjectAlias],
) -> Vec<CachedNamespaceObjectAlias> {
    namespace_object_aliases
        .iter()
        .map(|alias| CachedNamespaceObjectAlias {
            via_export_name: alias.via_export_name.clone(),
            suffix: alias.suffix.clone(),
            namespace_local: alias.namespace_local.clone(),
        })
        .collect()
}

/// Reconstruct a [`ModuleInfo`](crate::ModuleInfo) from a [`CachedModule`], skipping
/// the per-function complexity vec when `need_complexity` is `false`. Avoids the
/// `Vec<FunctionComplexity>` clone on warm runs of commands (e.g. `fallow dead-code`)
/// that don't consume complexity, which adds up across tens of thousands of files.
#[must_use]
pub fn cached_to_module_opts(
    cached: &CachedModule,
    file_id: fallow_types::discover::FileId,
    need_complexity: bool,
) -> crate::ModuleInfo {
    crate::ModuleInfo {
        file_id,
        exports: cached_exports_to_module(&cached.exports),
        imports: cached_imports_to_module(&cached.imports),
        re_exports: cached_re_exports_to_module(&cached.re_exports),
        dynamic_imports: cached_dynamic_imports_to_module(&cached.dynamic_imports),
        dynamic_import_patterns: cached_dynamic_patterns_to_module(&cached.dynamic_import_patterns),
        require_calls: cached_require_calls_to_module(&cached.require_calls),
        package_path_references: cached.package_path_references.clone(),
        member_accesses: cached.member_accesses.clone(),
        whole_object_uses: cached.whole_object_uses.clone(),
        has_cjs_exports: cached.has_cjs_exports,
        has_angular_component_template_url: cached.has_angular_component_template_url,
        content_hash: cached.content_hash,
        suppressions: cached_suppressions_to_module(&cached.suppressions),
        unknown_suppression_kinds: cached_unknown_suppressions_to_module(
            &cached.unknown_suppression_kinds,
        ),
        unused_import_bindings: cached.unused_import_bindings.clone(),
        type_referenced_import_bindings: cached.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: cached.value_referenced_import_bindings.clone(),
        line_offsets: cached.line_offsets.clone(),
        complexity: if need_complexity {
            cached.complexity.clone()
        } else {
            Vec::new()
        },
        flag_uses: cached.flag_uses.clone(),
        class_heritage: cached.class_heritage.clone(),
        injection_tokens: cached.injection_tokens.clone(),
        local_type_declarations: cached_local_types_to_module(&cached.local_type_declarations),
        public_signature_type_references: cached_signature_refs_to_module(
            &cached.public_signature_type_references,
        ),
        namespace_object_aliases: cached_namespace_aliases_to_module(
            &cached.namespace_object_aliases,
        ),
        iconify_prefixes: cached.iconify_prefixes.clone(),
        iconify_icon_names: cached.iconify_icon_names.clone(),
        auto_import_candidates: cached.auto_import_candidates.clone(),
        directives: cached.directives.clone(),
        client_only_dynamic_import_spans: cached.client_only_dynamic_import_spans.clone(),
        security_sinks: cached.security_sinks.clone(),
        security_sinks_skipped: cached.security_sinks_skipped,
        security_unresolved_callee_sites: cached.security_unresolved_callee_sites.clone(),
        tainted_bindings: cached.tainted_bindings.clone(),
        sanitized_sink_args: cached.sanitized_sink_args.clone(),
        security_control_sites: cached.security_control_sites.clone(),
        callee_uses: cached.callee_uses.clone(),
        misplaced_directives: cached.misplaced_directives.clone(),
        di_key_sites: cached.di_key_sites.clone(),
        has_dynamic_provide: cached.has_dynamic_provide,
        // Derived in `release_resolution_payload` from `imports` + `unused_import_bindings`
        // (both cached); never persisted, so the cache-load path leaves it empty.
        referenced_import_bindings: Vec::new(),
        component_props: cached.component_props.clone(),
        has_props_attrs_fallthrough: cached.has_props_attrs_fallthrough,
        has_define_expose: cached.has_define_expose,
        has_define_model: cached.has_define_model,
        has_unharvestable_props: cached.has_unharvestable_props,
        component_emits: cached.component_emits.clone(),
        has_unharvestable_emits: cached.has_unharvestable_emits,
        has_dynamic_emit: cached.has_dynamic_emit,
        has_emit_whole_object_use: cached.has_emit_whole_object_use,
        load_return_keys: cached.load_return_keys.clone(),
        has_unharvestable_load: cached.has_unharvestable_load,
        has_load_data_whole_use: cached.has_load_data_whole_use,
        // Derived in `release_resolution_payload` from `whole_object_uses`.
        has_page_data_store_whole_use: false,
        component_functions: cached.component_functions.clone(),
        react_props: cached.react_props.clone(),
        hook_uses: cached.hook_uses.clone(),
        render_edges: cached.render_edges.clone(),
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
        last_access_secs: current_unix_seconds(),
        exports: module_exports_to_cached(&module.exports),
        imports: module_imports_to_cached(&module.imports),
        re_exports: module_re_exports_to_cached(&module.re_exports),
        dynamic_imports: module_dynamic_imports_to_cached(&module.dynamic_imports),
        require_calls: module_require_calls_to_cached(&module.require_calls),
        package_path_references: module.package_path_references.clone(),
        member_accesses: module.member_accesses.clone(),
        whole_object_uses: module.whole_object_uses.clone(),
        dynamic_import_patterns: module_dynamic_patterns_to_cached(&module.dynamic_import_patterns),
        has_cjs_exports: module.has_cjs_exports,
        has_angular_component_template_url: module.has_angular_component_template_url,
        unused_import_bindings: module.unused_import_bindings.clone(),
        type_referenced_import_bindings: module.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: module.value_referenced_import_bindings.clone(),
        suppressions: module_suppressions_to_cached(&module.suppressions),
        unknown_suppression_kinds: module_unknown_suppressions_to_cached(
            &module.unknown_suppression_kinds,
        ),
        line_offsets: module.line_offsets.clone(),
        complexity: module.complexity.clone(),
        flag_uses: module.flag_uses.clone(),
        class_heritage: module.class_heritage.clone(),
        injection_tokens: module.injection_tokens.clone(),
        local_type_declarations: module_local_types_to_cached(&module.local_type_declarations),
        public_signature_type_references: module_signature_refs_to_cached(
            &module.public_signature_type_references,
        ),
        namespace_object_aliases: module_namespace_aliases_to_cached(
            &module.namespace_object_aliases,
        ),
        iconify_prefixes: module.iconify_prefixes.clone(),
        iconify_icon_names: module.iconify_icon_names.clone(),
        auto_import_candidates: module.auto_import_candidates.clone(),
        directives: module.directives.clone(),
        client_only_dynamic_import_spans: module.client_only_dynamic_import_spans.clone(),
        security_sinks: module.security_sinks.clone(),
        security_sinks_skipped: module.security_sinks_skipped,
        security_unresolved_callee_sites: module.security_unresolved_callee_sites.clone(),
        tainted_bindings: module.tainted_bindings.clone(),
        sanitized_sink_args: module.sanitized_sink_args.clone(),
        security_control_sites: module.security_control_sites.clone(),
        callee_uses: module.callee_uses.clone(),
        misplaced_directives: module.misplaced_directives.clone(),
        di_key_sites: module.di_key_sites.clone(),
        has_dynamic_provide: module.has_dynamic_provide,
        component_props: module.component_props.clone(),
        has_props_attrs_fallthrough: module.has_props_attrs_fallthrough,
        has_define_expose: module.has_define_expose,
        has_define_model: module.has_define_model,
        has_unharvestable_props: module.has_unharvestable_props,
        component_emits: module.component_emits.clone(),
        has_unharvestable_emits: module.has_unharvestable_emits,
        has_dynamic_emit: module.has_dynamic_emit,
        has_emit_whole_object_use: module.has_emit_whole_object_use,
        load_return_keys: module.load_return_keys.clone(),
        has_unharvestable_load: module.has_unharvestable_load,
        has_load_data_whole_use: module.has_load_data_whole_use,
        component_functions: module.component_functions.clone(),
        react_props: module.react_props.clone(),
        hook_uses: module.hook_uses.clone(),
        render_edges: module.render_edges.clone(),
    }
}
