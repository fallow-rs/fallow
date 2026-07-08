use super::helpers::*;

/// A dependency imported only from a root-level test file should be flagged.
#[test]
fn test_only_dep_from_root_test_file() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/app.test.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/app.test.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/app.test.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("describe".to_string()),
                local_name: "describe".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["vitest"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.iter().any(|d| d.package_name == "vitest"),
        "dep imported only from test files should be flagged as test-only"
    );
}

/// A dependency imported only from a root-level config file should be flagged.
#[test]
fn test_only_dep_from_root_config_file() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/vitest.config.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/vitest.config.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/vitest.config.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("defineConfig".to_string()),
                local_name: "defineConfig".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["vitest"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.iter().any(|d| d.package_name == "vitest"),
        "dep imported only from root config files should be flagged as test-only"
    );
}

/// Regression test for #112: a dependency imported only from a workspace-level
/// config file (e.g., `packages/foo/vitest.config.ts`) should be flagged.
/// Before the fix, the root-anchored `*.config.*` glob didn't match nested configs.
#[test]
fn test_only_dep_from_workspace_config_file() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("defineConfig".to_string()),
                local_name: "defineConfig".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["vitest"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.iter().any(|d| d.package_name == "vitest"),
        "dep imported only from workspace-level config files should be flagged (issue #112)"
    );
}

/// Application config files (e.g., `app.config.ts`) should NOT cause a dependency
/// to be flagged as test-only. This prevents the false positive from #111.
#[test]
fn not_test_only_when_imported_from_app_config() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/app/app.config.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/app/app.config.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/app/app.config.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@angular/router".to_string(),
                imported_name: ImportedName::Named("provideRouter".to_string()),
                local_name: "provideRouter".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("@angular/router".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["@angular/router"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.is_empty(),
        "app.config.ts is application code, not a tooling config (issue #111)"
    );
}

/// A dep imported from both a config file and a source file should NOT be flagged.
#[test]
fn not_test_only_when_also_imported_from_source() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/app.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/src/app.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/packages/foo/vitest.config.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "some-lib".to_string(),
                    imported_name: ImportedName::Named("configure".to_string()),
                    local_name: "configure".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("some-lib".to_string()),
            }],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/app.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "some-lib".to_string(),
                    imported_name: ImportedName::Named("doSomething".to_string()),
                    local_name: "doSomething".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("some-lib".to_string()),
            }],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
        },
    ];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["some-lib"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.is_empty(),
        "dep imported from both config and source files should NOT be flagged"
    );
}

/// Workspace-level jest.config.ts should also be recognized (not just vitest).
#[test]
fn test_only_dep_from_workspace_jest_config() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/packages/api/jest.config.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/packages/api/jest.config.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/packages/api/jest.config.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "ts-jest".to_string(),
                imported_name: ImportedName::Named("pathsToModuleNameMapper".to_string()),
                local_name: "pathsToModuleNameMapper".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("ts-jest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    let pkg = make_pkg(&["ts-jest"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let test_only = find_test_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        test_only.iter().any(|d| d.package_name == "ts-jest"),
        "dep imported only from workspace-level jest.config.ts should be flagged"
    );
}
