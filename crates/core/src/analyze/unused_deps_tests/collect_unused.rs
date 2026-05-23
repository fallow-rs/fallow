use super::helpers::*;

// ---- collect_unused_for_category unit tests ----

#[test]
fn collect_unused_empty_deps_returns_empty() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: true,
    };
    let result = collect_unused_for_category(
        vec![],
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert!(result.is_empty());
}

#[test]
fn collect_unused_all_used_returns_empty() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
    };
    let deps = vec!["react".to_string(), "lodash".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| true, // all used
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert!(result.is_empty());
}

#[test]
fn collect_unused_some_unused_are_flagged() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
    };
    let deps = vec![
        "react".to_string(),
        "lodash".to_string(),
        "axios".to_string(),
    ];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |dep| dep == "react", // only react is used
        |_| Vec::new(),
        Path::new("/project/package.json"),
        None,
    );
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
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    // With check_implicit = true, react-dom should be filtered out
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: false,
    };
    let deps = vec!["react-dom".to_string(), "lodash".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "lodash");
}

#[test]
fn collect_unused_implicit_filter_disabled_keeps_react_dom() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    // With check_implicit = false (dev deps), react-dom is NOT filtered
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: false,
    };
    let deps = vec!["react-dom".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "react-dom");
}

#[test]
fn collect_unused_known_tooling_filter_skips_jest() {
    let (pr, pt, su, id) = empty_shared_sets();
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    let category = DepCategoryConfig {
        location: DependencyLocation::DevDependencies,
        check_implicit: false,
        check_known_tooling: true,
        check_plugin_tooling: false,
    };
    let deps = vec!["jest".to_string(), "my-lib".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "my-lib");
}

#[test]
fn collect_unused_plugin_tooling_filter() {
    let (pr, su, id) = (
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
    );
    let mut pt: FxHashSet<&str> = FxHashSet::default();
    pt.insert("my-runtime");
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    // check_plugin_tooling = true should filter "my-runtime"
    let category = DepCategoryConfig {
        location: DependencyLocation::Dependencies,
        check_implicit: false,
        check_known_tooling: false,
        check_plugin_tooling: true,
    };
    let deps = vec!["my-runtime".to_string(), "other".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "other");
}

#[test]
fn collect_unused_plugin_tooling_disabled_keeps_dep() {
    let (pr, su, id) = (
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
    );
    let mut pt: FxHashSet<&str> = FxHashSet::default();
    pt.insert("my-runtime");
    let shared = SharedDepSets {
        plugin_referenced: &pr,
        plugin_tooling: &pt,
        script_used: &su,
        ignore_deps: &id,
    };
    // check_plugin_tooling = false (optional deps), "my-runtime" should NOT be filtered
    let category = DepCategoryConfig {
        location: DependencyLocation::OptionalDependencies,
        check_implicit: true,
        check_known_tooling: false,
        check_plugin_tooling: false,
    };
    let deps = vec!["my-runtime".to_string()];
    let result = collect_unused_for_category(
        deps,
        &category,
        &shared,
        |_| false,
        |_| Vec::new(),
        Path::new("/pkg.json"),
        None,
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].package_name, "my-runtime");
}

// ---- is_package_listed_for_file unit tests ----

#[test]
fn listed_in_root_deps() {
    let mut root_deps = FxHashSet::default();
    root_deps.insert("react".to_string());
    let ws_dep_map: Vec<(PathBuf, FxHashSet<String>)> = vec![];
    assert!(is_package_listed_for_file(
        Path::new("/project/src/index.ts"),
        "react",
        &root_deps,
        &ws_dep_map,
    ));
}

#[test]
fn workspace_file_does_not_inherit_root_deps() {
    let mut root_deps = FxHashSet::default();
    root_deps.insert("react".to_string());
    let ws_dep_map = vec![(PathBuf::from("/project/packages/app"), FxHashSet::default())];

    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "react",
        &root_deps,
        &ws_dep_map,
    ));
}

#[test]
fn listed_in_workspace_deps() {
    let root_deps = FxHashSet::default();
    let mut ws_deps = FxHashSet::default();
    ws_deps.insert("lodash".to_string());
    let ws_dep_map = vec![(PathBuf::from("/project/packages/app"), ws_deps)];
    // File inside the workspace
    assert!(is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "lodash",
        &root_deps,
        &ws_dep_map,
    ));
}

#[test]
fn not_listed_anywhere() {
    let root_deps = FxHashSet::default();
    let ws_dep_map: Vec<(PathBuf, FxHashSet<String>)> = vec![];
    assert!(!is_package_listed_for_file(
        Path::new("/project/src/index.ts"),
        "axios",
        &root_deps,
        &ws_dep_map,
    ));
}

#[test]
fn listed_in_different_workspace_not_matching() {
    let root_deps = FxHashSet::default();
    let mut ws_deps = FxHashSet::default();
    ws_deps.insert("lodash".to_string());
    let ws_dep_map = vec![(PathBuf::from("/project/packages/lib"), ws_deps)];
    // File in a different workspace
    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/src/index.ts"),
        "lodash",
        &root_deps,
        &ws_dep_map,
    ));
}

#[test]
fn nested_workspace_uses_most_specific_manifest() {
    let root_deps = FxHashSet::default();
    let mut parent_deps = FxHashSet::default();
    parent_deps.insert("react".to_string());
    let mut child_deps = FxHashSet::default();
    child_deps.insert("vue".to_string());
    let ws_dep_map = vec![
        (PathBuf::from("/project/packages/app"), parent_deps),
        (
            PathBuf::from("/project/packages/app/plugins/widget"),
            child_deps,
        ),
    ];

    assert!(is_package_listed_for_file(
        Path::new("/project/packages/app/plugins/widget/src/index.ts"),
        "vue",
        &root_deps,
        &ws_dep_map,
    ));
    assert!(!is_package_listed_for_file(
        Path::new("/project/packages/app/plugins/widget/src/index.ts"),
        "react",
        &root_deps,
        &ws_dep_map,
    ));
}

// ---- find_import_location unit tests ----

#[test]
fn import_location_found() {
    let mut spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    spans.insert(
        FileId(0),
        vec![("react", "react", 10), ("lodash", "lodash", 50)],
    );
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    // Without line offsets, falls back to (1, byte_offset) from byte_offset_to_line_col
    let (line, col) = find_import_location(&spans, &line_offsets, FileId(0), "lodash");
    assert_eq!(line, 1);
    assert_eq!(col, 50);
}

#[test]
fn import_location_not_found_falls_back() {
    let spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let (line, col) = find_import_location(&spans, &line_offsets, FileId(0), "axios");
    assert_eq!(line, 1);
    assert_eq!(col, 0);
}

#[test]
fn import_location_file_exists_but_package_not_found() {
    let mut spans: FxHashMap<FileId, Vec<(&str, &str, u32)>> = FxHashMap::default();
    spans.insert(FileId(0), vec![("react", "react", 10)]);
    let line_offsets: LineOffsetsMap<'_> = FxHashMap::default();
    let (line, col) = find_import_location(&spans, &line_offsets, FileId(0), "lodash");
    assert_eq!(line, 1);
    assert_eq!(col, 0);
}
