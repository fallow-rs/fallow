mod boundary;
mod boundary_calls;
mod boundary_coverage;
mod dynamic_segment_name_conflict;
pub mod feature_flags;
mod iconify;
mod invalid_client_exports;
mod misplaced_directive;
mod mixed_barrel;
mod package_json_utils;
mod policy;
mod predicates;
mod re_export_cycles;
mod route_collision;
mod route_tree;
mod security;
mod server_only;
mod unprovided_inject;
mod unrendered_component;
mod unused_catalog;
mod unused_component_emit;
mod unused_component_prop;
mod unused_deps;
mod unused_exports;
mod unused_files;
mod unused_members;
mod unused_overrides;
mod unused_server_action;

#[cfg(test)]
pub(crate) use unused_deps::matches_virtual_prefix;

/// Human-readable title for a security catalogue category id, for the CLI
/// renderer. Re-exported so the `fallow security` command can label a
/// `TaintedSink` finding without reaching into the private `security` module.
pub use security::catalogue_title as security_catalogue_title;
pub use security::derive_security_severity;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{PackageJson, ResolvedConfig, Severity};

use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use fallow_types::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding, DuplicateExportFinding, DynamicSegmentNameConflictFinding,
    EmptyCatalogGroupFinding, InvalidClientExportFinding, MisconfiguredDependencyOverrideFinding,
    MisplacedDirectiveFinding, MixedClientServerBarrelFinding, PolicyViolationFinding,
    PrivateTypeLeakFinding, ReExportCycleFinding, RouteCollisionFinding, TestOnlyDependencyFinding,
    TypeOnlyDependencyFinding, UnlistedDependencyFinding, UnprovidedInjectFinding,
    UnrenderedComponentFinding, UnresolvedCatalogReferenceFinding, UnresolvedImportFinding,
    UnusedCatalogEntryFinding, UnusedClassMemberFinding, UnusedComponentEmitFinding,
    UnusedComponentPropFinding, UnusedDependencyFinding, UnusedDependencyOverrideFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedOptionalDependencyFinding, UnusedStoreMemberFinding, UnusedTypeFinding,
};

use crate::results::{AnalysisResults, CircularDependency, CircularDependencyEdge};
use crate::suppress::{IssueKind, SuppressionContext};

use dynamic_segment_name_conflict::find_dynamic_segment_name_conflicts;
use invalid_client_exports::find_invalid_client_exports;
use misplaced_directive::find_misplaced_directives;
use mixed_barrel::find_mixed_client_server_barrels;
use re_export_cycles::find_re_export_cycles;
use route_collision::find_route_collisions;
use unprovided_inject::find_unprovided_injects;
use unrendered_component::find_unrendered_components;
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_catalog::{
    find_empty_catalog_groups, find_unresolved_catalog_references, find_unused_catalog_entries,
    gather_pnpm_catalog_state,
};
use unused_component_emit::find_unused_component_emits;
use unused_component_prop::find_unused_component_props;
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_deps::{
    find_test_only_dependencies, find_type_only_dependencies, find_unlisted_dependencies,
    find_unresolved_imports, find_unused_dependencies,
};
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_exports::{
    collect_export_usages, find_private_type_leaks, find_unused_exports,
    suppress_signature_backing_types,
};
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_files::find_unused_files;
use unused_members::find_unused_members_with_public_api_entry_points;
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_overrides::{
    find_misconfigured_dependency_overrides, find_unused_dependency_overrides,
    gather_pnpm_override_state,
};
use unused_server_action::reclassify_unused_server_actions;

/// Pre-computed line offset tables indexed by `FileId`, built during parse and
/// carried through the cache. Eliminates redundant file reads during analysis.
#[doc(hidden)]
pub type LineOffsetsMap<'a> = FxHashMap<FileId, &'a [u32]>;

struct SecurityDetectionContext<'a, 'm> {
    graph: &'a ModuleGraph,
    modules: &'a [ModuleInfo],
    config: &'a ResolvedConfig,
    suppressions: &'a crate::suppress::SuppressionContext<'m>,
    line_offsets_by_file: &'a LineOffsetsMap<'m>,
    declared_deps: &'a FxHashSet<String>,
    request_receivers: &'a FxHashSet<String>,
}

/// Convert a byte offset to (line, col) using pre-computed line offsets.
/// Falls back to `(1, byte_offset)` when no line table is available.
#[doc(hidden)]
pub fn byte_offset_to_line_col(
    line_offsets_map: &LineOffsetsMap<'_>,
    file_id: FileId,
    byte_offset: u32,
) -> (u32, u32) {
    line_offsets_map
        .get(&file_id)
        .map_or((1, byte_offset), |offsets| {
            fallow_types::extract::byte_offset_to_line_col(offsets, byte_offset)
        })
}

fn cycle_edge_line_col(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    cycle: &[FileId],
    edge_index: usize,
) -> Option<(u32, u32)> {
    if cycle.is_empty() {
        return None;
    }

    let from = cycle[edge_index];
    let to = cycle[(edge_index + 1) % cycle.len()];
    graph
        .find_import_span_start(from, to)
        .map(|span_start| byte_offset_to_line_col(line_offsets_map, from, span_start))
}

fn is_circular_dependency_suppressed(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    cycle: &[FileId],
) -> bool {
    if cycle
        .iter()
        .any(|&id| suppressions.is_file_suppressed(id, IssueKind::CircularDependency))
    {
        return true;
    }

    let mut line_suppressed = false;
    for edge_index in 0..cycle.len() {
        let from = cycle[edge_index];
        if let Some((line, _)) = cycle_edge_line_col(graph, line_offsets_map, cycle, edge_index)
            && suppressions.is_suppressed(from, line, IssueKind::CircularDependency)
        {
            line_suppressed = true;
        }
    }
    line_suppressed
}

/// Read source content from disk, returning empty string on failure.
/// Only used for LSP Code Lens reference resolution where the referencing
/// file may not be in the line offsets map.
fn read_source(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Check whether any two files in a cycle belong to different workspace packages.
/// Uses longest-prefix-match to assign each file to a workspace root.
/// Files outside all workspace roots (e.g., root-level shared code) are ignored —
/// only cycles between two distinct named workspaces are flagged.
fn is_cross_package_cycle(
    files: &[std::path::PathBuf],
    workspaces: &[fallow_config::WorkspaceInfo],
) -> bool {
    let find_workspace = |path: &std::path::Path| -> Option<&std::path::Path> {
        workspaces
            .iter()
            .map(|w| w.root.as_path())
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
    };

    let mut seen_workspace: Option<&std::path::Path> = None;
    for file in files {
        if let Some(ws) = find_workspace(file) {
            match &seen_workspace {
                None => seen_workspace = Some(ws),
                Some(prev) if *prev != ws => return true,
                _ => {}
            }
        }
    }
    false
}

fn public_workspace_roots<'a>(
    public_packages: &[String],
    workspaces: &'a [fallow_config::WorkspaceInfo],
) -> Vec<&'a std::path::Path> {
    if public_packages.is_empty() || workspaces.is_empty() {
        return Vec::new();
    }

    workspaces
        .iter()
        .filter(|ws| {
            public_packages.iter().any(|pattern| {
                ws.name == *pattern
                    || globset::Glob::new(pattern)
                        .ok()
                        .is_some_and(|g| g.compile_matcher().is_match(&ws.name))
            })
        })
        .map(|ws| ws.root.as_path())
        .collect()
}

