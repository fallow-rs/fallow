//! Tests for the incremental parse cache.

use std::path::Path;

use oxc_span::Span;

use crate::*;
use fallow_types::discover::FileId;
use fallow_types::extract::{SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason};

use super::*;

#[test]
fn cache_store_new_is_empty() {
    let store = CacheStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn cache_store_default_is_empty() {
    let store = CacheStore::default();
    assert!(store.is_empty());
}

#[test]
fn cache_roundtrip_preserves_unresolved_callee_diagnostics() {
    let module = parse_from_content(
        FileId(7),
        Path::new("src/security.ts"),
        "client[method](req.body.name);",
    );
    assert_eq!(module.security_sinks_skipped, 1);
    assert_eq!(module.security_unresolved_callee_sites.len(), 1);

    let cached = module_to_cached(&module, 10, 20);
    let restored = cached_to_module(&cached, FileId(7));
    let diagnostic = restored
        .security_unresolved_callee_sites
        .first()
        .expect("diagnostic round-tripped");

    assert_eq!(restored.security_sinks_skipped, 1);
    assert_eq!(
        diagnostic.reason,
        SkippedSecurityCalleeReason::ComputedMember
    );
    assert_eq!(
        diagnostic.expression_kind,
        SkippedSecurityCalleeExpressionKind::ComputedMemberExpression
    );
}

#[test]
fn cache_store_insert_and_get() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);
    assert_eq!(store.len(), 1);
    assert!(!store.is_empty());
    assert!(store.get(Path::new("test.ts"), 42).is_some());
}

#[test]
fn cache_store_hash_mismatch_returns_none() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);
    assert!(store.get(Path::new("test.ts"), 99).is_none());
}

#[test]
fn cache_store_missing_key_returns_none() {
    let store = CacheStore::new();
    assert!(store.get(Path::new("nonexistent.ts"), 42).is_none());
}

#[test]
fn cache_store_overwrite_entry() {
    let mut store = CacheStore::new();
    let m1 = CachedModule {
        content_hash: 1,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    let m2 = CachedModule {
        content_hash: 2,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), m1);
    store.insert(Path::new("test.ts"), m2);
    assert_eq!(store.len(), 1);
    assert!(store.get(Path::new("test.ts"), 1).is_none());
    assert!(store.get(Path::new("test.ts"), 2).is_some());
}

#[test]
fn module_to_cached_roundtrip_named_export() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("foo".to_string()),
            local_name: Some("foo".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            span: Span::new(10, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 123,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: vec!["jam".to_string(), "ic".to_string()],
        iconify_icon_names: vec!["simple-icons-github".to_string()],
        auto_import_candidates: vec!["Card001".to_string(), "BaseButton".to_string()],
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(
        restored.iconify_prefixes,
        vec!["jam".to_string(), "ic".to_string()]
    );
    assert_eq!(
        restored.iconify_icon_names,
        vec!["simple-icons-github".to_string()]
    );
    assert_eq!(
        restored.auto_import_candidates,
        vec!["Card001".to_string(), "BaseButton".to_string()]
    );
    assert_eq!(restored.exports.len(), 1);
    assert_eq!(
        restored.exports[0].name,
        ExportName::Named("foo".to_string())
    );
    assert!(!restored.exports[0].is_type_only);
    assert_eq!(restored.exports[0].span.start, 10);
    assert_eq!(restored.exports[0].span.end, 20);
    assert_eq!(restored.content_hash, 123);
}

#[test]
fn module_to_cached_roundtrip_side_effect_used_export() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("MyElement".to_string()),
            local_name: Some("MyElement".to_string()),
            is_type_only: false,
            is_side_effect_used: true,
            visibility: VisibilityTag::None,
            span: Span::new(10, 20),
            members: vec![],
            super_class: Some("HTMLElement".to_string()),
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 789,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.exports.len(), 1);
    assert!(restored.exports[0].is_side_effect_used);
    assert_eq!(
        restored.exports[0].super_class.as_deref(),
        Some("HTMLElement")
    );
}

