/// Public API entries of a package with `exports` set to `./lib/index.mjs`
/// and a graph that holds only `src/index.ts`.
fn lib_export_public_entries(with_lib_on_disk: bool) -> Vec<std::path::PathBuf> {
    use fallow_types::discover::{DiscoveredFile, FileId};

    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = dunce::canonicalize(directory.path()).expect("canonical root");
    std::fs::create_dir_all(root.join("src")).expect("src directory");
    std::fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("source entry");
    if with_lib_on_disk {
        std::fs::create_dir_all(root.join("lib")).expect("lib directory");
        std::fs::write(root.join("lib/index.mjs"), "export const value = 1;\n")
            .expect("build output outside the file set");
    }
    let package_json: fallow_config::PackageJson =
        serde_json::from_str(r#"{"name":"lib-output","exports":{".":"./lib/index.mjs"}}"#)
            .expect("package manifest");

    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: root.join("src/index.ts"),
        size_bytes: 0,
    }];
    let resolved_modules = vec![crate::resolve::ResolvedModule {
        file_id: FileId(0),
        path: files[0].path.clone(),
        ..Default::default()
    }];
    let graph = crate::graph::ModuleGraph::build(&resolved_modules, &[], &files);
    let path_to_file_id = super::graph_file_ids_by_path(&graph);

    let mut entries = rustc_hash::FxHashSet::default();
    super::add_package_public_api_entry_points(
        &mut entries,
        &graph,
        &path_to_file_id,
        &root,
        &package_json,
        &root,
    );
    let mut paths: Vec<std::path::PathBuf> = entries
        .into_iter()
        .map(|file_id| graph.modules[file_id.0 as usize].path.clone())
        .collect();
    paths.sort();
    paths
}

#[test]
fn lib_public_entry_outside_the_graph_maps_to_source() {
    let missing = lib_export_public_entries(false);
    assert_eq!(
        missing.len(),
        1,
        "a missing lib/ entry maps to src/: {missing:?}"
    );
    assert_eq!(
        lib_export_public_entries(true)
            .iter()
            .map(|path| path.ends_with("src/index.ts"))
            .collect::<Vec<_>>(),
        vec![true],
        "a lib/ entry on disk but outside the graph must map to src/"
    );
}

#[test]
fn exact_framework_contract_only_replaces_its_matching_plugin_rule() {
    use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
    use fallow_types::semantic::{SemanticFrameworkContract, SemanticFrameworkRelation};

    let contract = SemanticFrameworkContract {
        framework: "lit".to_string(),
        package: "lit".to_string(),
        heritage_symbol: "LitElement".to_string(),
        heritage_names: vec!["LitElement".to_string()],
        relation: SemanticFrameworkRelation::Extends,
        members: vec!["render".to_string()],
    };
    let matching = UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("LitElement".to_string()),
        implements: None,
        members: vec!["render".to_string()],
    });
    let local_name_only = UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("LocalLitElement".to_string()),
        implements: None,
        members: vec!["render".to_string()],
    });
    let extra_member = UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some("LitElement".to_string()),
        implements: None,
        members: vec!["render".to_string(), "localHook".to_string()],
    });

    assert!(super::framework_contract_covers_rule(&contract, &matching));
    assert!(!super::framework_contract_covers_rule(
        &contract,
        &local_name_only
    ));
    assert!(!super::framework_contract_covers_rule(
        &contract,
        &extra_member
    ));
}

#[test]
fn byte_offset_via_map_fallback() {
    use super::*;
    let map: LineOffsetsMap<'_> = FxHashMap::default();
    assert_eq!(
        super::byte_offset_to_line_col(&map, FileId(99), 42),
        (1, 42)
    );
}

#[test]
fn byte_offset_via_map_lookup() {
    use super::*;
    let offsets = fallow_types::extract::compute_line_offsets("abc\ndef\nghi");
    let mut map: LineOffsetsMap<'_> = FxHashMap::default();
    map.insert(FileId(0), &offsets);
    assert_eq!(super::byte_offset_to_line_col(&map, FileId(0), 5), (2, 1));
}

