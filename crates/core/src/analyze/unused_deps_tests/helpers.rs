#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use std::path::{Path, PathBuf};

#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use rustc_hash::{FxHashMap, FxHashSet};

#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use fallow_config::{
    BoundaryConfig, FallowConfig, OutputFormat, PackageJson, ResolvedConfig, WorkspaceInfo,
};
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use fallow_types::extract::{ImportInfo, ImportedName};

#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use crate::graph::ModuleGraph;
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use crate::plugins::AggregatedPluginResult;
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use crate::results::*;
#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use crate::suppress::{self, Suppression, SuppressionContext};

#[allow(unused_imports, reason = "shared re-export for sibling test modules")]
pub(super) use super::super::{
    DepCategoryConfig, LineOffsetsMap, SharedDepSets, UnlistedDependencyInput,
    collect_unused_for_category, find_dev_dependencies_in_production, find_import_location,
    find_test_only_dependencies, find_type_only_dependencies, find_unresolved_imports,
    find_unused_dependencies, is_package_listed_for_file, should_skip_dependency,
};

#[expect(
    clippy::too_many_arguments,
    reason = "test helper; thin wrapper mirroring the production signature for fixture setup"
)]
pub(super) fn find_unlisted_dependencies(
    graph: &ModuleGraph,
    pkg: &PackageJson,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    plugin_result: Option<&AggregatedPluginResult>,
    resolved_modules: &[ResolvedModule],
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnlistedDependency> {
    super::super::find_unlisted_dependencies(UnlistedDependencyInput {
        graph,
        pkg,
        config,
        workspaces,
        plugin_result,
        resolved_modules,
        line_offsets_by_file,
    })
}

/// Build a minimal ResolvedConfig for testing.
pub(super) fn test_config(root: PathBuf) -> ResolvedConfig {
    FallowConfig::default().resolve(root, OutputFormat::Human, 1, true, true, None)
}

/// Build a PackageJson with specific dependency fields via JSON deserialization.
/// This avoids directly constructing `std::collections::HashMap` (clippy disallowed type).
pub(super) fn make_pkg(deps: &[&str], dev_deps: &[&str], optional_deps: &[&str]) -> PackageJson {
    let to_obj = |names: &[&str]| -> serde_json::Value {
        let map: serde_json::Map<String, serde_json::Value> = names
            .iter()
            .map(|n| {
                (
                    n.to_string(),
                    serde_json::Value::String("^1.0.0".to_string()),
                )
            })
            .collect();
        serde_json::Value::Object(map)
    };

    let mut obj = serde_json::Map::new();
    obj.insert(
        "name".to_string(),
        serde_json::Value::String("test-project".to_string()),
    );
    if !deps.is_empty() {
        obj.insert("dependencies".to_string(), to_obj(deps));
    }
    if !dev_deps.is_empty() {
        obj.insert("devDependencies".to_string(), to_obj(dev_deps));
    }
    if !optional_deps.is_empty() {
        obj.insert("optionalDependencies".to_string(), to_obj(optional_deps));
    }
    serde_json::from_value(serde_json::Value::Object(obj))
        .expect("test PackageJson should deserialize")
}

/// Build a minimal graph where a single entry file imports given npm packages.
pub(super) fn build_graph_with_npm_imports(
    npm_packages: &[(&str, bool)], // (package_name, is_type_only)
) -> (ModuleGraph, Vec<ResolvedModule>) {
    let npm_imports: Vec<(&str, &str, bool)> = npm_packages
        .iter()
        .map(|(name, is_type_only)| (*name, *name, *is_type_only))
        .collect();
    build_graph_with_npm_import_sources(&npm_imports)
}

/// Build a minimal graph where a single entry file imports npm packages, allowing the original
/// source specifier to differ from the package name recorded by resolution.
#[expect(
    clippy::cast_possible_truncation,
    reason = "test span values are trivially small"
)]
pub(super) fn build_graph_with_npm_import_sources(
    npm_imports: &[(&str, &str, bool)], // (source, package_name, is_type_only)
) -> (ModuleGraph, Vec<ResolvedModule>) {
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        size_bytes: 100,
    }];

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/index.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    let resolved_imports: Vec<ResolvedImport> = npm_imports
        .iter()
        .enumerate()
        .map(|(i, (source, package_name, is_type_only))| ResolvedImport {
            info: ImportInfo {
                source: source.to_string(),
                imported_name: ImportedName::Named("default".to_string()),
                local_name: format!("import_{i}"),
                is_type_only: *is_type_only,
                from_style: false,
                span: oxc_span::Span::new((i * 20) as u32, (i * 20 + 15) as u32),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage(package_name.to_string()),
        })
        .collect();

    let resolved_modules = vec![ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/index.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports,
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
    }];

    let graph = ModuleGraph::build(&resolved_modules, &entry_points, &files);
    (graph, resolved_modules)
}

pub(super) type SkipDepSets = (
    FxHashSet<String>,
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
);

/// Helper: build empty sets for should_skip_dependency args.
pub(super) fn empty_sets() -> SkipDepSets {
    (
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
    )
}

pub(super) type SharedSets = (
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
    FxHashSet<&'static str>,
);

pub(super) fn empty_shared_sets() -> SharedSets {
    (
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
        FxHashSet::default(),
    )
}