fn graph_path_to_file_id(graph: &ModuleGraph) -> FxHashMap<std::path::PathBuf, FileId> {
    let mut path_to_file_id = FxHashMap::default();
    for module in &graph.modules {
        path_to_file_id.insert(module.path.clone(), module.file_id);
        if let Ok(canonical) = dunce::canonicalize(&module.path) {
            path_to_file_id.insert(canonical, module.file_id);
        }
    }
    path_to_file_id
}

fn add_package_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    path_to_file_id: &FxHashMap<std::path::PathBuf, FileId>,
    package_root: &std::path::Path,
    package_json: &PackageJson,
    canonical_project_root: &std::path::Path,
) {
    if package_json.private.unwrap_or(false) {
        return;
    }

    for entry in package_json.entry_points() {
        let Some(entry_point) = crate::discover::resolve_entry_path(
            package_root,
            &entry,
            canonical_project_root,
            crate::discover::EntryPointSource::PackageJsonExports,
        ) else {
            continue;
        };

        if let Some(file_id) = path_to_file_id.get(&entry_point.path).copied().or_else(|| {
            dunce::canonicalize(&entry_point.path)
                .ok()
                .and_then(|canonical| path_to_file_id.get(&canonical).copied())
        }) {
            public_api_entry_points.insert(file_id);
        }
    }
}

fn is_source_index_under_package(path: &std::path::Path, package_root: &std::path::Path) -> bool {
    let Ok(relative) = path.strip_prefix(package_root) else {
        return false;
    };

    if !matches!(
        relative.components().next(),
        Some(std::path::Component::Normal(segment)) if segment == "src"
    ) {
        return false;
    }

    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "index")
}

fn add_exportless_package_source_indexes(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &ModuleGraph,
    package_root: &std::path::Path,
    package_json: &PackageJson,
) {
    if package_json.private.unwrap_or(false) || package_json.exports.is_some() {
        return;
    }

    let mut roots = vec![package_root.to_path_buf()];
    if let Ok(canonical) = dunce::canonicalize(package_root) {
        roots.push(canonical);
    }

    for module in &graph.modules {
        if roots
            .iter()
            .any(|root| is_source_index_under_package(&module.path, root))
        {
            public_api_entry_points.insert(module.file_id);
        }
    }
}

fn public_api_package_entry_points(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> FxHashSet<FileId> {
    let mut public_api_entry_points = FxHashSet::default();
    let path_to_file_id = graph_path_to_file_id(graph);
    let canonical_project_root =
        dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());

    if let Some(pkg) = root_pkg {
        add_package_public_api_entry_points(
            &mut public_api_entry_points,
            &path_to_file_id,
            &config.root,
            pkg,
            &canonical_project_root,
        );
        add_exportless_package_source_indexes(
            &mut public_api_entry_points,
            graph,
            &config.root,
            pkg,
        );
    }

    for workspace in workspaces {
        let Ok(pkg) = PackageJson::load(&workspace.root.join("package.json")) else {
            continue;
        };
        add_package_public_api_entry_points(
            &mut public_api_entry_points,
            &path_to_file_id,
            &workspace.root,
            &pkg,
            &canonical_project_root,
        );
        add_exportless_package_source_indexes(
            &mut public_api_entry_points,
            graph,
            &workspace.root,
            &pkg,
        );
    }

    public_api_entry_points
}

fn find_circular_dependencies(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<CircularDependency> {
    let cycles = graph.find_cycles();
    let mut dependencies: Vec<CircularDependency> = cycles
        .into_iter()
        .filter_map(|cycle| {
            if is_circular_dependency_suppressed(graph, line_offsets_map, suppressions, &cycle) {
                return None;
            }

            // One anchor per hop in cycle order: `edges[i]` is the import in
            // `cycle[i]` pointing to `cycle[i + 1]`. Always populated for every
            // hop (fallback `(1, 0)` if the span is somehow missing) so
            // `edges.len() == files.len()` regardless of URL-resolvability on
            // the consumer side. The LSP renders one squiggly per edge.
            let edges: Vec<CircularDependencyEdge> = (0..cycle.len())
                .map(|edge_index| {
                    let from = cycle[edge_index];
                    let (line, col) =
                        cycle_edge_line_col(graph, line_offsets_map, &cycle, edge_index)
                            .unwrap_or((1, 0));
                    CircularDependencyEdge {
                        path: graph.modules[from.0 as usize].path.clone(),
                        line,
                        col,
                    }
                })
                .collect();

            let files: Vec<std::path::PathBuf> =
                edges.iter().map(|edge| edge.path.clone()).collect();
            let length = files.len();
            // Top-level `line`/`col` remain the first hop's anchor for
            // backward compatibility with consumers that predate `edges`.
            let (line, col) = edges.first().map_or((1, 0), |edge| (edge.line, edge.col));
            Some(CircularDependency {
                files,
                length,
                line,
                col,
                edges,
                is_cross_package: false,
            })
        })
        .collect();

    if !workspaces.is_empty() {
        for dep in &mut dependencies {
            dep.is_cross_package = is_cross_package_cycle(&dep.files, workspaces);
        }
    }

    dependencies
}

/// Thin wrapper around [`find_circular_dependencies`] that gates on
/// `Severity::Off` and wraps the bare results in typed envelopes.
/// Extracted from the rayon-join tree to keep nesting under the clippy
/// `excessive_nesting` threshold (7).
fn run_circular_dep_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<CircularDependencyFinding> {
    if config.rules.circular_dependencies == Severity::Off {
        return Vec::new();
    }
    find_circular_dependencies(graph, line_offsets_by_file, suppressions, workspaces)
        .into_iter()
        .map(CircularDependencyFinding::with_actions)
        .collect()
}

/// Thin wrapper around
/// [`boundary_coverage::find_boundary_coverage_violations`] that gates on the
/// shared `boundary-violation` severity. Extracted alongside
/// [`run_circular_dep_detector`].
fn run_boundary_coverage_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
) -> Vec<BoundaryCoverageViolationFinding> {
    if config.rules.boundary_violation == Severity::Off {
        return Vec::new();
    }
    boundary_coverage::find_boundary_coverage_violations(graph, config, suppressions)
        .into_iter()
        .map(BoundaryCoverageViolationFinding::with_actions)
        .collect()
}