#[test]
fn module_to_cached_roundtrip_default_export() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Default,
            local_name: None,
            is_type_only: false,
            visibility: VisibilityTag::None,
            span: Span::new(0, 10),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 456,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.exports[0].name, ExportName::Default);
}

#[test]
fn module_to_cached_roundtrip_imports() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![
            ImportInfo {
                source: "./utils".to_string(),
                imported_name: ImportedName::Named("foo".to_string()),
                local_name: "foo".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 10),
                source_span: Span::new(5, 10),
            },
            ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Default,
                local_name: "React".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(15, 30),
                source_span: Span::new(20, 30),
            },
            ImportInfo {
                source: "./all".to_string(),
                imported_name: ImportedName::Namespace,
                local_name: "all".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(35, 50),
                source_span: Span::new(40, 50),
            },
            ImportInfo {
                source: "./styles.css".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: Span::new(55, 70),
                source_span: Span::new(60, 70),
            },
        ],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 789,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.imports.len(), 4);
    assert_eq!(
        restored.imports[0].imported_name,
        ImportedName::Named("foo".to_string())
    );
    assert_eq!(restored.imports[0].span.start, 0);
    assert_eq!(restored.imports[0].span.end, 10);
    assert_eq!(restored.imports[1].imported_name, ImportedName::Default);
    assert_eq!(restored.imports[1].span.start, 15);
    assert_eq!(restored.imports[1].span.end, 30);
    assert_eq!(restored.imports[2].imported_name, ImportedName::Namespace);
    assert_eq!(restored.imports[2].span.start, 35);
    assert_eq!(restored.imports[2].span.end, 50);
    assert_eq!(restored.imports[3].imported_name, ImportedName::SideEffect);
    assert_eq!(restored.imports[3].span.start, 55);
    assert_eq!(restored.imports[3].span.end, 70);
}

#[test]
fn module_to_cached_roundtrip_re_exports() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![ReExportInfo {
            source: "./module".to_string(),
            imported_name: "foo".to_string(),
            exported_name: "bar".to_string(),
            is_type_only: true,
            span: oxc_span::Span::default(),
        }],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.re_exports.len(), 1);
    assert_eq!(restored.re_exports[0].source, "./module");
    assert_eq!(restored.re_exports[0].imported_name, "foo");
    assert_eq!(restored.re_exports[0].exported_name, "bar");
    assert!(restored.re_exports[0].is_type_only);
}

#[test]
fn module_to_cached_roundtrip_dynamic_imports() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![DynamicImportInfo {
            source: "./lazy".to_string(),
            span: Span::new(0, 10),
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
        }],
        require_calls: vec![RequireCallInfo {
            source: "fs".to_string(),
            span: Span::new(15, 25),
            destructured_names: Vec::new(),
            local_name: None,
            source_span: oxc_span::Span::default(),
        }],
        package_path_references: vec![],
        member_accesses: vec![MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        }],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: true,
        has_angular_component_template_url: false,
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.dynamic_imports.len(), 1);
    assert_eq!(restored.dynamic_imports[0].source, "./lazy");
    assert_eq!(restored.dynamic_imports[0].span.start, 0);
    assert_eq!(restored.dynamic_imports[0].span.end, 10);
    assert_eq!(restored.require_calls.len(), 1);
    assert_eq!(restored.require_calls[0].source, "fs");
    assert_eq!(restored.require_calls[0].span.start, 15);
    assert_eq!(restored.require_calls[0].span.end, 25);
    assert_eq!(restored.member_accesses.len(), 1);
    assert_eq!(restored.member_accesses[0].object, "Status");
    assert_eq!(restored.member_accesses[0].member, "Active");
    assert!(restored.has_cjs_exports);
}

