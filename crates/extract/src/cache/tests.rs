//! Tests for the incremental parse cache.

use std::path::Path;

use oxc_span::Span;

use crate::*;
use fallow_types::discover::FileId;

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
fn cache_store_insert_and_get() {
    let mut store = CacheStore::new();
    let module = CachedModule {
        content_hash: 42,
        mtime_secs: 0,
        file_size: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    let m2 = CachedModule {
        content_hash: 2,
        mtime_secs: 0,
        file_size: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 123,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 789,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 456,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 789,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        }],
        require_calls: vec![RequireCallInfo {
            source: "fs".to_string(),
            span: Span::new(15, 25),
            destructured_names: Vec::new(),
            local_name: None,
        }],
        member_accesses: vec![MemberAccess {
            object: "Status".to_string(),
            member: "Active".to_string(),
        }],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: true,
        content_hash: 0,
        suppressions: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
                },
                MemberInfo {
                    name: "greet".to_string(),
                    kind: MemberKind::ClassMethod,
                    span: Span::new(20, 30),
                    has_decorator: false,
                },
                MemberInfo {
                    name: "name".to_string(),
                    kind: MemberKind::ClassProperty,
                    span: Span::new(35, 45),
                    has_decorator: false,
                },
            ],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
    let result = CacheStore::load(Path::new("/nonexistent/path"));
    assert!(result.is_none());
}

/// Create a unique temporary directory for cache tests.
fn test_cache_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("fallow_cache_tests")
        .join(name)
        .join(format!("{}", std::process::id()));
    // Clean up any leftover from previous runs
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    store.insert(Path::new("test.ts"), module);
    store.save(&dir).unwrap();

    let loaded = CacheStore::load(&dir);
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    store.insert(Path::new("test.ts"), module);
    store.save(&dir).unwrap();

    // Verify the cache loads correctly before tampering
    assert!(CacheStore::load(&dir).is_some());

    // Read raw bytes and modify the version field.
    // The version (CACHE_VERSION) is the first encoded field.
    // Replace the first byte with a different version value (e.g., 255)
    // to simulate a version mismatch.
    let cache_file = dir.join("cache.bin");
    let mut data = std::fs::read(&cache_file).unwrap();
    assert!(!data.is_empty());
    data[0] = 255; // Corrupt the version byte
    std::fs::write(&cache_file, &data).unwrap();

    // Loading should return None due to version mismatch
    let result = CacheStore::load(&dir);
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    store.insert(Path::new("test.ts"), module);

    // get_by_path_only should return the entry without checking hash
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };

    store.insert(Path::new("/project/a.ts"), m());
    store.insert(Path::new("/project/b.ts"), m());
    store.insert(Path::new("/project/c.ts"), m());
    assert_eq!(store.len(), 3);

    // Only a.ts and c.ts still exist in the project
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    store.insert(Path::new("test.ts"), module);

    // Zero mtime should never match (falls through to content hash check)
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 42,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![0, 15, 30, 45],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };
    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));
    assert_eq!(restored.line_offsets, vec![0, 15, 30, 45]);
}

// ── Additional coverage ─────────────────────────────────────

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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![
            Suppression {
                line: 0,
                comment_line: 1,
                kind: None,
            },
            Suppression {
                line: 5,
                comment_line: 4,
                kind: Some(IssueKind::UnusedExport),
            },
            Suppression {
                line: 10,
                comment_line: 9,
                kind: Some(IssueKind::UnusedFile),
            },
        ],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert_eq!(restored.suppressions.len(), 3);
    assert_eq!(restored.suppressions[0].line, 0);
    assert!(restored.suppressions[0].kind.is_none());
    assert_eq!(restored.suppressions[1].line, 5);
    assert_eq!(restored.suppressions[1].kind, Some(IssueKind::UnusedExport));
    assert_eq!(restored.suppressions[2].line, 10);
    assert_eq!(restored.suppressions[2].kind, Some(IssueKind::UnusedFile));
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec!["Status".to_string()],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec!["unusedFoo".to_string(), "unusedBar".to_string()],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
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
            },
            FunctionComplexity {
                name: "simple".to_string(),
                line: 30,
                col: 4,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 3,
                param_count: 0,
            },
        ],
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        }],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        }],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
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
            }],
            is_side_effect_used: false,
            super_class: None,
        }],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        content_hash: 0,
        suppressions: vec![],
        line_offsets: vec![],
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };

    let cached = module_to_cached(&module, 0, 0);
    let restored = cached_to_module(&cached, FileId(0));

    assert!(restored.exports[0].members[0].has_decorator);
    assert_eq!(restored.exports[0].members[0].span.start, 50);
    assert_eq!(restored.exports[0].members[0].span.end, 80);
}