/// Thin wrapper around [`boundary_calls::find_boundary_call_violations`] that
/// gates on the shared `boundary-violation` severity. Extracted alongside
/// [`run_circular_dep_detector`].
fn run_boundary_call_detector(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryCallViolationFinding> {
    if config.rules.boundary_violation == Severity::Off {
        return Vec::new();
    }
    boundary_calls::find_boundary_call_violations(
        graph,
        modules,
        config,
        suppressions,
        line_offsets_by_file,
    )
    .into_iter()
    .map(BoundaryCallViolationFinding::with_actions)
    .collect()
}

/// Thin wrapper around [`policy::find_policy_violations`] that gates on the
/// `policy-violation` master severity (a kill switch: per-rule severity
/// cannot resurrect it) and on at least one configured rule pack. Extracted
/// alongside [`run_circular_dep_detector`].
fn run_policy_detector(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<PolicyViolationFinding> {
    if config.rules.policy_violation == Severity::Off || config.rule_packs.is_empty() {
        return Vec::new();
    }
    policy::find_policy_violations(graph, modules, config, suppressions, line_offsets_by_file)
        .into_iter()
        .map(PolicyViolationFinding::with_actions)
        .collect()
}

/// Run the boundary-coverage, boundary-call, and rule-pack policy detectors
/// in parallel. Extracted so the main `find_dead_code_full` join tree stays
/// within the nesting budget.
fn run_boundary_aux_detectors(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> (
    Vec<BoundaryCoverageViolationFinding>,
    (
        Vec<BoundaryCallViolationFinding>,
        Vec<PolicyViolationFinding>,
    ),
) {
    rayon::join(
        || run_boundary_coverage_detector(graph, config, suppressions),
        || {
            rayon::join(
                || {
                    run_boundary_call_detector(
                        graph,
                        modules,
                        config,
                        suppressions,
                        line_offsets_by_file,
                    )
                },
                || run_policy_detector(graph, modules, config, suppressions, line_offsets_by_file),
            )
        },
    )
}

/// Thin wrapper around [`re_export_cycles::find_re_export_cycles`] that gates
/// on `Severity::Off`. Extracted alongside [`run_circular_dep_detector`].
fn run_re_export_cycle_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
) -> Vec<ReExportCycleFinding> {
    if config.rules.re_export_cycle == Severity::Off {
        return Vec::new();
    }
    find_re_export_cycles(graph, suppressions)
}

/// Collect export usage counts for Code Lens (LSP feature). Skipped in CLI
/// mode since the field is `#[serde(skip)]` in all output formats.
fn run_export_usages_collector(
    graph: &ModuleGraph,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    collect_usages: bool,
) -> Vec<crate::results::ExportUsage> {
    if collect_usages {
        collect_export_usages(graph, line_offsets_by_file)
    } else {
        Vec::new()
    }
}

/// Collect every package name declared across the root `package.json` and each
/// workspace `package.json`. This is the dependency universe the plugin system
/// activates on, reused by the framework-scoped security catalogue rows (#861) to
/// gate a row on the active framework. Missing or malformed manifests contribute
/// nothing (a framework row simply stays inert), matching the conservative
/// false-negatives-over-false-positives posture.
fn collect_declared_dependency_names(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> FxHashSet<String> {
    let mut deps: FxHashSet<String> = FxHashSet::default();
    if let Some(pkg) = root_pkg {
        deps.extend(pkg.all_dependency_names());
    }
    for ws in workspaces {
        if ws.root == config.root {
            continue; // already covered by root_pkg
        }
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            deps.extend(pkg.all_dependency_names());
        }
    }
    deps
}

/// Find all dead code, with optional resolved module data, plugin context, and workspace info.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: replacement returns serde_json::Value, not typed AnalysisResults. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn find_dead_code_full(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    resolved_modules: &[ResolvedModule],
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    workspaces: &[fallow_config::WorkspaceInfo],
    modules: &[ModuleInfo],
    collect_usages: bool,
) -> AnalysisResults {
    let _span = tracing::info_span!("find_dead_code").entered();

    let suppressions = crate::suppress::SuppressionContext::new(modules);

    let line_offsets_by_file: LineOffsetsMap<'_> = modules
        .iter()
        .filter(|m| !m.line_offsets.is_empty())
        .map(|m| (m.file_id, m.line_offsets.as_slice()))
        .collect();

    let pkg_path = config.root.join("package.json");
    let pkg = PackageJson::load(&pkg_path).ok();
    let public_api_entry_points =
        public_api_package_entry_points(graph, config, pkg.as_ref(), workspaces);

    let iconify_referenced =
        iconify::collect_iconify_referenced_deps(modules, pkg.as_ref(), workspaces);
    let augmented_plugin_result;
    let plugin_result = if iconify_referenced.is_empty() {
        plugin_result
    } else {
        let mut owned = plugin_result.cloned().unwrap_or_default();
        owned.referenced_dependencies.extend(iconify_referenced);
        augmented_plugin_result = owned;
        Some(&augmented_plugin_result)
    };

    let mut user_class_members = config.used_class_members.clone();
    if let Some(plugin_result) = plugin_result {
        user_class_members.extend(plugin_result.used_class_members.iter().cloned());
    }

    let virtual_prefixes: Vec<&str> = plugin_result
        .map(|pr| {
            pr.virtual_module_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_patterns: Vec<&str> = plugin_result
        .map(|pr| {
            pr.generated_import_patterns
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_type_prefixes: Vec<&str> = plugin_result
        .map(|pr| {
            pr.generated_type_import_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();

    let declared_deps = collect_declared_dependency_names(config, pkg.as_ref(), workspaces);

    let mut results = run_parallel_dead_code_detectors(DeadCodeDetectorInput {
        graph,
        config,
        resolved_modules,
        workspaces,
        modules,
        suppressions: &suppressions,
        line_offsets_by_file: &line_offsets_by_file,
        plugin_result,
        pkg: pkg.as_ref(),
        user_class_members: &user_class_members,
        public_api_entry_points: &public_api_entry_points,
        virtual_prefixes: &virtual_prefixes,
        generated_patterns: &generated_patterns,
        generated_type_prefixes: &generated_type_prefixes,
        declared_deps: &declared_deps,
        collect_usages,
    });

    filter_public_workspace_results(config, workspaces, &mut results);

    // Reclassify the server-action subset of unused exports BEFORE stale
    // detection so a `// fallow-ignore-next-line unused-server-action` marker is
    // recorded as consumed. Gate-off keeps the findings as plain unused-exports.
    if config.rules.unused_server_actions != Severity::Off {
        reclassify_unused_server_actions(
            graph,
            modules,
            &declared_deps,
            &suppressions,
            &mut results,
        );
    }

    let request_receivers = config
        .security
        .request_receivers
        .iter()
        .cloned()
        .collect::<FxHashSet<_>>();

    populate_security_findings(
        &SecurityDetectionContext {
            graph,
            modules,
            config,
            suppressions: &suppressions,
            line_offsets_by_file: &line_offsets_by_file,
            declared_deps: &declared_deps,
            request_receivers: &request_receivers,
        },
        &mut results,
    );

    if config.rules.stale_suppressions != Severity::Off {
        results
            .stale_suppressions
            .extend(suppressions.find_stale(graph, config));
    }
    results.suppression_count = suppressions.used_count();
    results.active_suppressions = suppressions.all_suppressions(graph);

    populate_pnpm_catalog_findings(config, workspaces, &mut results);
    populate_pnpm_override_findings(config, workspaces, &mut results);
    populate_framework_specific_findings(
        graph,
        modules,
        resolved_modules,
        config,
        workspaces,
        &declared_deps,
        &public_api_entry_points,
        &suppressions,
        &line_offsets_by_file,
        &mut results,
    );

    results.sort();

    results
}

/// Run the framework-convention detectors that share the resolved-graph and
/// dep-gate context: Next.js RSC directives, Vue/Svelte DI and components, and
/// the App Router route tree. Extracted from `find_dead_code_full` to keep that
/// orchestrator under the unit-size ceiling; each callee is individually
/// rule-gated.
#[expect(
    clippy::too_many_arguments,
    reason = "threads the shared resolved-graph + dep-gate context to a fixed set of framework detectors"
)]
fn populate_framework_specific_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    populate_invalid_client_export_findings(
        graph,
        modules,
        config,
        declared_deps,
        suppressions,
        line_offsets_by_file,
        results,
    );
    populate_mixed_client_server_barrel_findings(
        graph,
        modules,
        resolved_modules,
        config,
        declared_deps,
        suppressions,
        line_offsets_by_file,
        results,
    );
    populate_misplaced_directive_findings(
        graph,
        modules,
        config,
        declared_deps,
        suppressions,
        line_offsets_by_file,
        results,
    );
    populate_unprovided_inject_findings(
        graph,
        modules,
        resolved_modules,
        config,
        declared_deps,
        public_api_entry_points,
        suppressions,
        line_offsets_by_file,
        results,
    );
    populate_unrendered_component_findings(
        graph,
        modules,
        resolved_modules,
        config,
        declared_deps,
        public_api_entry_points,
        suppressions,
        results,
    );
    populate_unused_component_prop_findings(
        graph,
        modules,
        config,
        declared_deps,
        line_offsets_by_file,
        results,
    );
    populate_unused_component_emit_findings(
        graph,
        modules,
        config,
        declared_deps,
        line_offsets_by_file,
        results,
    );
    populate_nextjs_route_tree_findings(
        graph,
        config,
        workspaces,
        declared_deps,
        suppressions,
        results,
    );
}

/// Populate `invalid_client_exports` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_invalid_client_exports`]).
fn populate_invalid_client_export_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.invalid_client_export == Severity::Off {
        return;
    }
    results.invalid_client_exports = find_invalid_client_exports(
        graph,
        modules,
        declared_deps,
        suppressions,
        line_offsets_by_file,
    )
    .into_iter()
    .map(InvalidClientExportFinding::with_actions)
    .collect();
}

/// Populate `mixed_client_server_barrels` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_mixed_client_server_barrels`]).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the invalid-client-export populate site; threading the resolved modules + gate context is intrinsic"
)]
fn populate_mixed_client_server_barrel_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.mixed_client_server_barrel == Severity::Off {
        return;
    }
    results.mixed_client_server_barrels = find_mixed_client_server_barrels(
        graph,
        modules,
        resolved_modules,
        declared_deps,
        suppressions,
        line_offsets_by_file,
    )
    .into_iter()
    .map(MixedClientServerBarrelFinding::with_actions)
    .collect();
}

