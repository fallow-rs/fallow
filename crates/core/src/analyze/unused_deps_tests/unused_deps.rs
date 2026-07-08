use super::helpers::*;

#[test]
fn unused_dep_flagged_when_never_imported() {
    let (graph, _) = build_graph_with_npm_imports(&[("react", false)]);
    let pkg = make_pkg(&["react", "lodash"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, unused_dev, unused_optional) =
        find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused.iter().any(|d| d.package_name == "lodash"),
        "lodash is never imported and should be flagged"
    );
    assert!(
        !unused.iter().any(|d| d.package_name == "react"),
        "react is imported and should NOT be flagged"
    );
    assert!(unused_dev.is_empty());
    assert!(unused_optional.is_empty());
}

#[test]
fn known_tooling_dev_deps_not_flagged_as_unused() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&[], &["jest", "vitest"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, unused_dev, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(unused.is_empty());
    assert!(
        !unused_dev.iter().any(|d| d.package_name == "jest"),
        "jest is a known tooling dep and should be filtered"
    );
    assert!(
        !unused_dev.iter().any(|d| d.package_name == "vitest"),
        "vitest is a known tooling dep and should be filtered"
    );
}

#[test]
fn unused_dev_dep_non_tooling_is_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&[], &["my-custom-lib"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (_, unused_dev, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused_dev.iter().any(|d| d.package_name == "my-custom-lib"),
        "non-tooling dev dep should be flagged as unused"
    );
}

#[test]
fn unused_optional_dep_flagged_when_never_imported() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&[], &[], &["sharp"]);
    let config = test_config(PathBuf::from("/project"));

    let (_, _, unused_optional) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused_optional.iter().any(|d| d.package_name == "sharp"),
        "unused optional dep should be flagged"
    );
}

#[test]
fn implicit_deps_not_flagged_as_unused() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["react-dom", "@types/node"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "react-dom"),
        "react-dom is implicit and should not be flagged"
    );
    assert!(
        !unused.iter().any(|d| d.package_name == "@types/node"),
        "@types/node is implicit and should not be flagged"
    );
}

#[test]
fn unused_workspace_package_names_are_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["@myorg/shared"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/shared"),
        name: "@myorg/shared".to_string(),
        is_internal_dependency: false,
    }];

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &workspaces);

    assert!(
        unused.iter().any(|d| d.package_name == "@myorg/shared"),
        "declared workspace package dependency should be flagged when unused"
    );
}

#[test]
fn ignore_dependencies_config_filters_deps() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["my-internal-pkg"], &[], &[]);

    let config = FallowConfig {
        ignore_dependencies: vec!["my-internal-pkg".to_string()],
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

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "my-internal-pkg"),
        "deps in ignoreDependencies should not be flagged"
    );
}

#[test]
fn plugin_referenced_deps_not_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["tailwindcss"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .referenced_dependencies
        .push("tailwindcss".to_string());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, Some(&plugin_result), &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "tailwindcss"),
        "plugin-referenced deps should not be flagged"
    );
}

#[test]
fn plugin_tooling_deps_not_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["my-framework-runtime"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .tooling_dependencies
        .push("my-framework-runtime".to_string());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, Some(&plugin_result), &[]);

    assert!(
        !unused
            .iter()
            .any(|d| d.package_name == "my-framework-runtime"),
        "plugin tooling deps should not be flagged"
    );
}

#[test]
fn script_used_packages_not_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["concurrently"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .script_used_packages
        .insert("concurrently".to_string());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, Some(&plugin_result), &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "concurrently"),
        "packages used in scripts should not be flagged"
    );
}

#[test]
fn peer_dependency_of_used_package_not_flagged() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("node_modules/react-dom")).expect("create react-dom dir");
    std::fs::write(
        root.join("node_modules/react-dom/package.json"),
        r#"{"name":"react-dom","peerDependencies":{"react":"^18.0.0"}}"#,
    )
    .expect("write react-dom package");

    let (graph, _) = build_graph_with_npm_imports(&[("react-dom", false)]);
    let pkg = make_pkg(&["react", "react-dom"], &[], &[]);
    let config = test_config(root.to_path_buf());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "react"),
        "react is a peer dependency of used react-dom and should not be flagged: {unused:?}"
    );
}

