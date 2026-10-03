use super::helpers::*;
use crate::analyze::unused_deps::{UnusedCategoryInput, find_unprovided_import_location};

#[test]
fn collect_unused_empty_deps_returns_empty() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: true,
        plugin_tooling_needs_evidence: false,
    };
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: vec![],
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert!(result.is_empty());
}

#[test]
fn collect_unused_all_used_returns_empty() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["react".to_string(), "lodash".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| true,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert!(result.is_empty());
}

#[test]
fn collect_unused_some_unused_are_flagged() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec![
        "react".to_string(),
        "lodash".to_string(),
        "axios".to_string(),
    ];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|dep| dep == "react",
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/project/package.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|d| d.package_name == "lodash"));
    assert!(result.iter().any(|d| d.package_name == "axios"));
    assert!(
        result
            .iter()
            .all(|d| matches!(d.location, DependencyLocation::DevDependencies))
    );
}

#[test]
fn collect_unused_implicit_filter_skips_react_dom() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["react-dom".to_string(), "lodash".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "lodash");
}

#[test]
fn collect_unused_implicit_filter_disabled_keeps_react_dom() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["react-dom".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "react-dom");
}

#[test]
fn collect_unused_known_tooling_filter_skips_jest() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: true,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["jest".to_string(), "my-lib".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "my-lib");
}

#[test]
fn collect_unused_plugin_tooling_filter() {
    let (pr, su, id) = (
        FxHashSet::default(),
        FxHashSet::default(),
        fallow_config::IgnoreDependencyMatcher::default(),
    );
    let mut pt: FxHashSet<&str> = FxHashSet::default();
    pt.insert("my-runtime");
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: true,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["my-runtime".to_string(), "other".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "other");
}

#[test]
fn collect_unused_plugin_tooling_disabled_keeps_dep() {
    let (pr, su, id) = (
        FxHashSet::default(),
        FxHashSet::default(),
        fallow_config::IgnoreDependencyMatcher::default(),
    );
    let mut pt: FxHashSet<&str> = FxHashSet::default();
    pt.insert("my-runtime");
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        package_plugin_referenced: &pr,
        plugin_tooling: &pt,
        credited_plugin_tooling: &pt,
        declared_packages: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::OptionalDependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: false,
        plugin_tooling_needs_evidence: false,
    };
    let deps = vec!["my-runtime".to_string()];
    let result = collect_unused_for_category(UnusedCategoryInput {
        dep_names: deps,
        category: &category,
        shared: &shared,
        is_used: &|_| false,
        used_in_workspaces: &|_| Vec::new(),
        pkg_path: Path::new("/pkg.json"),
        pkg_content: None,
    });
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "my-runtime");
}

fn ws_deps(root: &str, deps: &[&str], is_private: bool) -> super::super::WorkspaceDependencies {
    super::super::WorkspaceDependencies {
        root: PathBuf::from(root),
        deps: deps.iter().map(|dep| (*dep).to_string()).collect(),
        is_private,
    }
}

/// `file_is_production` stands in for the module classification: a
/// production file of a publishable workspace keeps the strict check.
fn is_package_listed_for_file_as(
    file_path: &Path,
    package_name: &str,
    root_deps: &FxHashSet<String>,
    ws_dep_map: &[super::super::WorkspaceDependencies],
    file_is_production: bool,
) -> bool {
    let (mut graph, _) = build_graph_with_npm_imports(&[(package_name, false)]);
    graph.modules[0].path = file_path.to_path_buf();
    let roots: Vec<&Path> = ws_dep_map.iter().map(|ws| ws.root.as_path()).collect();
    let ownership = super::super::WorkspaceOwnershipIndex::new(&graph, &roots);
    super::super::manifest_chain_declares(
        package_name,
        FileId(0),
        ws_dep_map,
        &ownership,
        root_deps,
        |owner_is_private| owner_is_private || !file_is_production,
    )
}

fn is_package_listed_for_file(
    file_path: &Path,
    package_name: &str,
    root_deps: &FxHashSet<String>,
    ws_dep_map: &[super::super::WorkspaceDependencies],
) -> bool {
    is_package_listed_for_file_as(file_path, package_name, root_deps, ws_dep_map, true)
}

fn names(list: &[&str]) -> FxHashSet<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn listed_in_root_deps() {
    assert!(is_package_listed_for_file(
        Path::new("/project/src/index.ts"),
        "react",
        &names(&["react"]),
        &[],
    ));
}

#[test]
fn production_file_of_publishable_workspace_does_not_inherit_root_deps() {
    let ws_dep_map = vec![ws_deps("/project/packages/app", &[], false)];

    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "react",
        &names(&["react"]),
        &ws_dep_map,
    ));
}