/// Populate `misplaced_directives` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_misplaced_directives`]).
fn populate_misplaced_directive_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.misplaced_directive == Severity::Off {
        return;
    }
    results.misplaced_directives = find_misplaced_directives(
        graph,
        modules,
        declared_deps,
        suppressions,
        line_offsets_by_file,
    )
    .into_iter()
    .map(MisplacedDirectiveFinding::with_actions)
    .collect();
}

/// Populate `unprovided_injects` when the rule is enabled. Gated on the project
/// declaring `vue` / `@vue/runtime-core` / `svelte` inside the detector (see
/// [`find_unprovided_injects`]).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the mixed-client-server-barrel populate site; threading resolved modules + the public-API entry-point set + the gate context is intrinsic"
)]
fn populate_unprovided_inject_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unprovided_injects == Severity::Off {
        return;
    }
    results.unprovided_injects = find_unprovided_injects(
        graph,
        resolved_modules,
        modules,
        declared_deps,
        public_api_entry_points,
        suppressions,
        line_offsets_by_file,
    )
    .into_iter()
    .map(UnprovidedInjectFinding::with_actions)
    .collect();
}

/// Populate `unrendered_components` when the rule is enabled. Gated on the
/// project declaring `vue` / `svelte` inside the detector (see
/// [`find_unrendered_components`]).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the unprovided-inject populate site; threading resolved modules + the public-API entry-point set + the suppression context is intrinsic"
)]
fn populate_unrendered_component_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unrendered_components == Severity::Off {
        return;
    }
    results.unrendered_components = find_unrendered_components(
        graph,
        resolved_modules,
        modules,
        declared_deps,
        public_api_entry_points,
        suppressions,
    )
    .into_iter()
    .map(UnrenderedComponentFinding::with_actions)
    .collect();
}

/// Populate `unused_component_props` when the rule is enabled. Gated on the
/// project declaring `vue` / `@vue/runtime-core` / `nuxt` inside the detector
/// (see [`find_unused_component_props`]).
fn populate_unused_component_prop_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unused_component_props == Severity::Off {
        return;
    }
    results.unused_component_props =
        find_unused_component_props(graph, modules, declared_deps, line_offsets_by_file)
            .into_iter()
            .map(UnusedComponentPropFinding::with_actions)
            .collect();
}

/// Populate `unused_component_emits` when the rule is enabled. Gated on the
/// project declaring `vue` / `@vue/runtime-core` / `nuxt` inside the detector
/// (see [`find_unused_component_emits`]).
fn populate_unused_component_emit_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unused_component_emits == Severity::Off {
        return;
    }
    results.unused_component_emits =
        find_unused_component_emits(graph, modules, declared_deps, line_offsets_by_file)
            .into_iter()
            .map(UnusedComponentEmitFinding::with_actions)
            .collect();
}

/// Populate `route_collisions` when the rule is enabled. Gated on the project
/// declaring `next` inside the detector (see [`find_route_collisions`]).
fn populate_route_collision_findings(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.route_collision == Severity::Off {
        return;
    }
    results.route_collisions =
        find_route_collisions(graph, config, workspaces, declared_deps, suppressions)
            .into_iter()
            .map(RouteCollisionFinding::with_actions)
            .collect();
}

