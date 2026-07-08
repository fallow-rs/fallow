use super::helpers::*;

/// Build a graph with a single import of `package` (value or type-only) from a
/// given file path, so the production-vs-test classification can be exercised.
fn graph_with_import_from(
    file_path: &str,
    package: &str,
    is_type_only: bool,
) -> (ModuleGraph, Vec<ResolvedModule>) {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from(file_path),
        size_bytes: 100,
    }];
    let entry_points = vec![EntryPoint {
        path: PathBuf::from(file_path),
        source: EntryPointSource::PackageJsonMain,
    }];
    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from(file_path),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: vec![ResolvedImport {
            info: ImportInfo {
                source: package.to_string(),
                imported_name: ImportedName::Named("thing".to_string()),
                local_name: "thing".to_string(),
                is_type_only,
                from_style: false,
                span: oxc_span::Span::new(0, 20),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage(package.to_string()),
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
    (graph, resolved_modules)
}

/// Build a `PackageJson` with `devDependencies` and `peerDependencies`.
fn pkg_with_peer(dev_deps: &[&str], peer_deps: &[&str]) -> PackageJson {
    let to_obj = |names: &[&str]| -> serde_json::Value {
        serde_json::Value::Object(
            names
                .iter()
                .map(|n| ((*n).to_string(), serde_json::Value::String("^1.0.0".into())))
                .collect(),
        )
    };
    let mut obj = serde_json::Map::new();
    obj.insert(
        "name".into(),
        serde_json::Value::String("test-project".into()),
    );
    obj.insert("devDependencies".into(), to_obj(dev_deps));
    obj.insert("peerDependencies".into(), to_obj(peer_deps));
    serde_json::from_value(serde_json::Value::Object(obj)).expect("pkg should deserialize")
}

/// A devDependency value-imported from a production file is flagged (promote to
/// dependencies).
#[test]
fn dev_dep_flagged_when_value_imported_from_prod() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "yaml", false);
    let pkg = make_pkg(&[], &["yaml"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.iter().any(|d| d.package_name == "yaml"),
        "devDependency value-imported from production code should be flagged"
    );
}

/// A devDependency imported from production code only via `import type` is NOT
/// flagged: type imports are erased at build time.
#[test]
fn dev_dep_not_flagged_when_type_only() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "type-fest", true);
    let pkg = make_pkg(&[], &["type-fest"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "type-only production imports must not be flagged"
    );
}

/// A devDependency imported only from a test file is NOT flagged: that is the
/// demote-side `test-only-dependency` rule's domain, and a test-only devDep is
/// already correctly placed.
#[test]
fn dev_dep_not_flagged_when_imported_only_from_test_file() {
    let (graph, _) = graph_with_import_from("/project/src/app.test.ts", "vitest", false);
    let pkg = make_pkg(&[], &["vitest"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "a devDependency imported only from test files must not be flagged"
    );
}

/// A production dependency (not a devDependency) is out of scope for this rule.
#[test]
fn prod_dependency_is_out_of_scope() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "yaml", false);
    let pkg = make_pkg(&["yaml"], &[], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "packages already in dependencies are not this rule's concern"
    );
}

/// `ignoreDependencies` suppresses the finding.
#[test]
fn dev_dep_skips_ignored_deps() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "yaml", false);
    let pkg = make_pkg(&[], &["yaml"], &[]);
    let config = FallowConfig {
        ignore_dependencies: vec!["yaml".to_string()],
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

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(flagged.is_empty(), "ignored deps must not be flagged");
}

/// A workspace package is never treated as an external dependency to promote.
#[test]
fn dev_dep_skips_workspace_packages() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "@myorg/ui", false);
    let pkg = make_pkg(&[], &["@myorg/ui"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/ui"),
        name: "@myorg/ui".to_string(),
        is_internal_dependency: false,
    }];

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &workspaces);

    assert!(
        flagged.is_empty(),
        "workspace packages must not be flagged as dev-dependency-in-production"
    );
}

/// Known dev tooling (`@types/*`, `typescript`, ...) is never promoted, matching
/// how the unused-dev-dependency detector treats it.
#[test]
fn dev_dep_skips_known_tooling() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "@types/node", false);
    let pkg = make_pkg(&[], &["@types/node"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "known tooling packages (e.g. @types/*) must not be flagged"
    );
}

/// A package listed in both `devDependencies` and `peerDependencies` is provided
/// at runtime by the peer, so it is not flagged.
#[test]
fn dev_dep_not_flagged_when_also_peer_dependency() {
    let (graph, _) = graph_with_import_from("/project/src/index.ts", "react", false);
    let pkg = pkg_with_peer(&["react"], &["react"]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "a dev+peer dependency is provided at runtime by the peer and must not be flagged"
    );
}

/// An import from a file OWNED BY a workspace package does not flag a root
/// devDependency: the workspace's own manifest governs that file's runtime
/// resolution (a package hoisted into root devDependencies but declared in the
/// workspace's dependencies would otherwise be a false positive).
#[test]
fn dev_dep_not_flagged_when_import_is_from_workspace_owned_file() {
    let (graph, _) = graph_with_import_from("/project/packages/app/src/index.ts", "lodash", false);
    let pkg = make_pkg(&[], &["lodash"], &[]);
    let config = test_config(PathBuf::from("/project"));
    let workspaces = vec![WorkspaceInfo {
        root: PathBuf::from("/project/packages/app"),
        name: "@myorg/app".to_string(),
        is_internal_dependency: false,
    }];

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &workspaces);

    assert!(
        flagged.is_empty(),
        "imports from workspace-owned files must not flag a root devDependency"
    );
}

/// An import from a file that is NOT reachable from any entry point (repo
/// tooling like `scripts/`, `benchmarks/`, or an orphaned module) is not
/// production evidence: the file is not part of the shipped artifact, and a
/// file fallow itself reports as unused cannot prove a dependency is needed at
/// runtime.
#[test]
fn dev_dep_not_flagged_when_importing_file_is_unreachable() {
    let (graph, _) = {
        let file_path = "/project/scripts/release.ts";
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from(file_path),
            size_bytes: 100,
        }];
        // No entry points: nothing is reachable.
        let entry_points = vec![];
        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from(file_path),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "enquirer".to_string(),
                    imported_name: ImportedName::Named("prompt".to_string()),
                    local_name: "prompt".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::NpmPackage("enquirer".to_string()),
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
        (graph, resolved_modules)
    };
    let pkg = make_pkg(&[], &["enquirer"], &[]);
    let config = test_config(PathBuf::from("/project"));

    let flagged = find_dev_dependencies_in_production(&graph, &pkg, &config, &[]);

    assert!(
        flagged.is_empty(),
        "imports from runtime-unreachable files must not flag a devDependency"
    );
}