#[test]
fn private_workspace_file_inherits_root_deps() {
    let ws_dep_map = vec![ws_deps("/project/packages/app", &[], true)];

    assert!(is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "react",
        &names(&["react"]),
        &ws_dep_map,
    ));
}

#[test]
fn non_production_file_inherits_nearest_ancestor_declaration() {
    let ws_dep_map = vec![
        ws_deps("/project/packages/app", &["react"], false),
        ws_deps("/project/packages/app/plugins/widget", &[], false),
    ];
    let file = Path::new("/project/packages/app/plugins/widget/scripts/build.mjs");

    assert!(is_package_listed_for_file_as(
        file,
        "react",
        &FxHashSet::default(),
        &ws_dep_map,
        false,
    ));
    assert!(is_package_listed_for_file_as(
        file,
        "vue",
        &names(&["vue"]),
        &ws_dep_map,
        false,
    ));
    assert!(!is_package_listed_for_file_as(
        file,
        "axios",
        &names(&["vue"]),
        &ws_dep_map,
        false,
    ));
    assert!(!is_package_listed_for_file_as(
        file,
        "react",
        &FxHashSet::default(),
        &ws_dep_map,
        true,
    ));
}

#[test]
fn listed_in_workspace_deps() {
    let ws_dep_map = vec![ws_deps("/project/packages/app", &["lodash"], false)];
    assert!(is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "lodash",
        &FxHashSet::default(),
        &ws_dep_map,
    ));
}

#[test]
fn not_listed_anywhere() {
    assert!(!is_package_listed_for_file(
        Path::new("/project/src/index.ts"),
        "axios",
        &FxHashSet::default(),
        &[],
    ));
}

#[test]
fn listed_in_different_workspace_not_matching() {
    let ws_dep_map = vec![
        ws_deps("/project/packages/app", &[], true),
        ws_deps("/project/packages/lib", &["lodash"], false),
    ];
    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "lodash",
        &FxHashSet::default(),
        &ws_dep_map,
    ));
}

#[test]
fn nested_workspace_uses_most_specific_manifest() {
    let ws_dep_map = vec![
        ws_deps("/project/packages/app", &["react"], false),
        ws_deps("/project/packages/app/plugins/widget", &["vue"], false),
    ];

    assert!(is_package_listed_for_file(
        Path::new("/project/packages/app/plugins/widget/src/index.ts"),
        "vue",
        &FxHashSet::default(),
        &ws_dep_map,
    ));
    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/plugins/widget/src/index.ts"),
        "react",
        &FxHashSet::default(),
        &ws_dep_map,
    ));
}

#[test]
fn workspace_ownership_uses_most_specific_ancestor() {
    let roots = [
        Path::new("/project/packages/app"),
        Path::new("/project/packages/app/plugins/widget"),
    ];
    let (mut graph, _) = build_graph_with_npm_imports(&[]);
    for (path, expected) in [
        ("/project/packages/app/plugins/widget/src/index.ts", Some(1)),
        ("/project/packages/app/src/index.ts", Some(0)),
        ("/project/packages/application/src/index.ts", None),
        ("/project/src/index.ts", None),
    ] {
        graph.modules[0].path = PathBuf::from(path);
        let ownership = super::super::WorkspaceOwnershipIndex::new(&graph, &roots);
        assert_eq!(ownership.workspace_index_for_file(FileId(0)), expected);
        assert_eq!(ownership.workspace_index_for_file(FileId(1)), None);
        assert_eq!(ownership.ancestors_of(0), &[] as &[usize]);
        assert_eq!(ownership.ancestors_of(1), &[0]);
    }
}

#[test]
fn import_location_found() {
    let mut spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    spans.insert(
        FileId(0),
        vec![("react", "react", 10), ("lodash", "lodash", 50)],
    );
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let location = find_unprovided_import_location(
        &spans,
        &line_offsets,
        &[],
        "src/a.ts",
        FileId(0),
        "lodash",
    );
    assert_eq!(location, Some((1, 50)));
}

#[test]
fn import_location_none_when_file_has_no_spans() {
    let spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let location =
        find_unprovided_import_location(&spans, &line_offsets, &[], "src/a.ts", FileId(0), "axios");
    assert_eq!(location, None);
}

#[test]
fn import_location_none_when_package_not_imported() {
    let mut spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    spans.insert(FileId(0), vec![("react", "react", 10)]);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let location = find_unprovided_import_location(
        &spans,
        &line_offsets,
        &[],
        "src/a.ts",
        FileId(0),
        "lodash",
    );
    assert_eq!(location, None);
}

#[test]
fn import_location_prefers_package_import_over_builtin() {
    // `node:test` and the `test` package share the name `test`. The finding
    // must point at the package import, not at the builtin import.
    let mut spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    spans.insert(
        FileId(0),
        vec![("test", "node:test", 10), ("test", "test", 50)],
    );
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let location =
        find_unprovided_import_location(&spans, &line_offsets, &[], "src/a.ts", FileId(0), "test");
    assert_eq!(location, Some((1, 50)));
}

