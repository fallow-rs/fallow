use super::helpers::*;

#[test]
fn deno_ambient_workspace_names_do_not_leak_into_npm_members() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let deno_root = dir.path().join("packages/deno-lib");
    let npm_root = dir.path().join("packages/npm-app");
    std::fs::create_dir_all(&deno_root).unwrap();
    std::fs::create_dir_all(&npm_root).unwrap();
    std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
    std::fs::write(deno_root.join("deno.json"), r#"{"name":"@scope/deno-lib"}"#).unwrap();
    std::fs::write(
        npm_root.join("package.json"),
        r#"{"name":"@scope/npm-app"}"#,
    )
    .unwrap();

    let workspaces = vec![
        WorkspaceInfo {
            root: deno_root.clone(),
            name: "@scope/deno-lib".to_string(),
            is_internal_dependency: false,
        },
        WorkspaceInfo {
            root: npm_root.clone(),
            name: "@scope/npm-app".to_string(),
            is_internal_dependency: false,
        },
    ];
    let config = test_config(dir.path().to_path_buf());
    let dependency_map = workspace_dependency_map(&workspaces, &config);
    let deno_deps = dependency_map
        .iter()
        .find(|ws| ws.root == deno_root)
        .map(|ws| &ws.deps)
        .unwrap();
    let npm_deps = dependency_map
        .iter()
        .find(|ws| ws.root == npm_root)
        .map(|ws| &ws.deps)
        .unwrap();

    assert!(deno_deps.contains("@scope/deno-lib"));
    assert!(!deno_deps.contains("@scope/npm-app"));
    assert!(npm_deps.contains("@scope/npm-app"));
    assert!(!npm_deps.contains("@scope/deno-lib"));
}

#[test]
fn unlisted_dep_detected_when_not_in_package_json() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("axios", false)]);
    let pkg = make_pkg(&["react"], &[], &[]); // axios is NOT listed
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|d| d.package_name == "axios"),
        "axios is imported but not listed, should be unlisted"
    );
}

#[test]
fn listed_dep_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("react", false)]);
    let pkg = make_pkg(&["react"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "dep listed in dependencies should not be flagged as unlisted"
    );
}

#[test]
fn dev_dep_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("jest", false)]);
    let pkg = make_pkg(&[], &["jest"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "dep listed in devDependencies should not be unlisted"
    );
}

#[test]
fn builtin_modules_not_reported_as_unlisted() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "node:fs".to_string(),
                imported_name: ImportedName::Named("readFile".to_string()),
                local_name: "readFile".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 25),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("node:fs".to_string()),
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
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "node:fs"),
        "node:fs builtin should not be flagged as unlisted"
    );
}

#[test]
fn virtual_modules_not_reported_as_unlisted() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "virtual:pwa-register".to_string(),
                imported_name: ImportedName::Named("register".to_string()),
                local_name: "register".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 30),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("virtual:pwa-register".to_string()),
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
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "virtual: modules should not be flagged as unlisted"
    );
}

#[test]
fn undeclared_workspace_package_names_are_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("@myorg/utils", false)]);
    let pkg = make_pkg(&[], &[], &[]); // @myorg/utils NOT listed
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/utils"),
        name: "@myorg/utils".to_string(),
        is_internal_dependency: false,
    }];

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &workspaces,
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|d| d.package_name == "@myorg/utils"),
        "workspace package imports should be flagged when the importing package does not declare them"
    );
}

