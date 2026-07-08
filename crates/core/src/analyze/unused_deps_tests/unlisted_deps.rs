use super::helpers::*;

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
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "node:fs".to_string(),
                imported_name: ImportedName::Named("readFile".to_string()),
                local_name: "readFile".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 25),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("node:fs".to_string()),
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
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "virtual:pwa-register".to_string(),
                imported_name: ImportedName::Named("register".to_string()),
                local_name: "register".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 30),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("virtual:pwa-register".to_string()),
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
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "unlisted-pkg".to_string(),
                    imported_name: ImportedName::Named("foo".to_string()),
                    local_name: "foo".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
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
            path: PathBuf::from("/project/src/b.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "unlisted-pkg".to_string(),
                    imported_name: ImportedName::Named("bar".to_string()),
                    local_name: "bar".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
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
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![],
        resolved_dynamic_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: "unlisted-pkg".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(14, 40),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("unlisted-pkg".to_string()),
        }],
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

#[test]
fn vitest_mocks_package_not_reported_as_unlisted_via_suffix() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("@aws-sdk/__mocks__", false)]);
    let pkg = make_pkg(&[], &["vitest"], &[]);
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
    config.ignore_dependencies = vec!["axios".to_string()];
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
fn workspace_file_does_not_use_root_manifest_for_unlisted_check() {
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
        "workspace imports must be checked against their own package.json, not root deps"
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

fn workspace_import_case(
    package_name: &str,
    is_type_only: bool,
    sibling_package_json: Option<&str>,
) -> WorkspaceImportCase {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("repo");
    let app_root = root.join("packages/app");
    std::fs::create_dir_all(app_root.join("src")).expect("create workspace source");
    std::fs::write(app_root.join("package.json"), r#"{"name":"app"}"#)
        .expect("write app package json");

    let mut workspaces = vec![WorkspaceInfo {
        root: app_root.clone(),
        name: "app".to_string(),
        is_internal_dependency: false,
    }];

    if let Some(package_json) = sibling_package_json {
        let sibling_root = root.join("packages/types-owner");
        std::fs::create_dir_all(sibling_root.join("src")).expect("create sibling source");
        std::fs::write(sibling_root.join("package.json"), package_json)
            .expect("write sibling package json");
        workspaces.push(WorkspaceInfo {
            root: sibling_root,
            name: "types-owner".to_string(),
            is_internal_dependency: false,
        });
    }

    let file_path = app_root.join("src/index.ts");
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: file_path.clone(),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: file_path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: file_path,
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: package_name.to_string(),
                imported_name: ImportedName::Named("value".to_string()),
                local_name: "value".to_string(),
                is_type_only,
                from_style: false,
                span: oxc_span::Span::new(0, 35),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage(package_name.to_string()),
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

    WorkspaceImportCase {
        tmp,
        root,
        graph,
        resolved_modules,
        workspaces,
    }
}