/// Populate `dynamic_segment_name_conflicts` when the rule is enabled. Gated on
/// the project declaring `next` inside the detector (see
/// [`find_dynamic_segment_name_conflicts`]).
fn populate_dynamic_segment_name_conflict_findings(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.dynamic_segment_name_conflict == Severity::Off {
        return;
    }
    results.dynamic_segment_name_conflicts =
        find_dynamic_segment_name_conflicts(graph, config, workspaces, declared_deps, suppressions)
            .into_iter()
            .map(DynamicSegmentNameConflictFinding::with_actions)
            .collect();
}

/// Populate both Next.js App Router route-tree findings (`route_collisions` and
/// `dynamic_segment_name_conflicts`). Both share the same path-only primitive
/// (see [`crate::analyze::route_tree`]) and are gated on the project declaring
/// `next` inside their detectors.
fn populate_nextjs_route_tree_findings(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    results: &mut AnalysisResults,
) {
    populate_route_collision_findings(
        graph,
        config,
        workspaces,
        declared_deps,
        suppressions,
        results,
    );
    populate_dynamic_segment_name_conflict_findings(
        graph,
        config,
        workspaces,
        declared_deps,
        suppressions,
        results,
    );
}

#[derive(Clone, Copy)]
struct DeadCodeDetectorInput<'a> {
    graph: &'a ModuleGraph,
    config: &'a ResolvedConfig,
    resolved_modules: &'a [ResolvedModule],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    modules: &'a [ModuleInfo],
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    plugin_result: Option<&'a crate::plugins::AggregatedPluginResult>,
    pkg: Option<&'a PackageJson>,
    user_class_members: &'a [fallow_config::UsedClassMemberRule],
    public_api_entry_points: &'a FxHashSet<FileId>,
    virtual_prefixes: &'a [&'a str],
    generated_patterns: &'a [&'a str],
    generated_type_prefixes: &'a [&'a str],
    declared_deps: &'a FxHashSet<String>,
    collect_usages: bool,
}

fn run_parallel_dead_code_detectors(input: DeadCodeDetectorInput<'_>) -> AnalysisResults {
    let (
        (unused_files, export_results),
        (
            (member_results, dependency_results),
            (
                (unresolved_imports, duplicate_exports),
                (
                    (
                        boundary_violations,
                        (
                            boundary_coverage_violations,
                            (boundary_call_violations, policy_violations),
                        ),
                    ),
                    (circular_dependencies, (re_export_cycles, export_usages)),
                ),
            ),
        ),
    ) = rayon::join(
        || run_file_and_export_detectors(input),
        || {
            rayon::join(
                || run_member_and_dependency_detectors(input),
                || {
                    rayon::join(
                        || run_import_and_duplicate_detectors(input),
                        || run_boundary_cycle_and_usage_detectors(input),
                    )
                },
            )
        },
    );

    AnalysisResults {
        unused_files,
        unused_exports: export_results.unused_exports,
        unused_types: export_results.unused_types,
        private_type_leaks: export_results.private_type_leaks,
        stale_suppressions: export_results.stale_suppressions,
        unused_enum_members: member_results.unused_enum_members,
        unused_class_members: member_results.unused_class_members,
        unused_store_members: member_results.unused_store_members,
        unused_dependencies: dependency_results.unused_dependencies,
        unused_dev_dependencies: dependency_results.unused_dev_dependencies,
        unused_optional_dependencies: dependency_results.unused_optional_dependencies,
        unlisted_dependencies: dependency_results.unlisted_dependencies,
        type_only_dependencies: dependency_results.type_only_dependencies,
        test_only_dependencies: dependency_results.test_only_dependencies,
        unresolved_imports,
        duplicate_exports,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        circular_dependencies,
        re_export_cycles,
        export_usages,
        ..AnalysisResults::default()
    }
}

fn run_file_and_export_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (Vec<UnusedFileFinding>, AnalysisResults) {
    rayon::join(
        || run_unused_file_detector(input.graph, input.config, input.suppressions),
        || {
            run_export_detectors(
                input.graph,
                input.modules,
                input.config,
                input.plugin_result,
                input.suppressions,
                input.line_offsets_by_file,
            )
        },
    )
}

fn run_member_and_dependency_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (AnalysisResults, AnalysisResults) {
    rayon::join(
        || {
            run_member_detectors(
                input.graph,
                input.resolved_modules,
                input.modules,
                input.config,
                input.suppressions,
                input.line_offsets_by_file,
                input.user_class_members,
                input.public_api_entry_points,
                input.declared_deps,
            )
        },
        || {
            run_dependency_detectors(
                input.graph,
                input.pkg,
                input.config,
                input.plugin_result,
                input.workspaces,
                input.resolved_modules,
                input.line_offsets_by_file,
            )
        },
    )
}

fn run_import_and_duplicate_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (Vec<UnresolvedImportFinding>, Vec<DuplicateExportFinding>) {
    rayon::join(
        || {
            run_unresolved_import_detector(
                input.resolved_modules,
                input.config,
                input.suppressions,
                input.virtual_prefixes,
                input.generated_patterns,
                input.generated_type_prefixes,
                input.line_offsets_by_file,
            )
        },
        || {
            run_duplicate_export_detector(
                input.graph,
                input.config,
                input.suppressions,
                input.line_offsets_by_file,
                input.plugin_result,
                input.resolved_modules,
            )
        },
    )
}

type BoundaryAuxResults = (
    Vec<BoundaryCoverageViolationFinding>,
    (
        Vec<BoundaryCallViolationFinding>,
        Vec<PolicyViolationFinding>,
    ),
);

type BoundaryCycleUsageResults = (
    (Vec<BoundaryViolationFinding>, BoundaryAuxResults),
    (
        Vec<CircularDependencyFinding>,
        (Vec<ReExportCycleFinding>, Vec<crate::results::ExportUsage>),
    ),
);