#[test]
fn module_to_cached_roundtrip_members() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("Color".to_string()),
            local_name: Some("Color".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            span: Span::new(0, 50),
            members: vec![
                MemberInfo {
                    name: "Red".to_string(),
                    kind: MemberKind::EnumMember,
                    span: Span::new(10, 15),
                    has_decorator: false,
                    decorator_names: Vec::new(),
                    is_instance_returning_static: false,
                    is_self_returning: false,
                },
                MemberInfo {
                    name: "greet".to_string(),
                    kind: MemberKind::ClassMethod,
                    span: Span::new(20, 30),
                    has_decorator: false,
                    decorator_names: Vec::new(),
                    is_instance_returning_static: false,
                    is_self_returning: false,
                },
                MemberInfo {
                    name: "name".to_string(),
                    kind: MemberKind::ClassProperty,
                    span: Span::new(35, 45),
                    has_decorator: false,
                    decorator_names: Vec::new(),
                    is_instance_returning_static: false,
                    is_self_returning: false,
                },
            ],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.exports[0].members.len(), 3);
    assert_eq!(restored.exports[0].members[0].kind, MemberKind::EnumMember);
    assert_eq!(restored.exports[0].members[1].kind, MemberKind::ClassMethod);
    assert_eq!(
        restored.exports[0].members[2].kind,
        MemberKind::ClassProperty
    );
}

#[test]
fn cache_load_nonexistent_returns_none() {
    let result = CacheStore::load(Path::new("/nonexistent/path"), 0, DEFAULT_CACHE_MAX_SIZE);
    assert!(result.is_none());
}

/// Create a unique temporary directory for cache tests.
fn test_cache_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("fallow_cache_tests")
        .join(name)
        .join(format!("{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cache_save_and_load_roundtrip() {
    let dir = test_cache_dir("roundtrip");
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);
    store.save(&dir, 0, DEFAULT_CACHE_MAX_SIZE).unwrap();

    let loaded = CacheStore::load(&dir, 0, DEFAULT_CACHE_MAX_SIZE);
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded.get(Path::new("test.ts"), 42).is_some());
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "*\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_version_mismatch_returns_none() {
    let dir = test_cache_dir("version_mismatch");
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);
    store.save(&dir, 0, DEFAULT_CACHE_MAX_SIZE).unwrap();

    assert!(CacheStore::load(&dir, 0, DEFAULT_CACHE_MAX_SIZE).is_some());

    let cache_file = dir.join("cache.bin");
    let mut data = std::fs::read(&cache_file).unwrap();
    assert!(!data.is_empty());
    data[0] = 255; // Corrupt the version byte
    std::fs::write(&cache_file, &data).unwrap();

    let result = CacheStore::load(&dir, 0, DEFAULT_CACHE_MAX_SIZE);
    assert!(result.is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn module_to_cached_roundtrip_type_only_import() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![ImportInfo {
            source: "./types".to_string(),
            imported_name: ImportedName::Named("Foo".to_string()),
            local_name: "Foo".to_string(),
            is_type_only: true,
            from_style: false,
            span: Span::new(0, 10),
            source_span: Span::new(5, 10),
        }],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert!(restored.imports[0].is_type_only);
    assert_eq!(restored.imports[0].span.start, 0);
    assert_eq!(restored.imports[0].span.end, 10);
}

#[test]
fn get_by_path_only_returns_entry_regardless_of_hash() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);

    let result = store.get_by_path_only(Path::new("test.ts"));
    assert!(result.is_some());
    assert_eq!(result.unwrap().content_hash, 42);
}

#[test]
fn get_by_path_only_returns_none_for_missing() {
    let store = CacheStore::new();
    assert!(
        store
            .get_by_path_only(Path::new("nonexistent.ts"))
            .is_none()
    );
}

#[test]
fn retain_paths_removes_stale_entries() {
    use fallow_types::discover::DiscoveredFile;
    use std::path::PathBuf;

    let mut store = CacheStore::new();
    let m = || CachedModule {
        content_hash: 1,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };

    store.insert(Path::new("/project/a.ts"), m());
    store.insert(Path::new("/project/b.ts"), m());
    store.insert(Path::new("/project/c.ts"), m());
    assert_eq!(store.len(), 3);

    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/a.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/c.ts"),
            size_bytes: 50,
        },
    ];

    store.retain_paths(&files);
    assert_eq!(store.len(), 2);
    assert!(store.get_by_path_only(Path::new("/project/a.ts")).is_some());
    assert!(store.get_by_path_only(Path::new("/project/b.ts")).is_none());
    assert!(store.get_by_path_only(Path::new("/project/c.ts")).is_some());
}