#[test]
fn undeclared_workspace_commonjs_require_retains_import_site() {
    let source_path = PathBuf::from("/project/src/index.js");
    let workspace_source_path = PathBuf::from("/project/packages/utils/src/index.ts");
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: source_path.clone(),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: workspace_source_path,
            size_bytes: 100,
        },
    ];
    let entry_points = vec![EntryPoint {
        path: source_path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: source_path.clone(),
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "@myorg/utils".to_string(),
                imported_name: ImportedName::Namespace,
                local_name: "utils".to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(42, 73),
                source_span: oxc_span::Span::new(64, 76),
            },
            target: ResolveResult::CommonJsInternalPackageModule {
                file_id: FileId(1),
                package_name: "@myorg/utils".to_string(),
            },
        }],
        ..ResolvedModule::default()
    }];
    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/utils"),
        name: "@myorg/utils".to_string(),
        is_internal_dependency: false,
    }];
    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);

    let line_offsets_storage = [0, 18, 36];
    let line_offsets = FxHashMap::from_iter([(FileId(0), line_offsets_storage.as_slice())]);
    let unlisted = find_unlisted_dependencies(
        &graph,
        &make_pkg(&[], &[], &[]),
        &test_config(PathBuf::from("/project")),
        &workspaces,
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert_eq!(unlisted.len(), 1);
    assert_eq!(unlisted[0].package_name, "@myorg/utils");
    assert_eq!(unlisted[0].imported_from.len(), 1);
    assert_eq!(unlisted[0].imported_from[0].path, source_path);
    assert_eq!(unlisted[0].imported_from[0].line, 3);
}

#[test]
fn plugin_virtual_prefixes_not_reported_as_unlisted() {
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let (graph2, resolved_modules2) = build_graph_with_npm_imports(&[("@theme/Layout", false)]);

    let mut plugin_result2 = AggregatedPluginResult::default();
    plugin_result2
        .virtual_module_prefixes
        .push("@theme/".to_string());

    let unlisted = find_unlisted_dependencies(
        &graph2,
        &pkg,
        &config,
        &[],
        Some(&plugin_result2),
        &resolved_modules2,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "@theme/Layout"),
        "imports matching virtual module prefixes should not be unlisted"
    );
}

#[test]
fn plugin_tooling_deps_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("h3", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result.tooling_dependencies.push("h3".to_string());

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        Some(&plugin_result),
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "h3"),
        "plugin tooling deps should not be flagged as unlisted"
    );
}

#[test]
fn peer_dep_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("react", false)]);
    let pkg: PackageJson = serde_json::from_str(r#"{"peerDependencies": {"react": "^18.0.0"}}"#)
        .expect("test pkg json");

    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "peer dependencies should not be flagged as unlisted"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "multi-file dependency fixture keeps the scenario local to the test"
)]
fn unlisted_dep_detected_across_multiple_files() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/a.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/b.ts"),
            size_bytes: 100,
        },
    ];
    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/src/a.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/src/b.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];
    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/a.ts"),
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "unlisted-pkg".to_string(),
                    imported_name: ImportedName::Named("foo".to_string()),
                    local_name: "foo".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
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
            path: PathBuf::from("/project/src/b.ts"),
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "unlisted-pkg".to_string(),
                    imported_name: ImportedName::Named("bar".to_string()),
                    local_name: "bar".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
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
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert_eq!(unlisted.len(), 1, "same unlisted pkg should be grouped");
    assert_eq!(unlisted[0].package_name, "unlisted-pkg");
    assert_eq!(
        unlisted[0].imported_from.len(),
        2,
        "should have import sites from both files"
    );
}

#[test]
fn dynamic_import_unlisted_dep_has_import_site() {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![],
        resolved_dynamic_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "unlisted-pkg".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(14, 40),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
        }],
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
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let offsets = vec![0, 12];
    let mut line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert_eq!(unlisted.len(), 1);
    assert_eq!(unlisted[0].package_name, "unlisted-pkg");
    assert_eq!(unlisted[0].imported_from.len(), 1);
    assert_eq!(unlisted[0].imported_from[0].line, 2);
    assert_eq!(unlisted[0].imported_from[0].col, 2);
}

/// The generic suffix mechanism stays available to external plugins even
/// though no built-in plugin declares `/__mocks__` anymore (issue #2226).
#[test]
fn scoped_mocks_package_not_reported_as_unlisted_via_declared_suffix() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("@aws-sdk/__mocks__", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .virtual_package_suffixes
        .push("/__mocks__".to_string());

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        Some(&plugin_result),
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "no unlisted deps expected when /__mocks__ suffix matches; got: {:?}",
        unlisted.iter().map(|d| &d.package_name).collect::<Vec<_>>()
    );
}

#[test]
fn plain_mocks_package_not_reported_as_unlisted_via_suffix() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("some-pkg/__mocks__", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .virtual_package_suffixes
        .push("/__mocks__".to_string());

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        Some(&plugin_result),
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.is_empty(),
        "no unlisted deps expected when /__mocks__ suffix matches unscoped; got: {:?}",
        unlisted.iter().map(|d| &d.package_name).collect::<Vec<_>>()
    );
}

