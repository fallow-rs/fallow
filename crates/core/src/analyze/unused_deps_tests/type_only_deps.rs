use super::helpers::*;

#[test]
fn type_only_dep_detected_when_all_imports_are_type_only() {
    let (graph, _) = build_graph_with_npm_imports(&[("zod", true)]);
    let pkg = make_pkg(&["zod"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        type_only.iter().any(|d| d.package_name == "zod"),
        "dep used only via `import type` should be flagged as type-only"
    );
}

#[test]
fn type_only_dep_not_detected_when_runtime_import_exists() {
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/other.ts"),
            size_bytes: 100,
        },
    ];

    let entry_points = vec![
        EntryPoint {
            path: PathBuf::from("/project/src/index.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
        EntryPoint {
            path: PathBuf::from("/project/src/other.ts"),
            source: EntryPointSource::PackageJsonMain,
        },
    ];

    let resolved_modules = vec![
        ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "zod".to_string(),
                    imported_name: ImportedName::Named("z".to_string()),
                    local_name: "z".to_string(),
                    is_type_only: true,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("zod".to_string()),
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
            path: PathBuf::from("/project/src/other.ts"),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "zod".to_string(),
                    imported_name: ImportedName::Named("z".to_string()),
                    local_name: "z".to_string(),
                    is_type_only: false, // runtime import
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("zod".to_string()),
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
    let pkg = make_pkg(&["zod"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        type_only.is_empty(),
        "dep with mixed type-only and runtime imports should NOT be flagged"
    );
}

#[test]
fn type_only_dep_not_detected_when_unused() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["zod"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        type_only.is_empty(),
        "completely unused deps should not appear in type_only results"
    );
}

#[test]
fn type_only_dep_skips_workspace_packages() {
    let (graph, _) = build_graph_with_npm_imports(&[("@myorg/types", true)]);
    let pkg = make_pkg(&["@myorg/types"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/types"),
        name: "@myorg/types".to_string(),
        is_internal_dependency: false,
    }];

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &workspaces);

    assert!(
        type_only.is_empty(),
        "workspace packages should not be flagged as type-only deps"
    );
}

#[test]
fn type_only_dep_skips_ignored_deps() {
    let (graph, _) = build_graph_with_npm_imports(&[("zod", true)]);
    let pkg = make_pkg(&["zod"], &[], &[]);

    let config = FallowConfig {
        ignore_dependencies: vec!["zod".to_string()],
        ignore_unresolved_imports: vec![],
        ..Default::default()
    }
    .resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        type_only.is_empty(),
        "ignored deps should not be flagged as type-only"
    );
}

#[test]
fn type_only_dep_ignores_dev_dependencies() {
    let (graph, _) = build_graph_with_npm_imports(&[("@types/lodash", true)]);
    let pkg = make_pkg(&[], &["@types/lodash"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let type_only = find_type_only_dependencies(&graph, &pkg, &config, &[]);

    assert!(
        type_only.is_empty(),
        "dev deps should not appear in type-only dependency results"
    );
}