fn manifest<'a>(
    root: &'a str,
    declared: &[&str],
    is_private: bool,
) -> super::super::WorkspaceManifest<'a> {
    let declared: FxHashSet<String> = declared.iter().map(|dep| (*dep).to_string()).collect();
    super::super::WorkspaceManifest {
        root: Path::new(root),
        name: root.to_string(),
        is_private,
        shipped: declared.clone(),
        installed: declared.clone(),
        declared,
        externalizes_packages: false,
    }
}

/// The graph helper makes its one file a runtime entry point, so the file is
/// production code unless its path matches a test pattern.
fn ancestor_root_for(
    file: &str,
    package_name: &str,
    manifests: &[super::super::WorkspaceManifest<'_>],
) -> Option<PathBuf> {
    let (mut graph, _) = build_graph_with_npm_imports(&[(package_name, false)]);
    graph.modules[0].path = PathBuf::from(file);
    let roots: Vec<&Path> = manifests.iter().map(|manifest| manifest.root).collect();
    let ownership = super::super::WorkspaceOwnershipIndex::new(&graph, &roots);
    let config = test_config(PathBuf::from("/project"));
    super::super::ancestor_satisfying_import(
        &graph,
        &config,
        manifests,
        &ownership,
        package_name,
        FileId(0),
    )
    .map(|ancestor| ancestor.root.to_path_buf())
}

#[test]
fn ancestor_declaration_satisfies_non_production_descendant_import() {
    let manifests = [
        manifest("/project/apps/tool", &["build-kit"], false),
        manifest("/project/apps/tool/packages/cli", &[], false),
    ];

    assert_eq!(
        ancestor_root_for(
            "/project/apps/tool/packages/cli/src/build.test.ts",
            "build-kit",
            &manifests
        ),
        Some(PathBuf::from("/project/apps/tool")),
        "a test file of the nested workspace uses the ancestor declaration"
    );
    assert_eq!(
        ancestor_root_for(
            "/project/apps/tool/packages/cli/src/index.ts",
            "build-kit",
            &manifests
        ),
        None,
        "a production file of a publishable nested workspace does not"
    );
}

#[test]
fn ancestor_declaration_satisfies_private_descendant_import() {
    let manifests = [
        manifest("/project/apps/tool", &["build-kit"], false),
        manifest("/project/apps/tool/packages/cli", &[], true),
    ];

    assert_eq!(
        ancestor_root_for(
            "/project/apps/tool/packages/cli/src/index.ts",
            "build-kit",
            &manifests
        ),
        Some(PathBuf::from("/project/apps/tool")),
    );
}

#[test]
fn own_declaration_is_not_attributed_to_an_ancestor() {
    let manifests = [
        manifest("/project/apps/tool", &["build-kit"], false),
        manifest("/project/apps/tool/packages/cli", &["build-kit"], true),
    ];

    assert_eq!(
        ancestor_root_for(
            "/project/apps/tool/packages/cli/src/index.ts",
            "build-kit",
            &manifests
        ),
        None,
    );
}

fn chain_installs(
    file: &str,
    package_name: &str,
    manifests: &[super::super::WorkspaceManifest<'_>],
) -> bool {
    let (mut graph, _) = build_graph_with_npm_imports(&[(package_name, false)]);
    graph.modules[0].path = PathBuf::from(file);
    let roots: Vec<&Path> = manifests.iter().map(|manifest| manifest.root).collect();
    let ownership = super::super::WorkspaceOwnershipIndex::new(&graph, &roots);
    super::super::workspace_chain_installs(manifests, &ownership, package_name, FileId(0))
}

#[test]
fn workspace_chain_installs_through_owner_or_ancestor() {
    let manifests = [
        manifest("/project/apps/tool", &["tool-lib"], false),
        manifest("/project/apps/tool/packages/cli", &["cli-lib"], false),
    ];
    let file = "/project/apps/tool/packages/cli/src/index.ts";

    assert!(chain_installs(file, "cli-lib", &manifests));
    assert!(chain_installs(file, "tool-lib", &manifests));
    assert!(!chain_installs(file, "root-lib", &manifests));
    assert!(
        !chain_installs("/project/src/index.ts", "tool-lib", &manifests),
        "a file outside every workspace is attributed to the root"
    );
}

#[test]
fn peer_only_declaration_does_not_install() {
    let mut app = manifest("/project/packages/app", &["react"], false);
    app.installed.clear();

    assert!(!chain_installs(
        "/project/packages/app/src/index.ts",
        "react",
        &[app]
    ));
}