#[test]
fn retain_paths_with_empty_files_clears_cache() {
    let mut store = CacheStore::new();
    let m = CachedModule {
        content_hash: 1,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("a.ts"), m);
    assert_eq!(store.len(), 1);

    store.retain_paths(&[]);
    assert!(store.is_empty());
}

#[test]
fn get_by_metadata_returns_entry_on_match() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 1000,
        file_size: 500,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);

    let result = store.get_by_metadata(Path::new("test.ts"), 1000, 500);
    assert!(result.is_some());
    assert_eq!(result.unwrap().content_hash, 42);
}

#[test]
fn get_by_metadata_returns_none_on_mtime_mismatch() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 1000,
        file_size: 500,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);

    assert!(
        store
            .get_by_metadata(Path::new("test.ts"), 2000, 500)
            .is_none()
    );
}

#[test]
fn get_by_metadata_returns_none_on_size_mismatch() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 1000,
        file_size: 500,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);

    assert!(
        store
            .get_by_metadata(Path::new("test.ts"), 1000, 999)
            .is_none()
    );
}

#[test]
fn get_by_metadata_returns_none_for_zero_mtime() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 500,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    };
    store.insert(Path::new("test.ts"), module);

    assert!(
        store
            .get_by_metadata(Path::new("test.ts"), 0, 500)
            .is_none()
    );
}

#[test]
fn get_by_metadata_returns_none_for_missing_file() {
    let store = CacheStore::new();
    assert!(
        store
            .get_by_metadata(Path::new("nonexistent.ts"), 1000, 500)
            .is_none()
    );
}

#[test]
fn module_to_cached_stores_mtime_and_size() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 42,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 12345, 6789);
    assert_eq!(cached.mtime_secs, 12345);
    assert_eq!(cached.file_size, 6789);
    assert_eq!(cached.content_hash, 42);
}

#[test]
fn module_to_cached_roundtrip_line_offsets() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![0, 15, 30, 45],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };
    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));
    assert_eq!(restored.line_offsets, vec![0, 15, 30, 45]);
}

#[test]
fn module_to_cached_roundtrip_suppressions_with_kinds() {
    use crate::suppress::{IssueKind, Suppression};

    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![
            Suppression::all(0, 1),
            Suppression::issue(5, 4, IssueKind::UnusedExport),
            Suppression::issue(10, 9, IssueKind::UnusedFile),
            Suppression::policy_rule(12, 11, "team-policy", "no-child-process"),
        ],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.suppressions.len(), 4);
    assert_eq!(restored.suppressions[0].line, 0);
    assert!(restored.suppressions[0].issue_kind_target().is_none());
    assert_eq!(restored.suppressions[1].line, 5);
    assert_eq!(
        restored.suppressions[1].issue_kind_target(),
        Some(IssueKind::UnusedExport)
    );
    assert_eq!(restored.suppressions[2].line, 10);
    assert_eq!(
        restored.suppressions[2].issue_kind_target(),
        Some(IssueKind::UnusedFile)
    );
    assert_eq!(restored.suppressions[3].line, 12);
    let policy = restored.suppressions[3]
        .policy_rule_target()
        .expect("scoped policy target should round-trip");
    assert_eq!(policy.pack, "team-policy");
    assert_eq!(policy.rule_id, "no-child-process");
}

#[test]
fn module_to_cached_roundtrip_unknown_suppression_kinds() {
    use fallow_types::suppress::UnknownSuppressionKind;

    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![
            UnknownSuppressionKind {
                comment_line: 3,
                is_file_level: false,
                token: "complexity-typo".to_string(),
            },
            UnknownSuppressionKind {
                comment_line: 7,
                is_file_level: true,
                token: "unsed-export".to_string(),
            },
        ],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.unknown_suppression_kinds.len(), 2);
    assert_eq!(restored.unknown_suppression_kinds[0].comment_line, 3);
    assert!(!restored.unknown_suppression_kinds[0].is_file_level);
    assert_eq!(
        restored.unknown_suppression_kinds[0].token,
        "complexity-typo"
    );
    assert_eq!(restored.unknown_suppression_kinds[1].comment_line, 7);
    assert!(restored.unknown_suppression_kinds[1].is_file_level);
    assert_eq!(restored.unknown_suppression_kinds[1].token, "unsed-export");
}

