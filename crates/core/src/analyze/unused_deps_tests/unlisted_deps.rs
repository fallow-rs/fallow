use super::helpers::*;

// ---- find_unlisted_dependencies tests ----

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
    // Import "fs" (a Node.js builtin) - should never be unlisted
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    // NpmPackage("fs") would be the resolve result if it were npm.
    // But in practice, builtins are tracked as NpmPackage in package_usage.
    // The key filter is is_builtin_module in find_unlisted_dependencies.
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
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
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
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
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

    // Use a non-path-alias virtual prefix (not "#" which is_path_alias catches)
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
    // react is listed as a peer dep only, not in deps/devDeps
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

// ---- Additional coverage: unlisted dep in workspace scope ----

#[test]
fn unlisted_dep_detected_across_multiple_files() {
    // Two files both import the same unlisted package — should deduplicate per file
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
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
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
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
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
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
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

// ---- virtual_package_suffixes suppression ----

#[test]
fn vitest_mocks_package_not_reported_as_unlisted_via_suffix() {
    // Imports like `@aws-sdk/__mocks__` should not be flagged when Vitest plugin
    // contributes `/__mocks__` as a virtual package suffix.
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

// ---- Additional coverage: find_unlisted_dependencies with optional dep listed ----

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

// ---- @types/<package> unlisted dependency false positive tests ----

#[test]
fn type_only_import_with_at_types_package_not_unlisted() {
    // `import type { Feature } from 'geojson'` with @types/geojson in devDeps
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
    // `import { Feature } from 'geojson'` (value import syntax) with @types/geojson in devDeps.
    // TypeScript resolves types from @types/ and erases the import — the bare package is not needed.
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
    // `import type { Foo } from '@scope/pkg'` with @types/scope__pkg in devDeps
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
    // `import { Feature } from 'geojson'` + `import type { Point } from 'geojson'`
    // with only @types/geojson — suppressed because @types/ presence means types-only usage
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
    // `import { axios } from 'axios'` with NO @types/axios — still flagged
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
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("bun:sqlite", false)]);
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
        !unlisted.iter().any(|d| d.package_name == "bun:sqlite"),
        "bun:sqlite builtin should not be flagged as unlisted"
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
