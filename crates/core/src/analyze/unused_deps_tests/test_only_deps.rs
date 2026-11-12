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
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("describe".to_string()),
                local_name: "describe".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
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
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("defineConfig".to_string()),
                local_name: "defineConfig".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
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
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "vitest".to_string(),
                imported_name: ImportedName::Named("defineConfig".to_string()),
                local_name: "defineConfig".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("vitest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
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
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@angular/router".to_string(),
                imported_name: ImportedName::Named("provideRouter".to_string()),
                local_name: "provideRouter".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("@angular/router".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
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
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "some-lib".to_string(),
                    imported_name: ImportedName::Named("configure".to_string()),
                    local_name: "configure".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("some-lib".to_string()),
            }],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![].into(),
            semantic_facts: std::sync::Arc::default(),
            whole_object_uses: std::sync::Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: std::sync::Arc::default(),
            exported_factory_return_object_shapes: std::sync::Arc::default(),
            type_member_types: std::sync::Arc::default(),
            missing_export_targets: vec![],
        },
        ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/app.ts"),
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "some-lib".to_string(),
                    imported_name: ImportedName::Named("doSomething".to_string()),
                    local_name: "doSomething".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("some-lib".to_string()),
            }],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![].into(),
            semantic_facts: std::sync::Arc::default(),
            whole_object_uses: std::sync::Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: std::sync::Arc::default(),
            exported_factory_return_object_shapes: std::sync::Arc::default(),
            type_member_types: std::sync::Arc::default(),
            missing_export_targets: vec![],
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
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "ts-jest".to_string(),
                imported_name: ImportedName::Named("pathsToModuleNameMapper".to_string()),
                local_name: "pathsToModuleNameMapper".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("ts-jest".to_string()),
        }],
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
        missing_export_targets: vec![],
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

/// Build a graph where each file is an entry point that imports the given npm
/// packages.
#[expect(
    clippy::cast_possible_truncation,
    reason = "test file ids and span values are trivially small"
)]
fn graph_with_importers(importers: &[(&str, &[&str])]) -> ModuleGraph {
    let files: Vec<DiscoveredFile> = importers
        .iter()
        .enumerate()
        .map(|(i, (path, _))| DiscoveredFile {
            id: FileId(i as u32),
            path: PathBuf::from(path),
            size_bytes: 100,
        })
        .collect();
    let entry_points: Vec<EntryPoint> = importers
        .iter()
        .map(|(path, _)| EntryPoint {
            path: PathBuf::from(path),
            source: EntryPointSource::PackageJsonMain,
        })
        .collect();
    let resolved_modules: Vec<ResolvedModule> = importers
        .iter()
        .enumerate()
        .map(|(i, (path, packages))| ResolvedModule {
            file_id: FileId(i as u32),
            path: PathBuf::from(path),
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: packages
                .iter()
                .enumerate()
                .map(|(j, package)| ResolvedImport {
                    info: ImportInfo {
                        source: (*package).to_string(),
                        imported_name: ImportedName::Default,
                        local_name: format!("import_{j}"),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::new((j * 20) as u32, (j * 20 + 15) as u32),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::NpmPackage((*package).to_string()),
                })
                .collect(),
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![].into(),
            semantic_facts: std::sync::Arc::default(),
            whole_object_uses: std::sync::Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: std::sync::Arc::default(),
            exported_factory_return_object_shapes: std::sync::Arc::default(),
            type_member_types: std::sync::Arc::default(),
            missing_export_targets: vec![],
        })
        .collect();
    ModuleGraph::build(&resolved_modules, &entry_points, &files)
}

fn test_only_names(importers: &[(&str, &[&str])], deps: &[&str]) -> Vec<String> {
    let graph = graph_with_importers(importers);
    let pkg = make_pkg(deps, &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    find_test_only_dependencies(&graph, &pkg, &config, &[])
        .into_iter()
        .map(|d| d.package_name)
        .collect()
}

/// A framework config file (`astro.config.mjs`) configures the production
/// build, so the dependencies that only it imports are production use.
#[test]
fn not_test_only_when_imported_from_framework_config() {
    let names = test_only_names(
        &[(
            "/project/astro.config.mjs",
            &["astro", "@astrojs/react", "@astrojs/sitemap"],
        )],
        &["astro", "@astrojs/react", "@astrojs/sitemap"],
    );
    assert!(
        names.is_empty(),
        "framework config imports are production use, found: {names:?}"
    );
}

/// A build tool config file in a workspace (`vite.config.ts`) is production
/// use too.
#[test]
fn not_test_only_when_imported_from_workspace_build_config() {
    let names = test_only_names(
        &[(
            "/project/packages/app/vite.config.ts",
            &["@vitejs/plugin-react"],
        )],
        &["@vitejs/plugin-react"],
    );
    assert!(
        names.is_empty(),
        "build config imports are production use, found: {names:?}"
    );
}

/// A dependency that a framework config and a test file import is production
/// use, because the config file is not a test file.
#[test]
fn not_test_only_when_imported_from_framework_config_and_test() {
    let names = test_only_names(
        &[
            ("/project/next.config.mjs", &["next-plugin-lib"]),
            ("/project/src/app.test.ts", &["next-plugin-lib"]),
        ],
        &["next-plugin-lib"],
    );
    assert!(names.is_empty(), "found: {names:?}");
}

/// A test runner setup file (`vitest.setup.ts`) is a test file, so its
/// dependencies stay test-only.
#[test]
fn test_only_dep_from_test_runner_setup_and_test() {
    let names = test_only_names(
        &[
            ("/project/vitest.setup.ts", &["@testing-library/jest-dom"]),
            ("/project/src/app.test.ts", &["@testing-library/jest-dom"]),
        ],
        &["@testing-library/jest-dom"],
    );
    assert_eq!(names, vec!["@testing-library/jest-dom".to_string()]);
}

/// A config file inside a test directory is a test fixture, so its
/// dependencies stay test-only.
#[test]
fn test_only_dep_from_config_in_test_directory() {
    let names = test_only_names(
        &[("/project/tests/fixtures/astro.config.mjs", &["astro"])],
        &["astro"],
    );
    assert_eq!(names, vec!["astro".to_string()]);
}