#[test]
fn optional_dep_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("sharp", false)]);
    let pkg = make_pkg(&[], &[], &["sharp"]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "sharp"),
        "optional deps should count as listed and not be flagged as unlisted"
    );
}

#[test]
fn type_only_import_with_at_types_package_not_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("geojson", true)]);
    let pkg = make_pkg(&[], &["@types/geojson"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "geojson"),
        "type-only import of 'geojson' should not be flagged when @types/geojson is listed"
    );
}

#[test]
fn value_import_with_at_types_package_not_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("geojson", false)]);
    let pkg = make_pkg(&[], &["@types/geojson"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "geojson"),
        "import from 'geojson' should not be flagged when @types/geojson is listed"
    );
}

#[test]
fn scoped_type_only_import_with_at_types_package_not_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("@scope/pkg", true)]);
    let pkg = make_pkg(&[], &["@types/scope__pkg"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "@scope/pkg"),
        "type-only scoped import should not be flagged when @types/scope__pkg is listed"
    );
}

#[test]
fn at_types_without_bare_package_suppresses_regardless_of_import_style() {
    let (graph, resolved_modules) =
        build_graph_with_npm_imports(&[("geojson", false), ("geojson", true)]);
    let pkg = make_pkg(&[], &["@types/geojson"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "geojson"),
        "@types/geojson listed — geojson should not be flagged regardless of import style"
    );
}

#[test]
fn no_at_types_still_flags_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("axios", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|d| d.package_name == "axios"),
        "no @types/axios listed — axios should be flagged as unlisted"
    );
}

#[test]
fn bun_builtins_not_reported_as_unlisted() {
    let (graph, resolved_modules) =
        build_graph_with_npm_imports(&[("bun", false), ("bun:sqlite", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "bun"),
        "bun builtin should not be flagged as unlisted"
    );
    assert!(
        !unlisted.iter().any(|d| d.package_name == "bun:sqlite"),
        "bun:sqlite builtin should not be flagged as unlisted"
    );
}

/// `node:sqlite` is a mandatory-`node:`-prefix builtin and must not be flagged as
/// unlisted, while the bare `sqlite` form (a real npm package) still surfaces.
/// See issue #627.
#[test]
fn node_prefix_only_builtins_not_reported_as_unlisted() {
    let (graph, resolved_modules) =
        build_graph_with_npm_imports(&[("node:sqlite", false), ("sqlite", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "node:sqlite"),
        "node:sqlite builtin should not be flagged as unlisted"
    );
    assert!(
        unlisted.iter().any(|d| d.package_name == "sqlite"),
        "bare sqlite is a real npm package and should still be flagged as unlisted"
    );
}

#[test]
fn bun_type_only_builtin_not_reported_as_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("bun", true)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "bun"),
        "type-only bun builtin import should not be flagged as unlisted"
    );
}

#[test]
fn bun_slash_subpath_reported_as_unlisted() {
    let (graph, resolved_modules) =
        build_graph_with_npm_import_sources(&[("bun", "bun", false), ("bun/foo", "bun", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|d| d.package_name == "bun"),
        "bun slash subpaths should be treated as package imports, not Bun builtins"
    );
}

#[test]
fn ignore_dependencies_suppresses_unlisted() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("axios", false)]);
    let pkg = make_pkg(&[], &[], &[]); // axios is NOT listed
    let mut config = test_config(PathBuf::from("/project"));
    config.ignore_dependencies =
        fallow_config::IgnoreDependencyMatcher::compile(&["axios".to_string()]);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    assert!(
        !unlisted.iter().any(|d| d.package_name == "axios"),
        "axios in ignoreDependencies should not be flagged as unlisted"
    );
}