#[test]
fn peer_dependency_of_parent_installed_package_not_flagged() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path().join("monorepo/packages/app");
    let parent = tmp.path().join("monorepo");
    std::fs::create_dir_all(parent.join("node_modules/react-dom"))
        .expect("create parent react-dom dir");
    std::fs::write(
        parent.join("node_modules/react-dom/package.json"),
        r#"{"name":"react-dom","peerDependencies":{"react":"^18.0.0"}}"#,
    )
    .expect("write parent react-dom package");

    let (graph, _) = build_graph_with_npm_imports(&[("react-dom", false)]);
    let pkg = make_pkg(&["react", "react-dom"], &[], &[]);
    let config = test_config(root);

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "react"),
        "react is a peer dependency of hoisted react-dom and should not be flagged: {unused:?}"
    );
}

#[test]
fn recursive_peer_dependencies_of_used_package_not_flagged() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("node_modules/plugin-a")).expect("create plugin-a dir");
    std::fs::create_dir_all(root.join("node_modules/plugin-b")).expect("create plugin-b dir");
    std::fs::write(
        root.join("node_modules/plugin-a/package.json"),
        r#"{"name":"plugin-a","peerDependencies":{"plugin-b":"^1.0.0"}}"#,
    )
    .expect("write plugin-a package");
    std::fs::write(
        root.join("node_modules/plugin-b/package.json"),
        r#"{"name":"plugin-b","peerDependencies":{"react":"^18.0.0"}}"#,
    )
    .expect("write plugin-b package");

    let (graph, _) = build_graph_with_npm_imports(&[("plugin-a", false)]);
    let pkg = make_pkg(&["plugin-a", "plugin-b", "react"], &[], &[]);
    let config = test_config(root.to_path_buf());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);
    let unused_names: Vec<&str> = unused.iter().map(|dep| dep.package_name.as_str()).collect();

    assert!(
        !unused_names.contains(&"plugin-b") && !unused_names.contains(&"react"),
        "recursive peer deps should be credited from used plugin-a, got: {unused_names:?}"
    );
}

#[test]
fn optional_peer_dependency_of_used_package_is_still_flagged_when_unused() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("node_modules/plugin-a")).expect("create plugin-a dir");
    std::fs::write(
        root.join("node_modules/plugin-a/package.json"),
        r#"{
  "name": "plugin-a",
  "peerDependencies": {"optional-peer": "^1.0.0"},
  "peerDependenciesMeta": {"optional-peer": {"optional": true}}
}"#,
    )
    .expect("write plugin-a package");

    let (graph, _) = build_graph_with_npm_imports(&[("plugin-a", false)]);
    let pkg = make_pkg(&["plugin-a", "optional-peer"], &[], &[]);
    let config = test_config(root.to_path_buf());

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused.iter().any(|d| d.package_name == "optional-peer"),
        "optional peer dependencies are not required by the used package and should still be reported: {unused:?}"
    );
}

#[test]
fn unused_dep_location_is_correct() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["unused-dep"], &["unused-dev"], &["unused-opt"]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, unused_dev, unused_optional) =
        find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(unused.iter().any(|d| d.package_name == "unused-dep"
        && matches!(d.location, DependencyLocation::Dependencies)));
    assert!(unused_dev.iter().any(|d| d.package_name == "unused-dev"
        && matches!(d.location, DependencyLocation::DevDependencies)));
    assert!(
        unused_optional
            .iter()
            .any(|d| d.package_name == "unused-opt"
                && matches!(d.location, DependencyLocation::OptionalDependencies))
    );
}

#[test]
fn scoped_package_subpath_import_recognized_as_used() {
    let (graph, _resolved_modules) = build_graph_with_npm_imports(&[("@chakra-ui/react", false)]);
    let pkg = make_pkg(&["@chakra-ui/react"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused.is_empty(),
        "@chakra-ui/react should be recognized as used via subpath import"
    );
}

#[test]
fn optional_dep_in_peer_deps_also_counts() {
    let (graph, _) = build_graph_with_npm_imports(&[("sharp", false)]);
    let pkg = make_pkg(&[], &[], &["sharp"]);
    let config = test_config(PathBuf::from("/project"));

    let (_, _, unused_optional) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused_optional.is_empty(),
        "optional dep that is imported should not be flagged as unused"
    );
}