#[test]
fn module_to_cached_roundtrip_visibility() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("publicFoo".to_string()),
            local_name: Some("publicFoo".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::Public,
            span: Span::new(0, 10),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn module_to_cached_roundtrip_visibility_internal() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("internalHelper".to_string()),
            local_name: Some("internalHelper".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::Internal,
            span: Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    assert_eq!(cached.exports[0].visibility, 2);
    let restored = cached_to_module(&cached, FileId(0));
    assert_eq!(restored.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn module_to_cached_roundtrip_visibility_beta() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("betaFeature".to_string()),
            local_name: Some("betaFeature".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::Beta,
            span: Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    assert_eq!(cached.exports[0].visibility, 3);
    let restored = cached_to_module(&cached, FileId(0));
    assert_eq!(restored.exports[0].visibility, VisibilityTag::Beta);
}

#[test]
fn module_to_cached_roundtrip_visibility_alpha() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("alphaFeature".to_string()),
            local_name: Some("alphaFeature".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::Alpha,
            span: Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    assert_eq!(cached.exports[0].visibility, 4);
    let restored = cached_to_module(&cached, FileId(0));
    assert_eq!(restored.exports[0].visibility, VisibilityTag::Alpha);
}

#[test]
fn module_to_cached_roundtrip_dynamic_import_patterns() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![
            crate::DynamicImportPattern {
                prefix: "./components/".to_string(),
                suffix: Some(".vue".to_string()),
                span: Span::new(0, 50),
            },
            crate::DynamicImportPattern {
                prefix: "./pages/**/".to_string(),
                suffix: None,
                span: Span::new(60, 100),
            },
        ],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.dynamic_import_patterns.len(), 2);
    assert_eq!(restored.dynamic_import_patterns[0].prefix, "./components/");
    assert_eq!(
        restored.dynamic_import_patterns[0].suffix,
        Some(".vue".to_string())
    );
    assert_eq!(restored.dynamic_import_patterns[0].span.start, 0);
    assert_eq!(restored.dynamic_import_patterns[0].span.end, 50);
    assert_eq!(restored.dynamic_import_patterns[1].prefix, "./pages/**/");
    assert!(restored.dynamic_import_patterns[1].suffix.is_none());
}

#[test]
fn module_to_cached_roundtrip_unused_import_bindings() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec!["Status".to_string()],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec!["unusedFoo".to_string(), "unusedBar".to_string()],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.unused_import_bindings.len(), 2);
    assert!(
        restored
            .unused_import_bindings
            .contains(&"unusedFoo".to_string())
    );
    assert!(
        restored
            .unused_import_bindings
            .contains(&"unusedBar".to_string())
    );
    assert!(restored.whole_object_uses.contains(&"Status".to_string()));
}

#[test]
fn module_to_cached_roundtrip_complexity() {
    use fallow_types::extract::FunctionComplexity;

    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![
            FunctionComplexity {
                name: "complex".to_string(),
                line: 5,
                col: 0,
                cyclomatic: 8,
                cognitive: 15,
                line_count: 20,
                param_count: 4,
                source_hash: Some("0123456789abcdef".to_string()),
                contributions: Vec::new(),
            },
            FunctionComplexity {
                name: "simple".to_string(),
                line: 30,
                col: 4,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 3,
                param_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
        ],
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.complexity.len(), 2);
    assert_eq!(restored.complexity[0].name, "complex");
    assert_eq!(restored.complexity[0].cyclomatic, 8);
    assert_eq!(restored.complexity[0].cognitive, 15);
    assert_eq!(restored.complexity[0].line_count, 20);
    assert_eq!(restored.complexity[1].name, "simple");
    assert_eq!(restored.complexity[1].cyclomatic, 1);
    assert_eq!(restored.complexity[1].cognitive, 0);
}