#[test]
fn ignore_dependencies_glob_suppresses_unlisted() {
    let (graph, resolved_modules) =
        build_graph_with_npm_imports(&[("@runtime/sqlite", false), ("axios", false)]);
    let pkg = make_pkg(&[], &[], &[]);
    let mut config = test_config(PathBuf::from("/project"));
    config.ignore_dependencies =
        fallow_config::IgnoreDependencyMatcher::compile(&["@runtime/*".to_string()]);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &graph,
        &pkg,
        &config,
        &[],
        None,
        &resolved_modules,
        &line_offsets,
    );

    let names: Vec<&str> = unlisted.iter().map(|d| d.package_name.as_str()).collect();
    assert_eq!(names, vec!["axios"]);
}

#[test]
fn production_file_of_publishable_workspace_does_not_use_root_manifest() {
    let case = workspace_import_case("react", false, None);
    let pkg = make_pkg(&["react"], &[], &[]);
    let config = test_config(case.root);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &case.graph,
        &pkg,
        &config,
        &case.workspaces,
        None,
        &case.resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|dep| dep.package_name == "react"),
        "a production file of a publishable workspace must declare its packages in its own package.json"
    );
}

fn unlisted_names_for(case: &WorkspaceImportCase, root_pkg: &PackageJson) -> Vec<String> {
    let config = test_config(case.root.clone());
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    find_unlisted_dependencies(
        &case.graph,
        root_pkg,
        &config,
        &case.workspaces,
        None,
        &case.resolved_modules,
        &line_offsets,
    )
    .into_iter()
    .map(|dep| dep.package_name)
    .collect()
}