fn run_boundary_cycle_and_usage_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> BoundaryCycleUsageResults {
    rayon::join(
        || {
            rayon::join(
                || {
                    run_boundary_violation_detector(
                        input.graph,
                        input.config,
                        input.suppressions,
                        input.line_offsets_by_file,
                    )
                },
                || {
                    run_boundary_aux_detectors(
                        input.graph,
                        input.modules,
                        input.config,
                        input.suppressions,
                        input.line_offsets_by_file,
                    )
                },
            )
        },
        || {
            rayon::join(
                || {
                    run_circular_dep_detector(
                        input.graph,
                        input.config,
                        input.line_offsets_by_file,
                        input.suppressions,
                        input.workspaces,
                    )
                },
                || {
                    rayon::join(
                        || {
                            run_re_export_cycle_detector(
                                input.graph,
                                input.config,
                                input.suppressions,
                            )
                        },
                        || {
                            run_export_usages_collector(
                                input.graph,
                                input.line_offsets_by_file,
                                input.collect_usages,
                            )
                        },
                    )
                },
            )
        },
    )
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn run_duplicate_export_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    resolved_modules: &[ResolvedModule],
) -> Vec<DuplicateExportFinding> {
    if config.rules.duplicate_exports == Severity::Off {
        return Vec::new();
    }
    let duplicate_exports = if let Some(plugin_result) = plugin_result {
        unused_exports::find_duplicate_exports_with_plugins(
            graph,
            config,
            suppressions,
            line_offsets_by_file,
            Some(plugin_result),
            resolved_modules,
        )
    } else {
        unused_exports::find_duplicate_exports(
            graph,
            config,
            suppressions,
            line_offsets_by_file,
            resolved_modules,
        )
    };
    duplicate_exports
        .into_iter()
        .map(DuplicateExportFinding::with_actions)
        .collect()
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn run_boundary_violation_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryViolationFinding> {
    if config.rules.boundary_violation == Severity::Off || config.boundaries.is_empty() {
        return Vec::new();
    }
    boundary::find_boundary_violations(graph, config, suppressions, line_offsets_by_file)
        .into_iter()
        .map(BoundaryViolationFinding::with_actions)
        .collect()
}

fn filter_public_workspace_results(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    results: &mut AnalysisResults,
) {
    let public_roots = public_workspace_roots(&config.public_packages, workspaces);
    if public_roots.is_empty() {
        return;
    }
    results.unused_exports.retain(|e| {
        !public_roots
            .iter()
            .any(|root| e.export.path.starts_with(root))
    });
    results.unused_types.retain(|e| {
        !public_roots
            .iter()
            .any(|root| e.export.path.starts_with(root))
    });
    results.unused_enum_members.retain(|e| {
        !public_roots
            .iter()
            .any(|root| e.member.path.starts_with(root))
    });
    results.unused_class_members.retain(|e| {
        !public_roots
            .iter()
            .any(|root| e.member.path.starts_with(root))
    });
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn populate_pnpm_catalog_findings(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    results: &mut AnalysisResults,
) {
    let need_unused = config.rules.unused_catalog_entries != Severity::Off;
    let need_empty_groups = config.rules.empty_catalog_groups != Severity::Off;
    let need_unresolved_refs = config.rules.unresolved_catalog_references != Severity::Off;
    let Some(state) = ((need_unused || need_empty_groups || need_unresolved_refs)
        .then(|| gather_pnpm_catalog_state(config, workspaces)))
    .flatten() else {
        return;
    };

    if need_unused {
        results.unused_catalog_entries = find_unused_catalog_entries(&state)
            .into_iter()
            .map(UnusedCatalogEntryFinding::with_actions)
            .collect();
    }
    if need_empty_groups {
        results.empty_catalog_groups = find_empty_catalog_groups(&state)
            .into_iter()
            .map(EmptyCatalogGroupFinding::with_actions)
            .collect();
    }
    if need_unresolved_refs {
        results.unresolved_catalog_references = find_unresolved_catalog_references(
            &state,
            &config.compiled_ignore_catalog_references,
            &config.root,
        )
        .into_iter()
        .map(UnresolvedCatalogReferenceFinding::with_actions)
        .collect();
    }
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn populate_pnpm_override_findings(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    results: &mut AnalysisResults,
) {
    let need_unused = config.rules.unused_dependency_overrides != Severity::Off;
    let need_misconfigured = config.rules.misconfigured_dependency_overrides != Severity::Off;
    let Some(state) = ((need_unused || need_misconfigured)
        .then(|| gather_pnpm_override_state(config, workspaces)))
    .flatten() else {
        return;
    };

    if need_unused {
        results.unused_dependency_overrides = find_unused_dependency_overrides(&state, config)
            .into_iter()
            .map(UnusedDependencyOverrideFinding::with_actions)
            .collect();
    }
    if need_misconfigured {
        results.misconfigured_dependency_overrides =
            find_misconfigured_dependency_overrides(&state, config)
                .into_iter()
                .map(MisconfiguredDependencyOverrideFinding::with_actions)
                .collect();
    }
}

fn populate_security_findings(
    ctx: &SecurityDetectionContext<'_, '_>,
    results: &mut AnalysisResults,
) {
    if ctx.config.rules.security_client_server_leak != Severity::Off {
        let (security_findings, stats) = security::find_security_findings(
            ctx.graph,
            ctx.modules,
            ctx.suppressions,
            ctx.line_offsets_by_file,
        );
        results.security_findings = security_findings;
        results.security_unresolved_edge_files = stats.client_files_with_unresolved_edges;
    }

    if ctx.config.rules.security_sink != Severity::Off {
        populate_tainted_sink_findings(ctx, results);
    }

    if !results.security_findings.is_empty() {
        annotate_security_findings(ctx, results);
    }
}

fn populate_tainted_sink_findings(
    ctx: &SecurityDetectionContext<'_, '_>,
    results: &mut AnalysisResults,
) {
    let categories = ctx.config.security.categories.as_ref();
    let filter = security::CategoryFilter::new(
        categories.and_then(|c| c.include.clone()),
        categories.and_then(|c| c.exclude.clone()),
    );
    let (sink_findings, sink_stats) = security::find_tainted_sinks(
        ctx.graph,
        ctx.modules,
        ctx.suppressions,
        ctx.line_offsets_by_file,
        ctx.declared_deps,
        &security::TaintedSinkContext {
            category_filter: &filter,
            request_receivers: ctx.request_receivers,
            root: &ctx.config.root,
        },
    );
    results.security_findings.extend(sink_findings);
    results.security_unresolved_callee_sites = sink_stats.sinks_skipped_dynamic_callee;
    results.security_unresolved_callee_diagnostics = sink_stats.unresolved_callee_diagnostics;
    results
        .security_findings
        .extend(security::find_hardcoded_secret_candidates(
            ctx.graph,
            ctx.modules,
            ctx.suppressions,
            ctx.line_offsets_by_file,
            &filter,
            &ctx.config.root,
        ));
}

fn annotate_security_findings(
    ctx: &SecurityDetectionContext<'_, '_>,
    results: &mut AnalysisResults,
) {
    security::annotate_dead_code_cross_links(
        ctx.graph,
        ctx.modules,
        ctx.line_offsets_by_file,
        &results.unused_files,
        &results.unused_exports,
        &mut results.security_findings,
    );
    let boundary_crossings = boundary_crossings_by_file(&results.boundary_violations);
    security::rank_security_findings(
        ctx.graph,
        ctx.modules,
        ctx.line_offsets_by_file,
        ctx.declared_deps,
        ctx.request_receivers,
        &boundary_crossings,
        &mut results.security_findings,
    );
}

fn boundary_crossings_by_file(
    boundary_violations: &[BoundaryViolationFinding],
) -> FxHashMap<std::path::PathBuf, (String, String)> {
    let mut boundary_crossings: FxHashMap<std::path::PathBuf, (String, String)> =
        FxHashMap::default();
    for violation in boundary_violations {
        let zones = (
            violation.violation.from_zone.clone(),
            violation.violation.to_zone.clone(),
        );
        for path in [
            violation.violation.from_path.clone(),
            violation.violation.to_path.clone(),
        ] {
            boundary_crossings
                .entry(path)
                .and_modify(|existing| {
                    if zones < *existing {
                        *existing = zones.clone();
                    }
                })
                .or_insert_with(|| zones.clone());
        }
    }
    boundary_crossings
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn run_unused_file_detector(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
) -> Vec<UnusedFileFinding> {
    if config.rules.unused_files == Severity::Off {
        return Vec::new();
    }
    find_unused_files(graph, suppressions)
        .into_iter()
        .map(UnusedFileFinding::with_actions)
        .collect()
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn run_export_detectors(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    if config.rules.unused_exports == Severity::Off
        && config.rules.unused_types == Severity::Off
        && config.rules.private_type_leaks == Severity::Off
    {
        return results;
    }

    let (exports, types, stale_expected) = find_unused_exports(
        graph,
        modules,
        config,
        plugin_result,
        suppressions,
        line_offsets_by_file,
    );
    if config.rules.unused_exports != Severity::Off {
        results.unused_exports = exports
            .into_iter()
            .map(UnusedExportFinding::with_actions)
            .collect();
    }
    if config.rules.unused_types != Severity::Off {
        let mut typed = types;
        suppress_signature_backing_types(&mut typed, graph, modules);
        results.unused_types = typed
            .into_iter()
            .map(UnusedTypeFinding::with_actions)
            .collect();
    }
    if config.rules.private_type_leaks != Severity::Off {
        results.private_type_leaks =
            find_private_type_leaks(graph, modules, config, suppressions, line_offsets_by_file)
                .into_iter()
                .map(PrivateTypeLeakFinding::with_actions)
                .collect();
    }
    if config.rules.stale_suppressions != Severity::Off {
        results.stale_suppressions.extend(stale_expected);
    }
    results
}

#[expect(
    clippy::too_many_arguments,
    reason = "member detection needs graph context plus public API and allowlist filters"
)]
fn run_member_detectors(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    user_class_members: &[fallow_config::UsedClassMemberRule],
    public_api_entry_points: &FxHashSet<FileId>,
    declared_deps: &FxHashSet<String>,
) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    // Store-member detection activates only when Pinia is a declared dependency,
    // so an unrelated user `defineStore`-named helper in a non-Pinia project
    // never fires. The harvest is intentionally loose at extraction time; this
    // is the activation boundary.
    let store_members_active = config.rules.unused_store_members != Severity::Off
        && (declared_deps.contains("pinia") || declared_deps.contains("@pinia/nuxt"));
    if config.rules.unused_enum_members == Severity::Off
        && config.rules.unused_class_members == Severity::Off
        && !store_members_active
    {
        return results;
    }

    let member_results = find_unused_members_with_public_api_entry_points(
        graph,
        resolved_modules,
        modules,
        suppressions,
        line_offsets_by_file,
        user_class_members,
        &config.ignore_decorators,
        public_api_entry_points,
    );
    if config.rules.unused_enum_members != Severity::Off {
        results.unused_enum_members = member_results
            .enum_members
            .into_iter()
            .map(UnusedEnumMemberFinding::with_actions)
            .collect();
    }
    if config.rules.unused_class_members != Severity::Off {
        results.unused_class_members = member_results
            .class_members
            .into_iter()
            .map(UnusedClassMemberFinding::with_actions)
            .collect();
    }
    if store_members_active {
        results.unused_store_members = member_results
            .store_members
            .into_iter()
            .map(UnusedStoreMemberFinding::with_actions)
            .collect();
    }
    results
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn run_dependency_detectors(
    graph: &ModuleGraph,
    pkg: Option<&PackageJson>,
    config: &ResolvedConfig,
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    workspaces: &[fallow_config::WorkspaceInfo],
    resolved_modules: &[ResolvedModule],
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    let Some(pkg) = pkg else {
        return results;
    };

    if config.rules.unused_dependencies != Severity::Off
        || config.rules.unused_dev_dependencies != Severity::Off
        || config.rules.unused_optional_dependencies != Severity::Off
    {
        let (deps, dev_deps, optional_deps) =
            find_unused_dependencies(graph, pkg, config, plugin_result, workspaces);
        if config.rules.unused_dependencies != Severity::Off {
            results.unused_dependencies = deps
                .into_iter()
                .map(UnusedDependencyFinding::with_actions)
                .collect();
        }
        if config.rules.unused_dev_dependencies != Severity::Off {
            results.unused_dev_dependencies = dev_deps
                .into_iter()
                .map(UnusedDevDependencyFinding::with_actions)
                .collect();
        }
        if config.rules.unused_optional_dependencies != Severity::Off {
            results.unused_optional_dependencies = optional_deps
                .into_iter()
                .map(UnusedOptionalDependencyFinding::with_actions)
                .collect();
        }
    }

    if config.rules.unlisted_dependencies != Severity::Off {
        results.unlisted_dependencies = find_unlisted_dependencies(
            graph,
            pkg,
            config,
            workspaces,
            plugin_result,
            resolved_modules,
            line_offsets_by_file,
        )
        .into_iter()
        .map(UnlistedDependencyFinding::with_actions)
        .collect();
    }

    if config.production {
        results.type_only_dependencies =
            find_type_only_dependencies(graph, pkg, config, workspaces)
                .into_iter()
                .map(TypeOnlyDependencyFinding::with_actions)
                .collect();
    }

    if !config.production && config.rules.test_only_dependencies != Severity::Off {
        results.test_only_dependencies =
            find_test_only_dependencies(graph, pkg, config, workspaces)
                .into_iter()
                .map(TestOnlyDependencyFinding::with_actions)
                .collect();
    }
    results
}

fn run_unresolved_import_detector(
    resolved_modules: &[ResolvedModule],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    virtual_prefixes: &[&str],
    generated_patterns: &[&str],
    generated_type_prefixes: &[&str],
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnresolvedImportFinding> {
    if config.rules.unresolved_imports == Severity::Off || resolved_modules.is_empty() {
        return Vec::new();
    }
    find_unresolved_imports(
        resolved_modules,
        config,
        suppressions,
        virtual_prefixes,
        generated_patterns,
        generated_type_prefixes,
        line_offsets_by_file,
    )
    .into_iter()
    .map(UnresolvedImportFinding::with_actions)
    .collect()
}

#[cfg(test)]
#[expect(
    deprecated,
    reason = "ADR-008 keeps direct analyzer unit tests while the public warning targets external callers"
)]
mod tests {
    use fallow_types::extract::{byte_offset_to_line_col, compute_line_offsets};

    fn line_col(source: &str, byte_offset: u32) -> (u32, u32) {
        let offsets = compute_line_offsets(source);
        byte_offset_to_line_col(&offsets, byte_offset)
    }

    #[test]
    fn compute_offsets_empty() {
        assert_eq!(compute_line_offsets(""), vec![0]);
    }

    #[test]
    fn compute_offsets_single_line() {
        assert_eq!(compute_line_offsets("hello"), vec![0]);
    }

    #[test]
    fn compute_offsets_multiline() {
        assert_eq!(compute_line_offsets("abc\ndef\nghi"), vec![0, 4, 8]);
    }

    #[test]
    fn compute_offsets_trailing_newline() {
        assert_eq!(compute_line_offsets("abc\n"), vec![0, 4]);
    }

    #[test]
    fn compute_offsets_crlf() {
        assert_eq!(compute_line_offsets("ab\r\ncd"), vec![0, 4]);
    }

    #[test]
    fn compute_offsets_consecutive_newlines() {
        assert_eq!(compute_line_offsets("\n\n"), vec![0, 1, 2]);
    }

    #[test]
    fn byte_offset_empty_source() {
        assert_eq!(line_col("", 0), (1, 0));
    }

    #[test]
    fn byte_offset_single_line_start() {
        assert_eq!(line_col("hello", 0), (1, 0));
    }

    #[test]
    fn byte_offset_single_line_middle() {
        assert_eq!(line_col("hello", 4), (1, 4));
    }

    #[test]
    fn byte_offset_multiline_start_of_line2() {
        assert_eq!(line_col("line1\nline2\nline3", 6), (2, 0));
    }

    #[test]
    fn byte_offset_multiline_middle_of_line3() {
        assert_eq!(line_col("line1\nline2\nline3", 14), (3, 2));
    }

    #[test]
    fn byte_offset_at_newline_boundary() {
        assert_eq!(line_col("line1\nline2", 5), (1, 5));
    }

    #[test]
    fn byte_offset_multibyte_utf8() {
        let source = "hi\n\u{1F600}x";
        assert_eq!(line_col(source, 3), (2, 0));
        assert_eq!(line_col(source, 7), (2, 4));
    }

    #[test]
    fn byte_offset_multibyte_accented_chars() {
        let source = "caf\u{00E9}\nbar";
        assert_eq!(line_col(source, 6), (2, 0));
        assert_eq!(line_col(source, 3), (1, 3));
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
        let offsets = compute_line_offsets("abc\ndef\nghi");
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

        #[test]
        fn find_dead_code_all_rules_off_returns_empty() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::graph::ModuleGraph;
            use crate::resolve::ResolvedModule;
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
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                namespace_object_aliases: vec![],
            }];
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);

            let rules = RulesConfig {
                unused_files: Severity::Off,
                unused_exports: Severity::Off,
                unused_types: Severity::Off,
                private_type_leaks: Severity::Off,
                unused_dependencies: Severity::Off,
                unused_dev_dependencies: Severity::Off,
                unused_optional_dependencies: Severity::Off,
                unused_enum_members: Severity::Off,
                unused_class_members: Severity::Off,
                unused_store_members: Severity::Off,
                unprovided_injects: Severity::Off,
                unrendered_components: Severity::Off,
                unused_component_props: Severity::Off,
                unused_component_emits: Severity::Off,
                unused_server_actions: Severity::Off,
                unresolved_imports: Severity::Off,
                unlisted_dependencies: Severity::Off,
                duplicate_exports: Severity::Off,
                type_only_dependencies: Severity::Off,
                circular_dependencies: Severity::Off,
                re_export_cycle: Severity::Off,
                test_only_dependencies: Severity::Off,
                boundary_violation: Severity::Off,
                coverage_gaps: Severity::Off,
                feature_flags: Severity::Off,
                stale_suppressions: Severity::Off,
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
            };
            let config = make_config_with_rules(rules);
            let results = find_dead_code(&graph, &config);

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
            assert!(results.export_usages.is_empty());
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
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                namespace_object_aliases: vec![],
            }];
            let mut graph = ModuleGraph::build(&resolved, &entry_points, &files);
            graph.modules[0].exports = vec![ExportSymbol {
                name: ExportName::Named("myExport".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: Span::new(10, 30),
                references: vec![],
                members: vec![],
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
        fn find_dead_code_delegates_to_find_dead_code_with_resolved() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::graph::ModuleGraph;
            use crate::resolve::ResolvedModule;
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
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                namespace_object_aliases: vec![],
            }];
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);
            let config = make_config_with_rules(RulesConfig::default());

            let results = find_dead_code(&graph, &config);
            assert!(results.unused_exports.is_empty());
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
                    exports: vec![],
                    re_exports: vec![],
                    resolved_imports: vec![],
                    resolved_dynamic_imports: vec![],
                    resolved_dynamic_patterns: vec![],
                    member_accesses: vec![],
                    whole_object_uses: vec![],
                    has_cjs_exports: false,
                    has_angular_component_template_url: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                    namespace_object_aliases: vec![],
                })
                .collect::<Vec<_>>();
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);

            let modules = vec![ModuleInfo {
                file_id: FileId(1),
                exports: vec![],
                imports: vec![],
                re_exports: vec![],
                dynamic_imports: vec![],
                dynamic_import_patterns: vec![],
                require_calls: vec![],
                package_path_references: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                has_angular_component_template_url: false,
                content_hash: 0,
                suppressions: vec![Suppression::issue(0, 1, IssueKind::UnusedFile)],
                unknown_suppression_kinds: vec![],
                unused_import_bindings: vec![],
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                line_offsets: vec![],
                complexity: vec![],
                flag_uses: vec![],
                class_heritage: vec![],
                injection_tokens: vec![],
                local_type_declarations: Vec::new(),
                public_signature_type_references: Vec::new(),
                namespace_object_aliases: Vec::new(),
                iconify_prefixes: Vec::new(),
                iconify_icon_names: Vec::new(),
                auto_import_candidates: Vec::new(),
                directives: Vec::new(),
                client_only_dynamic_import_spans: Vec::new(),
                security_sinks: Vec::new(),
                security_sinks_skipped: 0,
                security_unresolved_callee_sites: Vec::new(),
                tainted_bindings: Vec::new(),
                sanitized_sink_args: Vec::new(),
                security_control_sites: Vec::new(),
                callee_uses: Vec::new(),
                misplaced_directives: Vec::new(),
                di_key_sites: Vec::new(),
                has_dynamic_provide: false,
                referenced_import_bindings: Vec::new(),
                component_props: Vec::new(),
                has_props_attrs_fallthrough: false,
                has_define_expose: false,
                has_define_model: false,
                has_unharvestable_props: false,
                component_emits: Vec::new(),
                has_unharvestable_emits: false,
                has_dynamic_emit: false,
                has_emit_whole_object_use: false,
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
}
