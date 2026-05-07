use super::helpers::*;

// ---- find_unresolved_imports tests ----

#[test]
fn unresolved_import_detected() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./missing-file".to_string(),
                imported_name: ImportedName::Named("foo".to_string()),
                local_name: "foo".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 30),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./missing-file".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].specifier, "./missing-file");
}

#[test]
fn unresolved_dynamic_import_detected_with_real_location() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![],
        resolved_dynamic_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./missing-dynamic".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(14, 41),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./missing-dynamic".to_string()),
        }],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let offsets = vec![0, 12];
    let mut line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].specifier, "./missing-dynamic");
    assert_eq!(unresolved[0].line, 2);
    assert_eq!(unresolved[0].col, 2);
}

#[test]
fn unresolved_virtual_module_not_reported() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "virtual:generated-pages".to_string(),
                imported_name: ImportedName::Named("pages".to_string()),
                local_name: "pages".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 40),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("virtual:generated-pages".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "virtual: module imports should not be flagged as unresolved"
    );
}

#[test]
fn unresolved_import_with_virtual_prefix_not_reported() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "#imports".to_string(),
                imported_name: ImportedName::Named("useRouter".to_string()),
                local_name: "useRouter".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 25),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("#imports".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &["#"], // Nuxt-style virtual prefix
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "imports matching virtual_prefixes should not be flagged as unresolved"
    );
}

#[test]
fn unresolved_import_suppressed_by_generated_import_pattern() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/routes/+page.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "./$types".to_string(),
                    imported_name: ImportedName::Named("PageLoad".to_string()),
                    local_name: "PageLoad".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: oxc_span::Span::new(0, 40),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::Unresolvable("./$types".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./$types.js".to_string(),
                    imported_name: ImportedName::Named("PageLoad".to_string()),
                    local_name: "PageLoad".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: oxc_span::Span::new(50, 90),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::Unresolvable("./$types.js".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./$types.ts".to_string(),
                    imported_name: ImportedName::Named("PageLoad".to_string()),
                    local_name: "PageLoad".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: oxc_span::Span::new(100, 140),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::Unresolvable("./$types.ts".to_string()),
            },
        ],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &["/$types"], // SvelteKit-style generated import
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "imports matching generated_import_patterns should not be flagged as unresolved, found: {unresolved:?}"
    );
}

#[test]
fn unresolved_import_suppressed_by_inline_comment() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./broken".to_string(),
                imported_name: ImportedName::Named("thing".to_string()),
                local_name: "thing".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./broken".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    // Suppress unresolved imports on line 1 (byte offset 0 => line 1 without offsets)
    let supps = vec![Suppression {
        line: 1,
        comment_line: 0,
        kind: Some(suppress::IssueKind::UnresolvedImport),
    }];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(0), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "suppressed unresolved import should not be reported"
    );
}

#[test]
fn unresolved_dynamic_import_suppressed_by_inline_comment() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![],
        resolved_dynamic_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./broken-dynamic".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(14, 40),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./broken-dynamic".to_string()),
        }],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let supps = vec![Suppression {
        line: 2,
        comment_line: 1,
        kind: Some(suppress::IssueKind::UnresolvedImport),
    }];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(0), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);
    let offsets = vec![0, 12];
    let mut line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "suppressed dynamic unresolved import should not be reported"
    );
}

#[test]
fn unresolved_import_file_level_suppression() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./nonexistent".to_string(),
                imported_name: ImportedName::Named("x".to_string()),
                local_name: "x".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 25),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./nonexistent".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    // File-level suppression (line 0)
    let supps = vec![Suppression {
        line: 0,
        comment_line: 1,
        kind: Some(suppress::IssueKind::UnresolvedImport),
    }];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(0), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "file-level suppression should suppress all unresolved imports in the file"
    );
}

#[test]
fn resolved_import_not_reported_as_unresolved() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "react".to_string(),
                    imported_name: ImportedName::Named("useState".to_string()),
                    local_name: "useState".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("react".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./utils".to_string(),
                    imported_name: ImportedName::Named("helper".to_string()),
                    local_name: "helper".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(25, 50),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            },
        ],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "resolved imports should never appear as unresolved"
    );
}

// ---- Additional coverage: find_unresolved_imports with empty input ----

#[test]
fn no_resolved_modules_produces_no_unresolved() {
    let resolved_modules: Vec<ResolvedModule> = vec![];
    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert!(
        unresolved.is_empty(),
        "empty resolved_modules should produce no unresolved imports"
    );
}

// ---- Additional coverage: find_unresolved_imports suppression does not suppress wrong kind ----

#[test]
fn unresolved_import_not_suppressed_by_wrong_kind() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "./broken".to_string(),
                imported_name: ImportedName::Named("thing".to_string()),
                local_name: "thing".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::Unresolvable("./broken".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    }];

    let config = test_config(PathBuf::from("/project"));
    // Suppress a DIFFERENT issue kind on line 1 -- should NOT suppress unresolved import
    let supps = vec![Suppression {
        line: 1,
        comment_line: 0,
        kind: Some(suppress::IssueKind::UnusedExport),
    }];
    let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
    supp_map.insert(FileId(0), &supps);
    let suppressions = SuppressionContext::from_map(supp_map);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &line_offsets,
    );

    assert_eq!(
        unresolved.len(),
        1,
        "suppression with wrong issue kind should not suppress unresolved import"
    );
}