#[test]
fn module_to_cached_roundtrip_require_with_destructured() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![RequireCallInfo {
            source: "fs".to_string(),
            span: Span::new(0, 30),
            destructured_names: vec!["readFile".to_string(), "writeFile".to_string()],
            local_name: None,
            source_span: oxc_span::Span::default(),
        }],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.require_calls.len(), 1);
    assert_eq!(restored.require_calls[0].source, "fs");
    assert!(restored.require_calls[0].local_name.is_none());
    assert_eq!(
        restored.require_calls[0].destructured_names,
        vec!["readFile", "writeFile"]
    );
}

#[test]
fn module_to_cached_roundtrip_dynamic_import_with_local() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![DynamicImportInfo {
            source: "./mod".to_string(),
            span: Span::new(0, 20),
            destructured_names: vec![],
            local_name: Some("mod".to_string()),
            is_speculative: false,
        }],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(
        restored.dynamic_imports[0].local_name,
        Some("mod".to_string())
    );
}

#[test]
fn module_to_cached_roundtrip_source_span() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![],
        imports: vec![ImportInfo {
            source: "./utils".to_string(),
            imported_name: ImportedName::Named("foo".to_string()),
            local_name: "foo".to_string(),
            is_type_only: false,
            from_style: false,
            span: Span::new(0, 30),
            source_span: Span::new(25, 33),
        }],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.imports[0].source_span.start, 25);
    assert_eq!(restored.imports[0].source_span.end, 33);
}