#[test]
fn private_workspace_file_uses_root_manifest() {
    let case = workspace_case(&WorkspaceCaseSpec {
        package_name: "react",
        workspaces: &[("packages/app", r#"{"name":"app","private":true}"#)],
        file: "packages/app/src/index.ts",
        is_entry: true,
    });

    assert!(
        unlisted_names_for(&case, &make_pkg(&["react"], &[], &[])).is_empty(),
        "a private workspace may use a package that the root manifest declares"
    );
    assert_eq!(
        unlisted_names_for(&case, &make_pkg(&[], &[], &[])),
        vec!["react".to_string()],
        "the package stays unlisted when no manifest in the chain declares it"
    );
}

#[test]
fn test_file_of_publishable_workspace_uses_root_dev_dependency() {
    let case = workspace_case(&WorkspaceCaseSpec {
        package_name: "test-helper",
        workspaces: &[("packages/app", r#"{"name":"app","version":"1.0.0"}"#)],
        file: "packages/app/src/index.test.ts",
        is_entry: true,
    });

    assert!(
        unlisted_names_for(&case, &make_pkg(&[], &["test-helper"], &[])).is_empty(),
        "a test file may use a devDependency that the root manifest declares"
    );
}

#[test]
fn nested_workspace_script_uses_ancestor_workspace_manifest() {
    let case = workspace_case(&WorkspaceCaseSpec {
        package_name: "build-kit",
        workspaces: &[
            (
                "apps/tool",
                r#"{"name":"tool","version":"1.0.0","dependencies":{"build-kit":"1.0.0"}}"#,
            ),
            (
                "apps/tool/packages/cli",
                r#"{"name":"cli","version":"1.0.0"}"#,
            ),
        ],
        file: "apps/tool/packages/cli/scripts/build.mjs",
        is_entry: false,
    });

    assert!(
        unlisted_names_for(&case, &make_pkg(&[], &[], &[])).is_empty(),
        "a build script of a nested workspace may use the ancestor workspace's declaration"
    );
}

#[test]
fn nested_publishable_production_file_does_not_use_ancestor_workspace_manifest() {
    let case = workspace_case(&WorkspaceCaseSpec {
        package_name: "build-kit",
        workspaces: &[
            (
                "apps/tool",
                r#"{"name":"tool","version":"1.0.0","dependencies":{"build-kit":"1.0.0"}}"#,
            ),
            (
                "apps/tool/packages/cli",
                r#"{"name":"cli","version":"1.0.0"}"#,
            ),
        ],
        file: "apps/tool/packages/cli/src/index.ts",
        is_entry: true,
    });

    assert_eq!(
        unlisted_names_for(&case, &make_pkg(&[], &[], &[])),
        vec!["build-kit".to_string()],
        "a production file of a publishable nested workspace keeps the strict check"
    );
}

#[test]
fn private_workspace_does_not_use_sibling_manifest() {
    let case = workspace_case(&WorkspaceCaseSpec {
        package_name: "react",
        workspaces: &[
            ("packages/app", r#"{"name":"app","private":true}"#),
            (
                "packages/other",
                r#"{"name":"other","dependencies":{"react":"1.0.0"}}"#,
            ),
        ],
        file: "packages/app/src/index.ts",
        is_entry: true,
    });

    assert_eq!(
        unlisted_names_for(&case, &make_pkg(&[], &[], &[])),
        vec!["react".to_string()],
        "the walk covers ancestors only, never a sibling workspace"
    );
}

#[test]
fn sibling_at_types_package_does_not_suppress_unlisted_check() {
    let case = workspace_import_case(
        "geojson",
        true,
        Some(r#"{"name":"types-owner","devDependencies":{"@types/geojson":"^1.0.0"}}"#),
    );
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(case.root);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unlisted = find_unlisted_dependencies(
        &case.graph,
        &pkg,
        &config,
        &case.workspaces,
        None,
        &case.resolved_modules,
        &line_offsets,
    );

    assert!(
        unlisted.iter().any(|dep| dep.package_name == "geojson"),
        "a sibling workspace's @types package must not satisfy the importing workspace"
    );
}

struct WorkspaceImportCase {
    #[expect(dead_code, reason = "keeps tempdir alive for workspace package files")]
    tmp: tempfile::TempDir,
    root: PathBuf,
    graph: ModuleGraph,
    resolved_modules: Vec<ResolvedModule>,
    workspaces: Vec<WorkspaceInfo>,
}

struct WorkspaceCaseSpec<'a> {
    package_name: &'a str,
    /// Workspace root relative to the repo root, and its `package.json`.
    workspaces: &'a [(&'a str, &'a str)],
    /// The importing file, relative to the repo root.
    file: &'a str,
    /// Whether the file is a runtime entry point (`main`).
    is_entry: bool,
}

fn workspace_import_case(
    package_name: &str,
    is_type_only: bool,
    sibling_package_json: Option<&str>,
) -> WorkspaceImportCase {
    let mut workspaces = vec![("packages/app", r#"{"name":"app"}"#)];
    if let Some(package_json) = sibling_package_json {
        workspaces.push(("packages/types-owner", package_json));
    }
    build_workspace_case(
        &WorkspaceCaseSpec {
            package_name,
            workspaces: &workspaces,
            file: "packages/app/src/index.ts",
            is_entry: true,
        },
        is_type_only,
    )
}

fn workspace_case(spec: &WorkspaceCaseSpec<'_>) -> WorkspaceImportCase {
    build_workspace_case(spec, false)
}

fn build_workspace_case(spec: &WorkspaceCaseSpec<'_>, is_type_only: bool) -> WorkspaceImportCase {
    let package_name = spec.package_name;
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("repo");

    let mut workspaces = Vec::new();
    for (relative_root, package_json) in spec.workspaces {
        let ws_root = root.join(relative_root);
        std::fs::create_dir_all(&ws_root).expect("create workspace root");
        std::fs::write(ws_root.join("package.json"), package_json)
            .expect("write workspace package json");
        let manifest: serde_json::Value =
            serde_json::from_str(package_json).expect("workspace package json parses");
        workspaces.push(WorkspaceInfo {
            root: ws_root,
            name: manifest["name"]
                .as_str()
                .expect("workspace package json has a name")
                .to_string(),
            is_internal_dependency: false,
        });
    }

    let file_path = root.join(spec.file);
    std::fs::create_dir_all(file_path.parent().expect("file has a parent"))
        .expect("create source directory");
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: file_path.clone(),
        size_bytes: 100,
    }];
    let entry_points = if spec.is_entry {
        vec![EntryPoint {
            path: file_path.clone(),
            source: EntryPointSource::PackageJsonMain,
        }]
    } else {
        Vec::new()
    };
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: file_path,
        exports: vec![].into(),
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: package_name.to_string(),
                imported_name: ImportedName::Named("value".to_string()),
                local_name: "value".to_string(),
                is_type_only,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 35),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage(package_name.to_string()),
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

    WorkspaceImportCase {
        tmp,
        root,
        graph,
        resolved_modules,
        workspaces,
    }
}