mod orchestration {
    use super::super::*;
    use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};
    use std::path::PathBuf;

    fn find_dead_code(graph: &ModuleGraph, config: &ResolvedConfig) -> AnalysisResults {
        find_dead_code_full(graph, config, &[], None, &[], &[], false)
    }

    fn make_config_with_rules(rules: RulesConfig) -> ResolvedConfig {
        FallowConfig {
            rules,
            ..Default::default()
        }
        .resolve(
            PathBuf::from("/tmp/orchestration-test"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    const ALL_RULES_OFF: RulesConfig = RulesConfig {
        unused_files: Severity::Off,
        unused_exports: Severity::Off,
        unused_types: Severity::Off,
        private_type_leaks: Severity::Off,
        private_type_leaks_configured: false,
        deprecated_exports_in_use: Severity::Off,
        unused_dependencies: Severity::Off,
        unused_dev_dependencies: Severity::Off,
        unused_optional_dependencies: Severity::Off,
        unused_enum_members: Severity::Off,
        unused_class_members: Severity::Off,
        unused_store_members: Severity::Off,
        unprovided_injects: Severity::Off,
        unrendered_components: Severity::Off,
        unused_component_props: Severity::Off,
        absent_component_props: Severity::Off,
        unused_component_emits: Severity::Off,
        unused_component_inputs: Severity::Off,
        unused_component_outputs: Severity::Off,
        unused_svelte_events: Severity::Off,
        unused_effect_layers: Severity::Off,
        unused_tagged_errors: Severity::Off,
        unproduced_catch_tags: Severity::Off,
        unused_server_actions: Severity::Off,
        unused_load_data_keys: Severity::Off,
        prop_drilling: Severity::Off,
        thin_wrapper: Severity::Off,
        duplicate_prop_shape: Severity::Off,
        css_token_drift: Severity::Off,
        css_duplicate_block: Severity::Off,
        css_selector_complexity: Severity::Off,
        css_dead_surface: Severity::Off,
        css_broken_reference: Severity::Off,
        complexity_cyclomatic: Severity::Off,
        complexity_cognitive: Severity::Off,
        complexity_crap: Severity::Off,
        complexity_file_lines: Severity::Off,
        unresolved_imports: Severity::Off,
        unlisted_dependencies: Severity::Off,
        duplicate_exports: Severity::Off,
        type_only_dependencies: Severity::Off,
        circular_dependencies: Severity::Off,
        re_export_cycle: Severity::Off,
        package_cycle: Severity::Off,
        test_only_dependencies: Severity::Off,
        dev_dependencies_in_production: Severity::Off,
        boundary_violation: Severity::Off,
        coverage_gaps: Severity::Off,
        feature_flags: Severity::Off,
        stale_suppressions: Severity::Off,
        require_suppression_reason: Severity::Off,
        unused_catalog_entries: Severity::Off,
        empty_catalog_groups: Severity::Off,
        unresolved_catalog_references: Severity::Off,
        unused_dependency_overrides: Severity::Off,
        misconfigured_dependency_overrides: Severity::Off,
        security_client_server_leak: Severity::Off,
        security_sink: Severity::Off,
        policy_violation: Severity::Off,
        invalid_client_export: Severity::Off,
        mixed_client_server_barrel: Severity::Off,
        misplaced_directive: Severity::Off,
        route_collision: Severity::Off,
        dynamic_segment_name_conflict: Severity::Off,
        weakened_tests: Severity::Off,
        added_suppression: Severity::Off,
        test_only_branch: Severity::Off,
        diverged_clone_sibling: Severity::Off,
        added_dependency: Severity::Off,
    };

    /// Build a two-module graph: an entry `index.ts` and a reachable
    /// `lib.ts` with one export that no module imports.
    fn graph_with_unused_export() -> ModuleGraph {
        use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
        use crate::extract::{ExportName, VisibilityTag};
        use crate::graph::ExportSymbol;
        use crate::resolve::ResolvedModule;
        use oxc_span::Span;
        use rustc_hash::FxHashSet;

        let paths = [
            PathBuf::from("/tmp/orchestration-test/src/index.ts"),
            PathBuf::from("/tmp/orchestration-test/src/lib.ts"),
        ];
        let files: Vec<DiscoveredFile> = paths
            .iter()
            .enumerate()
            .map(|(idx, path)| DiscoveredFile {
                id: FileId(u32::try_from(idx).unwrap()),
                path: path.clone(),
                size_bytes: 100,
            })
            .collect();
        let entry_points = vec![EntryPoint {
            path: paths[0].clone(),
            source: EntryPointSource::ManualEntry,
        }];
        let resolved: Vec<ResolvedModule> = files
            .iter()
            .map(|file| ResolvedModule {
                file_id: file.id,
                path: file.path.clone(),
                exports: vec![].into(),
                re_exports: vec![],
                resolved_imports: vec![],
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
        let mut graph = ModuleGraph::build(&resolved, &entry_points, &files);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("unusedHelper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(10, 30),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        }];
        graph
    }

    #[test]
    fn find_dead_code_all_rules_off_drops_findings_default_rules_report() {
        let graph = graph_with_unused_export();

        let default_results =
            find_dead_code(&graph, &make_config_with_rules(RulesConfig::default()));
        let reported: Vec<String> = default_results
            .unused_exports
            .iter()
            .map(|finding| finding.export.export_name.clone())
            .collect();
        assert_eq!(reported, vec!["unusedHelper".to_string()]);

        let results = find_dead_code(&graph, &make_config_with_rules(ALL_RULES_OFF));

        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.unused_dev_dependencies.is_empty());
        assert!(results.unused_optional_dependencies.is_empty());
        assert!(results.unused_enum_members.is_empty());
        assert!(results.unused_class_members.is_empty());
        assert!(results.unresolved_imports.is_empty());
        assert!(results.unlisted_dependencies.is_empty());
        assert!(results.duplicate_exports.is_empty());
        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn find_dead_code_full_collect_usages_flag() {
        use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
        use crate::extract::{ExportName, VisibilityTag};
        use crate::graph::{ExportSymbol, ModuleGraph};
        use crate::resolve::ResolvedModule;
        use oxc_span::Span;
        use rustc_hash::FxHashSet;

        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
            size_bytes: 100,
        }];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
            source: EntryPointSource::ManualEntry,
        }];
        let resolved = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![],
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
        let mut graph = ModuleGraph::build(&resolved, &entry_points, &files);
        graph.modules[0].exports = vec![ExportSymbol {
            name: ExportName::Named("myExport".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: Span::new(10, 30),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        }];

        let rules = RulesConfig::default();
        let config = make_config_with_rules(rules);

        let results_no_collect = find_dead_code_full(
            &graph,
            &config,
            &[],
            None,
            &[],
            &[],
            false, // collect_usages = false
        );
        assert!(
            results_no_collect.export_usages.is_empty(),
            "export_usages should be empty when collect_usages is false"
        );

        let results_with_collect = find_dead_code_full(
            &graph,
            &config,
            &[],
            None,
            &[],
            &[],
            true, // collect_usages = true
        );
        assert!(
            !results_with_collect.export_usages.is_empty(),
            "export_usages should be populated when collect_usages is true"
        );
        assert_eq!(
            results_with_collect.export_usages[0].export_name,
            "myExport"
        );
    }

    #[test]
    fn suppressions_built_from_modules() {
        use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
        use crate::extract::ModuleInfo;
        use crate::graph::ModuleGraph;
        use crate::resolve::ResolvedModule;
        use crate::suppress::{IssueKind, Suppression};
        use rustc_hash::FxHashSet;

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/entry.ts"),
                size_bytes: 100,
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/tmp/orchestration-test/src/utils.ts"),
                size_bytes: 100,
            },
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/tmp/orchestration-test/src/entry.ts"),
            source: EntryPointSource::ManualEntry,
        }];
        let resolved = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                exports: vec![].into(),
                re_exports: vec![],
                resolved_imports: vec![],
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
            .collect::<Vec<_>>();
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let modules = vec![ModuleInfo {
            suppressions: vec![Suppression::issue(0, 1, IssueKind::UnusedFile)],
            ..ModuleInfo::empty(FileId(1))
        }];

        let rules = RulesConfig {
            unused_files: Severity::Error,
            ..RulesConfig::default()
        };
        let config = make_config_with_rules(rules);

        let results = find_dead_code_full(&graph, &config, &[], None, &[], &modules, false);

        assert!(
            !results.unused_files.iter().any(|f| f
                .file
                .path
                .to_string_lossy()
                .contains("utils.ts")),
            "suppressed file should not appear in unused_files"
        );
    }
}