#[test]
fn module_to_cached_roundtrip_member_decorators() {
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![ExportInfo {
            name: ExportName::Named("Svc".to_string()),
            local_name: Some("Svc".to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            span: Span::new(0, 100),
            members: vec![MemberInfo {
                name: "handler".to_string(),
                kind: MemberKind::ClassMethod,
                span: Span::new(50, 80),
                has_decorator: true,
                decorator_names: vec!["Inject".to_string()],
                is_instance_returning_static: false,
                is_self_returning: false,
            }],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: vec![],
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert!(restored.exports[0].members[0].has_decorator);
    assert_eq!(restored.exports[0].members[0].span.start, 50);
    assert_eq!(restored.exports[0].members[0].span.end, 80);
}

/// Build a minimal `CachedModule` whose serialized size is roughly
/// proportional to `payload_kb`. Used by the eviction tests to populate
/// a store with predictable byte cost.
fn synthetic_module(content_hash: u64, last_access_secs: u64, payload_kb: usize) -> CachedModule {
    let payload = "a".repeat(payload_kb * 1024);
    CachedModule {
        content_hash,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![payload],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
    }
}

#[test]
fn cache_save_no_eviction_when_under_threshold() {
    let dir = test_cache_dir("no_evict");
    let mut store = CacheStore::new();
    for i in 0..10u64 {
        store.insert(Path::new(&format!("file{i}.ts")), synthetic_module(i, i, 1));
    }
    store
        .save(&dir, 42, DEFAULT_CACHE_MAX_SIZE)
        .expect("save under threshold");

    let loaded = CacheStore::load(&dir, 42, DEFAULT_CACHE_MAX_SIZE).expect("load round-trip");
    assert_eq!(loaded.len(), 10, "all 10 entries survive");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_save_evicts_when_over_threshold() {
    let dir = test_cache_dir("evict_over_threshold");
    let mut store = CacheStore::new();
    for i in 0..50u64 {
        store.insert(Path::new(&format!("file{i}.ts")), synthetic_module(i, i, 1));
    }
    let cap = 40 * 1024;
    store.save(&dir, 99, cap).expect("save with eviction");

    let loaded = CacheStore::load(&dir, 99, DEFAULT_CACHE_MAX_SIZE).expect("load after eviction");
    assert!(loaded.len() < 50, "eviction removed entries");
    let kept_max = (0..50u64)
        .filter(|i| {
            loaded
                .get(&std::path::PathBuf::from(format!("file{i}.ts")), *i)
                .is_some()
        })
        .max()
        .expect("at least one entry remains");
    let kept_min = (0..50u64)
        .filter(|i| {
            loaded
                .get(&std::path::PathBuf::from(format!("file{i}.ts")), *i)
                .is_some()
        })
        .min()
        .expect("at least one entry remains");
    assert_eq!(kept_max, 49, "newest entry survives eviction");
    assert!(
        kept_min > 0,
        "oldest entry was evicted (kept_min = {kept_min})"
    );

    let on_disk = std::fs::read(dir.join("cache.bin")).unwrap();
    assert!(
        on_disk.len() < cap * 80 / 100,
        "post-eviction size ({on_disk_kb} KB) < 80% of cap ({cap_kb} KB)",
        on_disk_kb = on_disk.len() / 1024,
        cap_kb = cap / 1024,
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_save_always_honors_cap_with_huge_entries() {
    let dir = test_cache_dir("huge_entry_overshoot");
    let mut store = CacheStore::new();
    store.insert(Path::new("huge.ts"), synthetic_module(7, 0, 10));
    store
        .save(&dir, 5, 1024)
        .expect("save honors cap with overshoot");

    let loaded =
        CacheStore::load(&dir, 5, DEFAULT_CACHE_MAX_SIZE).expect("load after overshoot save");
    assert_eq!(loaded.len(), 1, "single entry preserved under overshoot");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_load_returns_none_on_config_hash_mismatch() {
    let dir = test_cache_dir("config_hash_mismatch");
    let mut store = CacheStore::new();
    store.insert(Path::new("test.ts"), synthetic_module(1, 0, 1));
    store
        .save(&dir, 0xDEAD_BEEF, DEFAULT_CACHE_MAX_SIZE)
        .expect("save with config_hash A");

    assert!(
        CacheStore::load(&dir, 0xCAFE_BABE, DEFAULT_CACHE_MAX_SIZE).is_none(),
        "mismatched config_hash invalidates cache"
    );
    assert!(
        CacheStore::load(&dir, 0xDEAD_BEEF, DEFAULT_CACHE_MAX_SIZE).is_some(),
        "matching config_hash round-trips"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_load_round_trip_with_matching_config_hash() {
    let dir = test_cache_dir("config_hash_roundtrip");
    let mut store = CacheStore::new();
    store.insert(Path::new("a.ts"), synthetic_module(1, 0, 1));
    store.insert(Path::new("b.ts"), synthetic_module(2, 1, 1));
    let hash = 0xABCD_1234_5678_9ABC_u64;
    store
        .save(&dir, hash, DEFAULT_CACHE_MAX_SIZE)
        .expect("save with hash");

    let loaded =
        CacheStore::load(&dir, hash, DEFAULT_CACHE_MAX_SIZE).expect("load with matching hash");
    assert_eq!(loaded.len(), 2);
    assert!(loaded.get(Path::new("a.ts"), 1).is_some());
    assert!(loaded.get(Path::new("b.ts"), 2).is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_load_honors_user_max_size_above_default() {
    let dir = test_cache_dir("load_honors_user_max");
    let mut store = CacheStore::new();
    store.insert(Path::new("a.ts"), synthetic_module(1, 0, 1));
    store
        .save(&dir, 0, DEFAULT_CACHE_MAX_SIZE)
        .expect("save under default");

    assert!(
        CacheStore::load(&dir, 0, 1).is_some(),
        "tiny user cap does not discard valid existing cache"
    );

    assert!(
        CacheStore::load(&dir, 0, DEFAULT_CACHE_MAX_SIZE * 2).is_some(),
        "user cap above default round-trips"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_load_returns_none_on_bitcode_decode_failure() {
    let dir = test_cache_dir("decode_fail_info");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cache.bin"), b"not-a-valid-bitcode-payload").unwrap();
    assert!(CacheStore::load(&dir, 0, DEFAULT_CACHE_MAX_SIZE).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_save_atomic_write_leaves_no_tmp_on_success() {
    let dir = test_cache_dir("atomic_no_tmp");
    let mut store = CacheStore::new();
    store.insert(Path::new("test.ts"), synthetic_module(1, 0, 1));
    store
        .save(&dir, 0, DEFAULT_CACHE_MAX_SIZE)
        .expect("save atomic");

    assert!(dir.join("cache.bin").exists(), "cache.bin present");
    assert!(
        !dir.join("cache.bin.tmp").exists(),
        "tmp file cleaned up after rename"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