#[test]
fn no_deps_produces_no_unused() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, unused_dev, unused_optional) =
        find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(unused.is_empty());
    assert!(unused_dev.is_empty());
    assert!(unused_optional.is_empty());
}

#[test]
fn no_imports_flags_all_non_implicit_deps() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&["lodash", "axios"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, _, _) = find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(unused.iter().any(|d| d.package_name == "lodash"));
    assert!(unused.iter().any(|d| d.package_name == "axios"));
}

#[test]
fn unlisted_dep_has_import_sites() {
    let (graph, resolved_modules) = build_graph_with_npm_imports(&[("unlisted-pkg", false)]);
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

    assert_eq!(unlisted.len(), 1);
    assert_eq!(unlisted[0].package_name, "unlisted-pkg");
    assert!(
        !unlisted[0].imported_from.is_empty(),
        "unlisted dep should have at least one import site"
    );
    assert_eq!(
        unlisted[0].imported_from[0].path,
        PathBuf::from("/project/src/index.ts")
    );
}

#[test]
fn path_alias_imports_not_reported_as_unlisted() {
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
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "@/components/Button".to_string(),
                    imported_name: ImportedName::Named("Button".to_string()),
                    local_name: "Button".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 30),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("@/components/Button".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "~/utils/helper".to_string(),
                    imported_name: ImportedName::Named("helper".to_string()),
                    local_name: "helper".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(35, 60),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("~/utils/helper".to_string()),
            },
        ],
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
        "path aliases should never be flagged as unlisted dependencies"
    );
}

#[test]
fn multiple_unresolved_imports_collected() {
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![
            ResolvedImport {
                info: ImportInfo {
                    source: "./missing-a".to_string(),
                    imported_name: ImportedName::Named("a".to_string()),
                    local_name: "a".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::Unresolvable("./missing-a".to_string()),
            },
            ResolvedImport {
                info: ImportInfo {
                    source: "./missing-b".to_string(),
                    imported_name: ImportedName::Named("b".to_string()),
                    local_name: "b".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(25, 45),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::Unresolvable("./missing-b".to_string()),
            },
        ],
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

    let config = test_config(PathBuf::from("/project"));
    let suppressions = SuppressionContext::empty();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();

    let unresolved = find_unresolved_imports(
        &resolved_modules,
        &config,
        &suppressions,
        &[],
        &[],
        &[],
        &line_offsets,
    );

    assert_eq!(unresolved.len(), 2);
    assert!(unresolved.iter().any(|u| u.specifier == "./missing-a"));
    assert!(unresolved.iter().any(|u| u.specifier == "./missing-b"));
}

#[test]
fn all_deps_used_produces_no_unused() {
    let (graph, _) =
        build_graph_with_npm_imports(&[("react", false), ("lodash", false), ("axios", false)]);
    let pkg = make_pkg(&["react", "lodash", "axios"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, unused_dev, unused_optional) =
        find_unused_dependencies(&graph, &pkg, &config, None, &[]);

    assert!(
        unused.is_empty(),
        "all deps are used, none should be flagged"
    );
    assert!(unused_dev.is_empty());
    assert!(unused_optional.is_empty());
}

#[test]
fn workspace_dep_used_within_workspace_not_flagged() {
    let ws_root = PathBuf::from("/project/packages/web");
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: ws_root.join("src/index.ts"),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: ws_root.join("src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: ws_root.join("src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
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

    let root_pkg = make_pkg(&[], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let (unused, _, _) = find_unused_dependencies(&graph, &root_pkg, &config, None, &[]);

    assert!(
        !unused.iter().any(|d| d.package_name == "react"),
        "react should not be in root unused since it's not in root deps"
    );
}

#[test]
fn plugin_tooling_dev_deps_not_flagged() {
    let (graph, _) = build_graph_with_npm_imports(&[]);
    let pkg = make_pkg(&[], &["my-dev-tool"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let mut plugin_result = AggregatedPluginResult::default();
    plugin_result
        .tooling_dependencies
        .push("my-dev-tool".to_string());

    let (_, unused_dev, _) =
        find_unused_dependencies(&graph, &pkg, &config, Some(&plugin_result), &[]);

    assert!(
        !unused_dev.iter().any(|d| d.package_name == "my-dev-tool"),
        "plugin tooling dev deps should not be flagged as unused"
    );
}
