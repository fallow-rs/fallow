mod boundary;
mod boundary_calls;
mod boundary_coverage;
mod duplicate_prop_shape;
mod dynamic_segment_name_conflict;
pub mod feature_flags;
mod iconify;
mod invalid_client_exports;
mod misplaced_directive;
mod mixed_barrel;
mod package_json_utils;
mod policy;
mod predicates;
mod prop_drilling;
mod re_export_cycles;
mod react_intel;
mod react_resolve;
mod render_fan_in;
mod route_collision;
mod route_tree;
mod security;
mod server_only;
mod thin_wrapper;
mod unprovided_inject;
mod unrendered_component;
mod unused_catalog;
mod unused_component_emit;
mod unused_component_input;
mod unused_component_output;
mod unused_component_prop;
mod unused_deps;
mod unused_exports;
mod unused_files;
mod unused_load_data_key;
mod unused_members;
mod unused_overrides;
mod unused_server_action;
mod unused_svelte_event;

#[cfg(test)]
pub(crate) mod test_support;

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
    CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
    PropDrillingChainFinding, ReExportCycleFinding, RouteCollisionFinding,
    TestOnlyDependencyFinding, ThinWrapperFinding, TypeOnlyDependencyFinding,
    UnlistedDependencyFinding, UnprovidedInjectFinding, UnrenderedComponentFinding,
    UnresolvedCatalogReferenceFinding, UnresolvedImportFinding, UnusedCatalogEntryFinding,
    UnusedClassMemberFinding, UnusedComponentEmitFinding, UnusedComponentInputFinding,
    UnusedComponentOutputFinding, UnusedComponentPropFinding, UnusedDependencyFinding,
    UnusedDependencyOverrideFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding,
    UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
    UnusedOptionalDependencyFinding, UnusedStoreMemberFinding, UnusedSvelteEventFinding,
    UnusedTypeFinding,
};

use crate::results::{
    AnalysisResults, CircularDependency, CircularDependencyEdge, StaleSuppression,
    UnusedDependency, UnusedExport, UnusedMember,
};
use crate::suppress::{IssueKind, SuppressionContext};

use duplicate_prop_shape::find_duplicate_prop_shapes;
use dynamic_segment_name_conflict::find_dynamic_segment_name_conflicts;
use invalid_client_exports::find_invalid_client_exports;
use misplaced_directive::find_misplaced_directives;
use mixed_barrel::find_mixed_client_server_barrels;
use prop_drilling::find_prop_drilling_chains;
use re_export_cycles::find_re_export_cycles;
use react_intel::compute_react_component_intel;
use render_fan_in::compute_render_fan_in;
use route_collision::find_route_collisions;
use thin_wrapper::find_thin_wrappers;
use unprovided_inject::{UnprovidedInjectInput, find_unprovided_injects};
use unrendered_component::{
    LitUnrenderedInput, find_unrendered_angular_components, find_unrendered_components,
    find_unrendered_lit_elements,
};
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_catalog::{
    find_empty_catalog_groups, find_unresolved_catalog_references, find_unused_catalog_entries,
    gather_pnpm_catalog_state,
};
use unused_component_emit::find_unused_component_emits;
use unused_component_input::find_unused_component_inputs;
use unused_component_output::find_unused_component_outputs;
use unused_component_prop::{find_unused_component_props, find_unused_react_props};
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_deps::{
    UnlistedDependencyInput, find_test_only_dependencies, find_type_only_dependencies,
    find_unlisted_dependencies, find_unresolved_imports, find_unused_dependencies,
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
use unused_load_data_key::find_unused_load_data_keys;
use unused_members::{UnusedMemberScanInput, find_unused_members_with_public_api_entry_points};
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
use unused_overrides::{
    find_misconfigured_dependency_overrides, find_unused_dependency_overrides,
    gather_pnpm_override_state,
};
use unused_server_action::reclassify_unused_server_actions;
use unused_svelte_event::find_unused_svelte_events;

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
/// Files outside all workspace roots (e.g., root-level shared code) are ignored,
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

/// Build the raw (as-discovered) module-path -> `FileId` index.
///
/// Public-API entry-point resolution previously also canonicalized every module
/// here (one `realpath` syscall per module, ~21k on a large monorepo) so the map
/// could match an entry point expressed in a module's canonical form. That eager
/// sweep is almost entirely wasted: the consumer
/// ([`add_package_public_api_entry_points`]) already canonicalizes the ENTRY and
/// matches it against raw module paths, which covers every project without
/// intra-project symlinks. The residual symlinked-module case is handled lazily
/// and package-scoped by [`resolve_entry_via_scoped_canonical`], so the common
/// path pays zero canonicalize syscalls.
fn graph_path_to_file_id(graph: &ModuleGraph) -> FxHashMap<std::path::PathBuf, FileId> {
    graph
        .modules
        .iter()
        .map(|module| (module.path.clone(), module.file_id))
        .collect()
}

/// Resolve a canonicalized entry-point path against the canonical form of the
/// modules UNDER `package_root`, without canonicalizing the whole project.
///
/// Only reached when an entry point matches neither a raw module path nor the
/// canonicalized-entry-against-raw-map lookup, i.e. the module is reached through
/// an intra-project symlink so its stored (raw) path differs from its canonical
/// path. Scoping the scan to the entry's own package keeps a fruitless miss
/// (e.g. a `bin` script that is not a discovered module) bounded by that
/// package's file count instead of the entire graph.
fn resolve_entry_via_scoped_canonical(
    graph: &ModuleGraph,
    package_root: &std::path::Path,
    canonical_entry: &std::path::Path,
) -> Option<FileId> {
    match_canonical_entry_under_package(
        graph.modules.iter().map(|m| (m.path.as_path(), m.file_id)),
        package_root,
        canonical_entry,
    )
}

/// Pure core of [`resolve_entry_via_scoped_canonical`], decoupled from
/// `ModuleGraph` for direct unit testing of the symlink-resolution path. Returns
/// the `FileId` of the first candidate under `package_root` whose canonical form
/// equals `canonical_entry`.
fn match_canonical_entry_under_package<'a>(
    candidates: impl Iterator<Item = (&'a std::path::Path, FileId)>,
    package_root: &std::path::Path,
    canonical_entry: &std::path::Path,
) -> Option<FileId> {
    candidates
        .filter(|(path, _)| path.starts_with(package_root))
        .find_map(|(path, file_id)| {
            (dunce::canonicalize(path).ok().as_deref() == Some(canonical_entry)).then_some(file_id)
        })
}

fn add_package_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &ModuleGraph,
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
                .and_then(|canonical| {
                    path_to_file_id.get(&canonical).copied().or_else(|| {
                        resolve_entry_via_scoped_canonical(graph, package_root, &canonical)
                    })
                })
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

/// Compute the exports-aware public-API entry-point set: the `package.json`
/// `exports`-mapped modules (non-private packages) plus the no-`exports`
/// source-index fallback. This encodes rule R4 (the `exports`-mapped copy is
/// public; a no-`exports` copy is internal). Exposed for the review brief,
/// which feeds it into [`ModuleGraph::public_export_keys`] to compute the
/// exports-aware public-API surface delta.
pub fn public_api_package_entry_points(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> FxHashSet<FileId> {
    let mut public_api_entry_points = FxHashSet::default();
    let path_to_file_id = graph_path_to_file_id(graph);
    let canonical_project_root =
        dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());

    add_root_public_api_entry_points(
        &mut public_api_entry_points,
        graph,
        &path_to_file_id,
        config,
        root_pkg,
        &canonical_project_root,
    );
    add_workspace_public_api_entry_points(
        &mut public_api_entry_points,
        graph,
        &path_to_file_id,
        workspaces,
        &canonical_project_root,
    );

    public_api_entry_points
}

fn add_root_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &ModuleGraph,
    path_to_file_id: &FxHashMap<std::path::PathBuf, FileId>,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    canonical_project_root: &std::path::Path,
) {
    if let Some(pkg) = root_pkg {
        add_package_public_api_entry_points(
            public_api_entry_points,
            graph,
            path_to_file_id,
            &config.root,
            pkg,
            canonical_project_root,
        );
        add_exportless_package_source_indexes(public_api_entry_points, graph, &config.root, pkg);
    }
}

fn add_workspace_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &ModuleGraph,
    path_to_file_id: &FxHashMap<std::path::PathBuf, FileId>,
    workspaces: &[fallow_config::WorkspaceInfo],
    canonical_project_root: &std::path::Path,
) {
    for workspace in workspaces {
        let Ok(pkg) = PackageJson::load(&workspace.root.join("package.json")) else {
            continue;
        };
        add_package_public_api_entry_points(
            public_api_entry_points,
            graph,
            path_to_file_id,
            &workspace.root,
            &pkg,
            canonical_project_root,
        );
        add_exportless_package_source_indexes(
            public_api_entry_points,
            graph,
            &workspace.root,
            &pkg,
        );
    }
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
            Some(circular_dependency_from_cycle(
                graph,
                line_offsets_map,
                &cycle,
            ))
        })
        .collect();

    if !workspaces.is_empty() {
        for dep in &mut dependencies {
            dep.is_cross_package = is_cross_package_cycle(&dep.files, workspaces);
        }
    }

    dependencies
}

fn circular_dependency_from_cycle(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    cycle: &[FileId],
) -> CircularDependency {
    // One anchor per hop in cycle order: `edges[i]` is the import in
    // `cycle[i]` pointing to `cycle[i + 1]`. Always populated for every
    // hop (fallback `(1, 0)` if the span is somehow missing) so
    // `edges.len() == files.len()` regardless of URL-resolvability on
    // the consumer side. The LSP renders one squiggly per edge.
    let edges: Vec<CircularDependencyEdge> = (0..cycle.len())
        .map(|edge_index| {
            let from = cycle[edge_index];
            let (line, col) =
                cycle_edge_line_col(graph, line_offsets_map, cycle, edge_index).unwrap_or((1, 0));
            CircularDependencyEdge {
                path: graph.modules[from.0 as usize].path.clone(),
                line,
                col,
            }
        })
        .collect();

    let files: Vec<std::path::PathBuf> = edges.iter().map(|edge| edge.path.clone()).collect();
    let length = files.len();
    // Top-level `line`/`col` remain the first hop's anchor for backward
    // compatibility with consumers that predate `edges`.
    let (line, col) = edges.first().map_or((1, 0), |edge| (edge.line, edge.col));
    CircularDependency {
        files,
        length,
        line,
        col,
        edges,
        is_cross_package: false,
    }
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
    declared_deps: &FxHashSet<String>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<PolicyViolationFinding> {
    if config.rules.policy_violation == Severity::Off || config.rule_packs.is_empty() {
        return Vec::new();
    }
    policy::find_policy_violations(
        graph,
        modules,
        config,
        declared_deps,
        suppressions,
        line_offsets_by_file,
    )
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
    declared_deps: &FxHashSet<String>,
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
                || {
                    run_policy_detector(
                        graph,
                        modules,
                        config,
                        declared_deps,
                        suppressions,
                        line_offsets_by_file,
                    )
                },
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

struct DeadCodeRunContext<'a> {
    suppressions: SuppressionContext<'a>,
    line_offsets_by_file: LineOffsetsMap<'a>,
    pkg: Option<PackageJson>,
    public_api_entry_points: FxHashSet<FileId>,
    declared_deps: FxHashSet<String>,
}

fn build_dead_code_run_context<'a>(
    graph: &'a ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    modules: &'a [ModuleInfo],
) -> DeadCodeRunContext<'a> {
    let suppressions = SuppressionContext::new(modules);
    let line_offsets_by_file: LineOffsetsMap<'a> = modules
        .iter()
        .filter(|m| !m.line_offsets.is_empty())
        .map(|m| (m.file_id, m.line_offsets.as_slice()))
        .collect();

    let pkg_path = config.root.join("package.json");
    let pkg = PackageJson::load(&pkg_path).ok();
    let public_api_entry_points =
        public_api_package_entry_points(graph, config, pkg.as_ref(), workspaces);
    let declared_deps = collect_declared_dependency_names(config, pkg.as_ref(), workspaces);

    DeadCodeRunContext {
        suppressions,
        line_offsets_by_file,
        pkg,
        public_api_entry_points,
        declared_deps,
    }
}

/// Find all dead code, with optional resolved module data, plugin context, and workspace info.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::detect_dead_code instead. NOTE: replacement returns serde_json::Value, not typed AnalysisResults. See docs/fallow-core-migration.md and ADR-008."
)]
#[expect(
    clippy::too_many_arguments,
    reason = "frozen deprecated public API (ADR-008); signature must not change"
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

    let run_context = build_dead_code_run_context(graph, config, workspaces, modules);

    let mut results = run_setup_and_detect(&SetupAndDetectInput {
        graph,
        config,
        resolved_modules,
        plugin_result,
        workspaces,
        modules,
        suppressions: &run_context.suppressions,
        line_offsets_by_file: &run_context.line_offsets_by_file,
        pkg: run_context.pkg.as_ref(),
        public_api_entry_points: &run_context.public_api_entry_points,
        declared_deps: &run_context.declared_deps,
        collect_usages,
    });

    populate_post_detection_findings(&mut PostDetectionInput {
        graph,
        modules,
        resolved_modules,
        config,
        workspaces,
        declared_deps: &run_context.declared_deps,
        public_api_entry_points: &run_context.public_api_entry_points,
        suppressions: &run_context.suppressions,
        line_offsets_by_file: &run_context.line_offsets_by_file,
        collect_usages,
        results: &mut results,
    });

    results.sort();

    results
}

/// Inputs to the dead-code setup-and-detect phase: the pre-run-shared context
/// plus the raw plugin result the iconify augmentation may extend.
struct SetupAndDetectInput<'a, 'm> {
    graph: &'a ModuleGraph,
    config: &'a ResolvedConfig,
    resolved_modules: &'a [ResolvedModule],
    plugin_result: Option<&'a crate::plugins::AggregatedPluginResult>,
    workspaces: &'a [fallow_config::WorkspaceInfo],
    modules: &'a [ModuleInfo],
    suppressions: &'a SuppressionContext<'m>,
    line_offsets_by_file: &'a LineOffsetsMap<'m>,
    pkg: Option<&'a PackageJson>,
    public_api_entry_points: &'a FxHashSet<FileId>,
    declared_deps: &'a FxHashSet<String>,
    collect_usages: bool,
}

/// Build the iconify-augmented plugin result, derive plugin-backed slices and
/// the user class-member set, then run the parallel dead-code detectors.
/// Extracted from `find_dead_code_full` to keep that orchestrator's body as
/// setup -> detect -> populate.
fn run_setup_and_detect(input: &SetupAndDetectInput<'_, '_>) -> AnalysisResults {
    let iconify_referenced =
        iconify::collect_iconify_referenced_deps(input.modules, input.pkg, input.workspaces);
    let augmented_plugin_result;
    let plugin_result = if iconify_referenced.is_empty() {
        input.plugin_result
    } else {
        let mut owned = input.plugin_result.cloned().unwrap_or_default();
        owned.referenced_dependencies.extend(iconify_referenced);
        augmented_plugin_result = owned;
        Some(&augmented_plugin_result)
    };

    let mut user_class_members = input.config.used_class_members.clone();
    if let Some(plugin_result) = plugin_result {
        user_class_members.extend(plugin_result.used_class_members.iter().cloned());
    }

    let (virtual_prefixes, generated_patterns, generated_type_prefixes) =
        derive_plugin_string_slices(plugin_result);

    run_parallel_dead_code_detectors(DeadCodeDetectorInput {
        graph: input.graph,
        config: input.config,
        resolved_modules: input.resolved_modules,
        workspaces: input.workspaces,
        modules: input.modules,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        plugin_result,
        pkg: input.pkg,
        user_class_members: &user_class_members,
        public_api_entry_points: input.public_api_entry_points,
        virtual_prefixes: &virtual_prefixes,
        generated_patterns: &generated_patterns,
        generated_type_prefixes: &generated_type_prefixes,
        declared_deps: input.declared_deps,
        collect_usages: input.collect_usages,
    })
}

/// Derive the borrowed plugin string slices (virtual module prefixes, generated
/// import patterns, generated type-import prefixes) consumed by the detectors.
fn derive_plugin_string_slices(
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
) -> (Vec<&str>, Vec<&str>, Vec<&str>) {
    let virtual_prefixes = plugin_result
        .map(|pr| {
            pr.virtual_module_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_patterns = plugin_result
        .map(|pr| {
            pr.generated_import_patterns
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_type_prefixes = plugin_result
        .map(|pr| {
            pr.generated_type_import_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    (
        virtual_prefixes,
        generated_patterns,
        generated_type_prefixes,
    )
}

/// Shared context for the post-detector populate sequence in
/// `find_dead_code_full`.
struct PostDetectionInput<'a, 'm> {
    graph: &'a ModuleGraph,
    modules: &'a [ModuleInfo],
    resolved_modules: &'a [ResolvedModule],
    config: &'a ResolvedConfig,
    workspaces: &'a [fallow_config::WorkspaceInfo],
    declared_deps: &'a FxHashSet<String>,
    public_api_entry_points: &'a FxHashSet<FileId>,
    suppressions: &'a SuppressionContext<'m>,
    line_offsets_by_file: &'a LineOffsetsMap<'m>,
    /// Whether the editor/LSP usages path is active; gates in-process-only
    /// intel (`react_component_intel`) off the bare `fallow` / `audit` hot path.
    collect_usages: bool,
    results: &'a mut AnalysisResults,
}

/// Run the post-detector populate/reclassify phases: server-action
/// reclassification, security, catalog/override, framework-convention findings,
/// and stale-suppression accounting. Extracted from `find_dead_code_full` so
/// that orchestrator reads as setup -> detect -> populate.
fn populate_post_detection_findings(input: &mut PostDetectionInput<'_, '_>) {
    filter_public_workspace_results(input.config, input.workspaces, input.results);

    // Reclassify the server-action subset of unused exports BEFORE stale
    // detection so a `// fallow-ignore-next-line unused-server-action` marker is
    // recorded as consumed. Gate-off keeps the findings as plain unused-exports.
    if input.config.rules.unused_server_actions != Severity::Off {
        reclassify_unused_server_actions(
            input.graph,
            input.modules,
            input.declared_deps,
            input.suppressions,
            input.results,
        );
    }

    populate_configured_security_findings(input);
    populate_package_and_framework_findings(input);
    populate_stale_suppression_findings(input);
}

fn populate_configured_security_findings(input: &mut PostDetectionInput<'_, '_>) {
    let request_receivers = input
        .config
        .security
        .request_receivers
        .iter()
        .cloned()
        .collect::<FxHashSet<_>>();

    populate_security_findings(
        &SecurityDetectionContext {
            graph: input.graph,
            modules: input.modules,
            config: input.config,
            suppressions: input.suppressions,
            line_offsets_by_file: input.line_offsets_by_file,
            declared_deps: input.declared_deps,
            request_receivers: &request_receivers,
        },
        input.results,
    );
}

fn populate_package_and_framework_findings(input: &mut PostDetectionInput<'_, '_>) {
    // Framework-convention detectors run BEFORE stale-suppression detection so
    // any inline suppression they consume (e.g. a `// fallow-ignore-next-line
    // unused-component-prop` honored by the prop/emit/component detectors) is
    // recorded consumed and not falsely reported stale. These detectors gate on
    // their own rule severity and dep presence, so they are no-ops when inactive.
    populate_pnpm_catalog_findings(input.config, input.workspaces, input.results);
    populate_pnpm_override_findings(input.config, input.workspaces, input.results);
    populate_framework_specific_findings(&mut FrameworkSpecificFindingsInput {
        graph: input.graph,
        modules: input.modules,
        resolved_modules: input.resolved_modules,
        config: input.config,
        workspaces: input.workspaces,
        declared_deps: input.declared_deps,
        public_api_entry_points: input.public_api_entry_points,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        collect_usages: input.collect_usages,
        results: input.results,
    });
}

/// Append stale-suppression and missing-reason findings, then record the
/// suppression accounting metadata onto the results.
fn populate_stale_suppression_findings(input: &mut PostDetectionInput<'_, '_>) {
    if input.config.rules.stale_suppressions != Severity::Off {
        input
            .results
            .stale_suppressions
            .extend(input.suppressions.find_stale(input.graph, input.config));
    }
    if input.config.rules.require_suppression_reason != Severity::Off {
        input
            .results
            .stale_suppressions
            .extend(input.suppressions.find_missing_reasons(input.graph));
    }
    input.results.suppression_count = input.suppressions.used_count();
    input.results.active_suppressions = input.suppressions.all_suppressions(input.graph);
}

/// Run the framework-convention detectors that share the resolved-graph and
/// dep-gate context: Next.js RSC directives, Vue/Svelte DI and components, and
/// the App Router route tree. Extracted from `find_dead_code_full` to keep that
/// orchestrator under the unit-size ceiling; each callee is individually
/// rule-gated.
struct FrameworkSpecificFindingsInput<'a> {
    graph: &'a ModuleGraph,
    modules: &'a [ModuleInfo],
    resolved_modules: &'a [ResolvedModule],
    config: &'a ResolvedConfig,
    workspaces: &'a [fallow_config::WorkspaceInfo],
    declared_deps: &'a FxHashSet<String>,
    public_api_entry_points: &'a FxHashSet<FileId>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    /// Mirror of `PostDetectionInput::collect_usages`; gates the LSP-only
    /// `react_component_intel` computation.
    collect_usages: bool,
    results: &'a mut AnalysisResults,
}

fn populate_framework_specific_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    populate_client_boundary_findings(input);
    populate_component_contract_findings(input);
    populate_react_health_findings(input);
    populate_nextjs_findings(input);
}

fn populate_client_boundary_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    populate_invalid_client_export_findings(input);
    populate_mixed_client_server_barrel_findings(input);
    populate_misplaced_directive_findings(input);
}

fn populate_component_contract_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    populate_unprovided_inject_findings(input);
    populate_unrendered_component_findings(input);
    populate_unused_component_prop_findings(input);
    populate_unused_component_emit_findings(
        input.graph,
        input.modules,
        input.config,
        input.declared_deps,
        input.line_offsets_by_file,
        input.results,
    );
    populate_unused_component_input_findings(
        input.graph,
        input.modules,
        input.config,
        input.declared_deps,
        input.line_offsets_by_file,
        input.results,
    );
    populate_unused_component_output_findings(
        input.graph,
        input.modules,
        input.config,
        input.declared_deps,
        input.line_offsets_by_file,
        input.results,
    );
    populate_unused_svelte_event_findings(
        input.graph,
        input.modules,
        input.config,
        input.declared_deps,
        input.line_offsets_by_file,
        input.results,
    );
    populate_unused_load_data_key_findings(input);
}

fn populate_react_health_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    populate_prop_drilling_findings(input);
    populate_thin_wrapper_findings(input);
    populate_render_fan_in(input);
    populate_react_component_intel(input);
    populate_duplicate_prop_shape_findings(input);
}

fn populate_nextjs_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    populate_nextjs_route_tree_findings(
        input.graph,
        input.config,
        input.workspaces,
        input.declared_deps,
        input.suppressions,
        input.results,
    );
}

/// Populate the descriptive component render fan-in metric (the component-graph
/// analogue of module fan-in). UNLIKE the prop-drilling / thin-wrapper detectors
/// this is NOT rule-gated: it is a descriptive blast-radius signal that runs
/// whenever React is declared (the dep gate lives inside
/// [`compute_render_fan_in`]). The field is `#[serde(skip)]` on
/// [`AnalysisResults`], so it never serializes under bare `fallow` / `audit`; it
/// is read in-process by the health vital-signs computation only.
fn populate_render_fan_in(input: &mut FrameworkSpecificFindingsInput<'_>) {
    input.results.render_fan_in = compute_render_fan_in(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        &input.config.root,
    );
}

/// Populate the descriptive per-component React intelligence carrier (render
/// sites, props, hooks). Like [`populate_render_fan_in`] this is NOT rule-gated:
/// it is a descriptive ambient-editor signal computed whenever React is declared
/// (the dep gate lives inside [`compute_react_component_intel`]). The field is
/// `#[serde(skip)]` on [`AnalysisResults`], so it never serializes under bare
/// `fallow` / `audit`; it is read in-process by the LSP code-lens / hover layer
/// only. Gated on `collect_usages` (the editor/LSP path) so bare `fallow` /
/// `audit` (the CI hot path) never pay for the render aggregation + prop-drilling
/// chain traversal that nothing on those paths reads.
fn populate_react_component_intel(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if !input.collect_usages {
        return;
    }
    input.results.react_component_intel = compute_react_component_intel(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        &input.config.root,
        input.line_offsets_by_file,
    );
}

/// Populate `unused_load_data_keys` when the rule is enabled. Gated on the
/// project declaring `@sveltejs/kit` inside the detector (see
/// [`find_unused_load_data_keys`]). Runs as a sequential populate because it
/// needs the run's `declared_deps` for the dep gate.
fn populate_unused_load_data_key_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.unused_load_data_keys == Severity::Off {
        return;
    }
    let result = find_unused_load_data_keys(
        input.graph,
        input.modules,
        input.declared_deps,
        input.suppressions,
        input.line_offsets_by_file,
        &input.config.root,
    );
    if result.global_abstain {
        input.results.unused_load_data_keys_global_abstain = true;
        tracing::debug!(
            "unused-load-data-key: abstained project-wide (a whole-object use of \
             page.data / $page.data was seen; any key could be read reflectively)"
        );
    }
    input.results.unused_load_data_keys = result
        .findings
        .into_iter()
        .map(UnusedLoadDataKeyFinding::with_actions)
        .collect();
}

/// Populate `invalid_client_exports` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_invalid_client_exports`]).
fn populate_invalid_client_export_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.invalid_client_export == Severity::Off {
        return;
    }
    input.results.invalid_client_exports = find_invalid_client_exports(
        input.graph,
        input.modules,
        input.declared_deps,
        input.suppressions,
        input.line_offsets_by_file,
    )
    .into_iter()
    .map(InvalidClientExportFinding::with_actions)
    .collect();
}

/// Populate `mixed_client_server_barrels` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_mixed_client_server_barrels`]).
fn populate_mixed_client_server_barrel_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.mixed_client_server_barrel == Severity::Off {
        return;
    }
    input.results.mixed_client_server_barrels = find_mixed_client_server_barrels(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        input.suppressions,
        input.line_offsets_by_file,
    )
    .into_iter()
    .map(MixedClientServerBarrelFinding::with_actions)
    .collect();
}

/// Populate `misplaced_directives` when the rule is enabled. Gated on the
/// project declaring `next` inside the detector (see
/// [`find_misplaced_directives`]).
fn populate_misplaced_directive_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.misplaced_directive == Severity::Off {
        return;
    }
    input.results.misplaced_directives = find_misplaced_directives(
        input.graph,
        input.modules,
        input.declared_deps,
        input.suppressions,
        input.line_offsets_by_file,
    )
    .into_iter()
    .map(MisplacedDirectiveFinding::with_actions)
    .collect();
}

/// Populate `unprovided_injects` when the rule is enabled. Gated on the project
/// declaring `vue` / `@vue/runtime-core` / `svelte` inside the detector (see
/// [`find_unprovided_injects`]).
fn populate_unprovided_inject_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.unprovided_injects == Severity::Off {
        return;
    }
    input.results.unprovided_injects = find_unprovided_injects(UnprovidedInjectInput {
        graph: input.graph,
        resolved_modules: input.resolved_modules,
        modules: input.modules,
        declared_deps: input.declared_deps,
        public_api_entry_points: input.public_api_entry_points,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
    })
    .into_iter()
    .map(UnprovidedInjectFinding::with_actions)
    .collect();
}

/// Populate `unrendered_components` when the rule is enabled. Gated on the
/// project declaring `vue` / `svelte` inside the detector (see
/// [`find_unrendered_components`]).
fn populate_unrendered_component_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.unrendered_components == Severity::Off {
        return;
    }
    input.results.unrendered_components = find_unrendered_components(
        input.graph,
        input.resolved_modules,
        input.modules,
        input.declared_deps,
        input.public_api_entry_points,
        input.suppressions,
    )
    .into_iter()
    .map(UnrenderedComponentFinding::with_actions)
    .collect();
    // Angular arm: a separate detection arm (selector-based) producing the SAME
    // finding kind / result type with `framework: "angular"`, appended to the
    // same vector. Gated on `@angular/core` inside the detector. Mirrors how the
    // Vue Options-API arm extends the existing rule (no new IssueKind).
    input.results.unrendered_components.extend(
        find_unrendered_angular_components(
            input.graph,
            input.modules,
            input.declared_deps,
            input.public_api_entry_points,
            input.line_offsets_by_file,
            input.suppressions,
        )
        .into_iter()
        .map(UnrenderedComponentFinding::with_actions),
    );
    // Lit arm: a registered custom element (`@customElement` /
    // `customElements.define`) rendered as a tag in no `html` template. SAME
    // finding kind / result type with `framework: "lit"`, gated on a Lit
    // dependency inside the detector. No new IssueKind.
    input.results.unrendered_components.extend(
        find_unrendered_lit_elements(&LitUnrenderedInput {
            graph: input.graph,
            modules: input.modules,
            declared_deps: input.declared_deps,
            public_api_entry_points: input.public_api_entry_points,
            line_offsets_by_file: input.line_offsets_by_file,
            suppressions: input.suppressions,
            root: &input.config.root,
        })
        .into_iter()
        .map(UnrenderedComponentFinding::with_actions),
    );
}

/// Populate `unused_component_props` when the rule is enabled. Gated on the
/// project declaring the matching framework dependency inside the detector (see
/// [`find_unused_component_props`]).
fn populate_unused_component_prop_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.unused_component_props == Severity::Off {
        return;
    }
    // Vue/Svelte arm: one component per SFC, flagged from `component_props`.
    let sfc = find_unused_component_props(
        input.graph,
        input.modules,
        input.declared_deps,
        input.line_offsets_by_file,
        input.config.unused_component_props_ignore.as_ref(),
    );
    input.results.unused_component_props_exempted += sfc.exempted;
    input.results.unused_component_props = sfc
        .findings
        .into_iter()
        .map(UnusedComponentPropFinding::with_actions)
        .collect();

    append_react_unused_component_prop_findings(input);
    retain_unsuppressed_unused_component_prop_findings(input);
}

fn append_react_unused_component_prop_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    // React/Preact arm: another producer of the SAME finding kind, emitting into
    // the same vector. Gated on `react` / `react-dom` / `next` / `preact` inside
    // the producer.
    let react = find_unused_react_props(
        input.graph,
        input.modules,
        input.declared_deps,
        input.line_offsets_by_file,
        input.config.unused_component_props_ignore.as_ref(),
    );
    input.results.unused_component_props_exempted += react.exempted;
    if react.components_scanned > 0 {
        // Observability: make a silent dep-gate or silent abstain visible (a
        // scanned-but-zero-finding run is a clean bill, not a no-op). Surfaced at
        // info level so `RUST_LOG=fallow_core=info` shows it.
        tracing::info!(
            components_scanned = react.components_scanned,
            unused_props = react.findings.len(),
            "React detected, {} component(s) scanned for unused props",
            react.components_scanned
        );
    }
    input.results.unused_component_props.extend(
        react
            .findings
            .into_iter()
            .map(UnusedComponentPropFinding::with_actions),
    );
}

fn retain_unsuppressed_unused_component_prop_findings(
    input: &mut FrameworkSpecificFindingsInput<'_>,
) {
    // Inline-suppression filter over ALL arms: a `// fallow-ignore-next-line
    // unused-component-prop` above the prop (or a file-level
    // `// fallow-ignore-file unused-component-prop`) drops the finding. The
    // finding's `path` is the absolute graph node path, so it maps directly to a
    // FileId for the line-anchored suppression check.
    let path_to_id = graph_file_ids_by_path(input.graph);
    input.results.unused_component_props.retain(|finding| {
        !path_line_is_suppressed(
            &path_to_id,
            input.suppressions,
            finding.prop.path.as_path(),
            finding.prop.line,
            IssueKind::UnusedComponentProp,
        )
    });
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

/// Populate `prop_drilling_chains` when the rule is enabled. The rule defaults to
/// `off` (opt-in health signal), so this is dormant by default: the located
/// per-chain records and the small capped health penalty appear only once the
/// user sets `prop-drilling` to `warn`/`error`. Gated on the project declaring
/// `react` / `react-dom` / `next` / `preact` inside the detector (see
/// [`find_prop_drilling_chains`]).
fn populate_prop_drilling_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.prop_drilling == Severity::Off {
        return;
    }
    input.results.prop_drilling_chains = collect_prop_drilling_findings(input);

    retain_unsuppressed_prop_drilling_findings(input);
}

fn collect_prop_drilling_findings(
    input: &FrameworkSpecificFindingsInput<'_>,
) -> Vec<PropDrillingChainFinding> {
    let scan = find_prop_drilling_chains(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        input.line_offsets_by_file,
    );
    if scan.components_scanned > 0 {
        // Observability: a scanned-but-zero run is a clean bill, not a no-op.
        tracing::info!(
            components_scanned = scan.components_scanned,
            prop_drilling_chains = scan.chains.len(),
            "React detected, {} component(s) scanned for prop drilling",
            scan.components_scanned
        );
    }
    scan.chains
        .into_iter()
        .map(PropDrillingChainFinding::with_actions)
        .collect()
}

fn retain_unsuppressed_prop_drilling_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    // Inline-suppression filter: a `// fallow-ignore-next-line prop-drilling`
    // above the source prop declaration (or a file-level
    // `// fallow-ignore-file prop-drilling` on the source file) drops the chain.
    // The source hop's `file` is the absolute graph node path, so it maps to a
    // FileId for the line-anchored check.
    let path_to_id = graph_file_ids_by_path(input.graph);
    input.results.prop_drilling_chains.retain(|finding| {
        let Some(source) = finding.chain.hops.first() else {
            return true;
        };
        !path_line_is_suppressed(
            &path_to_id,
            input.suppressions,
            source.file.as_path(),
            source.line,
            IssueKind::PropDrilling,
        )
    });
}

/// Populate `thin_wrappers` when the rule is enabled. The rule defaults to `off`
/// (opt-in health signal), so this is dormant by default: the located
/// per-wrapper records appear only once the user sets `thin-wrapper` to
/// `warn`/`error`. Gated on the project declaring `react` / `react-dom` / `next`
/// / `preact` inside the detector (see [`find_thin_wrappers`]).
fn populate_thin_wrapper_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.thin_wrapper == Severity::Off {
        return;
    }
    input.results.thin_wrappers = collect_thin_wrapper_findings(input);

    retain_unsuppressed_thin_wrapper_findings(input);
}

fn collect_thin_wrapper_findings(
    input: &FrameworkSpecificFindingsInput<'_>,
) -> Vec<ThinWrapperFinding> {
    let scan = find_thin_wrappers(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        input.line_offsets_by_file,
    );
    if scan.components_scanned > 0 {
        // Observability: a scanned-but-zero run is a clean bill, not a no-op.
        tracing::info!(
            components_scanned = scan.components_scanned,
            thin_wrappers = scan.wrappers.len(),
            "React detected, {} component(s) scanned for thin wrappers",
            scan.components_scanned
        );
    }
    scan.wrappers
        .into_iter()
        .map(ThinWrapperFinding::with_actions)
        .collect()
}

fn retain_unsuppressed_thin_wrapper_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    // Inline-suppression filter: a `// fallow-ignore-next-line thin-wrapper`
    // above the wrapper component definition (or a file-level
    // `// fallow-ignore-file thin-wrapper` on the wrapper's file) drops it. The
    // wrapper's `file` is the absolute graph node path, so it maps to a FileId
    // for the line-anchored check.
    let path_to_id = graph_file_ids_by_path(input.graph);
    input.results.thin_wrappers.retain(|finding| {
        !path_line_is_suppressed(
            &path_to_id,
            input.suppressions,
            finding.wrapper.file.as_path(),
            finding.wrapper.line,
            IssueKind::ThinWrapper,
        )
    });
}

/// Populate `duplicate_prop_shapes` when the rule is enabled. The rule defaults
/// to `off` (opt-in structural-refactor health signal), so this is dormant by
/// default: the located per-component records appear only once the user sets
/// `duplicate-prop-shape` to `warn`/`error`. Gated on the project declaring
/// `react` / `react-dom` / `next` / `preact` inside the detector (see
/// [`find_duplicate_prop_shapes`]).
///
/// Multi-file suppress model (copied from route-collision): a per-member finding
/// is dropped by a line-level (`// fallow-ignore-next-line duplicate-prop-shape`
/// at its component definition) or a file-level
/// (`// fallow-ignore-file duplicate-prop-shape`) suppress, but the suppressed
/// member STILL appears in its siblings' `sharing_components`, because the
/// `sharing_components` roster is built at emit time (before this filter) and
/// the group is real regardless of suppression.
fn populate_duplicate_prop_shape_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.duplicate_prop_shape == Severity::Off {
        return;
    }
    let scan = find_duplicate_prop_shapes(
        input.graph,
        input.modules,
        input.declared_deps,
        input.line_offsets_by_file,
    );
    if scan.components_scanned > 0 {
        // Observability: a scanned-but-zero run is a clean bill, not a no-op.
        tracing::info!(
            components_scanned = scan.components_scanned,
            duplicate_prop_shapes = scan.groups.len(),
            "React detected, {} component(s) scanned for duplicate prop shapes",
            scan.components_scanned
        );
    }
    input.results.duplicate_prop_shapes = scan
        .groups
        .into_iter()
        .map(DuplicatePropShapeFinding::with_actions)
        .collect();

    // Inline-suppression filter: a line-level marker above the component
    // definition or a file-level marker on the component's file drops THIS
    // member; its slot in the siblings' `sharing_components` is unaffected (the
    // roster was built at emit time).
    let path_to_id = graph_file_ids_by_path(input.graph);
    input.results.duplicate_prop_shapes.retain(|finding| {
        !path_line_is_suppressed(
            &path_to_id,
            input.suppressions,
            finding.shape.file.as_path(),
            finding.shape.line,
            IssueKind::DuplicatePropShape,
        )
    });
}

fn graph_file_ids_by_path(graph: &ModuleGraph) -> FxHashMap<&std::path::Path, FileId> {
    graph
        .modules
        .iter()
        .map(|node| (node.path.as_path(), node.file_id))
        .collect()
}

fn path_line_is_suppressed(
    path_to_id: &FxHashMap<&std::path::Path, FileId>,
    suppressions: &SuppressionContext<'_>,
    path: &std::path::Path,
    line: u32,
    kind: IssueKind,
) -> bool {
    let Some(&file_id) = path_to_id.get(path) else {
        return false;
    };
    suppressions.is_suppressed(file_id, line, kind)
        || suppressions.is_file_suppressed(file_id, kind)
}

/// Populate `unused_component_inputs` when the rule is enabled. Gated on the
/// project declaring `@angular/core` inside the detector (see
/// [`find_unused_component_inputs`]).
fn populate_unused_component_input_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unused_component_inputs == Severity::Off {
        return;
    }
    results.unused_component_inputs =
        find_unused_component_inputs(graph, modules, declared_deps, line_offsets_by_file)
            .into_iter()
            .map(UnusedComponentInputFinding::with_actions)
            .collect();
}

/// Populate `unused_component_outputs` when the rule is enabled. Gated on the
/// project declaring `@angular/core` inside the detector (see
/// [`find_unused_component_outputs`]).
fn populate_unused_component_output_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unused_component_outputs == Severity::Off {
        return;
    }
    results.unused_component_outputs =
        find_unused_component_outputs(graph, modules, declared_deps, line_offsets_by_file)
            .into_iter()
            .map(UnusedComponentOutputFinding::with_actions)
            .collect();
}

/// Populate `unused_svelte_events` when the rule is enabled. Gated on the
/// project declaring `svelte` inside the detector (see
/// [`find_unused_svelte_events`]).
fn populate_unused_svelte_event_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    results: &mut AnalysisResults,
) {
    if config.rules.unused_svelte_events == Severity::Off {
        return;
    }
    results.unused_svelte_events =
        find_unused_svelte_events(graph, modules, declared_deps, line_offsets_by_file)
            .into_iter()
            .map(UnusedSvelteEventFinding::with_actions)
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

struct ParallelDeadCodeDetectorResults {
    unused_files: Vec<UnusedFileFinding>,
    export_results: AnalysisResults,
    member_results: AnalysisResults,
    dependency_results: AnalysisResults,
    unresolved_imports: Vec<UnresolvedImportFinding>,
    duplicate_exports: Vec<DuplicateExportFinding>,
    boundary_violations: Vec<BoundaryViolationFinding>,
    boundary_coverage_violations: Vec<BoundaryCoverageViolationFinding>,
    boundary_call_violations: Vec<BoundaryCallViolationFinding>,
    policy_violations: Vec<PolicyViolationFinding>,
    circular_dependencies: Vec<CircularDependencyFinding>,
    re_export_cycles: Vec<ReExportCycleFinding>,
    export_usages: Vec<crate::results::ExportUsage>,
}

impl ParallelDeadCodeDetectorResults {
    fn into_analysis_results(self) -> AnalysisResults {
        AnalysisResults {
            unused_files: self.unused_files,
            unused_exports: self.export_results.unused_exports,
            unused_types: self.export_results.unused_types,
            private_type_leaks: self.export_results.private_type_leaks,
            stale_suppressions: self.export_results.stale_suppressions,
            unused_enum_members: self.member_results.unused_enum_members,
            unused_class_members: self.member_results.unused_class_members,
            unused_store_members: self.member_results.unused_store_members,
            unused_dependencies: self.dependency_results.unused_dependencies,
            unused_dev_dependencies: self.dependency_results.unused_dev_dependencies,
            unused_optional_dependencies: self.dependency_results.unused_optional_dependencies,
            unlisted_dependencies: self.dependency_results.unlisted_dependencies,
            type_only_dependencies: self.dependency_results.type_only_dependencies,
            test_only_dependencies: self.dependency_results.test_only_dependencies,
            unresolved_imports: self.unresolved_imports,
            duplicate_exports: self.duplicate_exports,
            boundary_violations: self.boundary_violations,
            boundary_coverage_violations: self.boundary_coverage_violations,
            boundary_call_violations: self.boundary_call_violations,
            policy_violations: self.policy_violations,
            circular_dependencies: self.circular_dependencies,
            re_export_cycles: self.re_export_cycles,
            export_usages: self.export_usages,
            ..AnalysisResults::default()
        }
    }
}

fn run_parallel_dead_code_detectors(input: DeadCodeDetectorInput<'_>) -> AnalysisResults {
    collect_parallel_dead_code_detector_results(input).into_analysis_results()
}

fn collect_parallel_dead_code_detector_results(
    input: DeadCodeDetectorInput<'_>,
) -> ParallelDeadCodeDetectorResults {
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

    ParallelDeadCodeDetectorResults {
        unused_files,
        export_results,
        member_results,
        dependency_results,
        unresolved_imports,
        duplicate_exports,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        circular_dependencies,
        re_export_cycles,
        export_usages,
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
            run_member_detectors(MemberDetectorInput {
                graph: input.graph,
                resolved_modules: input.resolved_modules,
                modules: input.modules,
                config: input.config,
                suppressions: input.suppressions,
                line_offsets_by_file: input.line_offsets_by_file,
                user_class_members: input.user_class_members,
                public_api_entry_points: input.public_api_entry_points,
                declared_deps: input.declared_deps,
            })
        },
        || {
            run_dependency_detectors(DependencyDetectorInput {
                graph: input.graph,
                pkg: input.pkg,
                config: input.config,
                plugin_result: input.plugin_result,
                workspaces: input.workspaces,
                resolved_modules: input.resolved_modules,
                line_offsets_by_file: input.line_offsets_by_file,
            })
        },
    )
}

fn run_import_and_duplicate_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (Vec<UnresolvedImportFinding>, Vec<DuplicateExportFinding>) {
    rayon::join(
        || {
            run_unresolved_import_detector(UnresolvedImportDetectorInput {
                resolved_modules: input.resolved_modules,
                config: input.config,
                suppressions: input.suppressions,
                virtual_prefixes: input.virtual_prefixes,
                generated_patterns: input.generated_patterns,
                generated_type_prefixes: input.generated_type_prefixes,
                line_offsets_by_file: input.line_offsets_by_file,
            })
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
        || run_boundary_detectors(input),
        || run_cycle_and_usage_detectors(input),
    )
}

fn run_boundary_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (Vec<BoundaryViolationFinding>, BoundaryAuxResults) {
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
                input.declared_deps,
                input.suppressions,
                input.line_offsets_by_file,
            )
        },
    )
}

fn run_cycle_and_usage_detectors(
    input: DeadCodeDetectorInput<'_>,
) -> (
    Vec<CircularDependencyFinding>,
    (Vec<ReExportCycleFinding>, Vec<crate::results::ExportUsage>),
) {
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
                || run_re_export_cycle_detector(input.graph, input.config, input.suppressions),
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
        &security::SecurityRankingInput {
            graph: ctx.graph,
            modules: ctx.modules,
            line_offsets_by_file: ctx.line_offsets_by_file,
            declared_deps: ctx.declared_deps,
            request_receivers: ctx.request_receivers,
            boundary_crossings: &boundary_crossings,
        },
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
    if export_rules_are_disabled(config) {
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
    populate_unused_export_findings(&mut results, config, exports);
    populate_unused_type_findings(&mut results, config, graph, modules, types);
    populate_private_type_leak_findings(
        &mut results,
        graph,
        modules,
        config,
        suppressions,
        line_offsets_by_file,
    );
    populate_expected_stale_suppressions(&mut results, config, stale_expected);
    results
}

fn export_rules_are_disabled(config: &ResolvedConfig) -> bool {
    config.rules.unused_exports == Severity::Off
        && config.rules.unused_types == Severity::Off
        && config.rules.private_type_leaks == Severity::Off
}

fn populate_unused_export_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    exports: Vec<UnusedExport>,
) {
    if config.rules.unused_exports == Severity::Off {
        return;
    }
    results.unused_exports = exports
        .into_iter()
        .map(UnusedExportFinding::with_actions)
        .collect();
}

fn populate_unused_type_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    types: Vec<UnusedExport>,
) {
    if config.rules.unused_types == Severity::Off {
        return;
    }
    let mut typed = types;
    suppress_signature_backing_types(&mut typed, graph, modules);
    results.unused_types = typed
        .into_iter()
        .map(UnusedTypeFinding::with_actions)
        .collect();
}

fn populate_private_type_leak_findings(
    results: &mut AnalysisResults,
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) {
    if config.rules.private_type_leaks == Severity::Off {
        return;
    }
    results.private_type_leaks =
        find_private_type_leaks(graph, modules, config, suppressions, line_offsets_by_file)
            .into_iter()
            .map(PrivateTypeLeakFinding::with_actions)
            .collect();
}

fn populate_expected_stale_suppressions(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    stale_expected: Vec<StaleSuppression>,
) {
    if config.rules.stale_suppressions != Severity::Off {
        results.stale_suppressions.extend(stale_expected);
    } else if config.rules.require_suppression_reason != Severity::Off {
        results
            .stale_suppressions
            .extend(stale_expected.into_iter().filter(|s| s.missing_reason));
    }
}

#[derive(Clone, Copy)]
struct MemberDetectorInput<'a> {
    graph: &'a ModuleGraph,
    resolved_modules: &'a [ResolvedModule],
    modules: &'a [ModuleInfo],
    config: &'a ResolvedConfig,
    suppressions: &'a crate::suppress::SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    user_class_members: &'a [fallow_config::UsedClassMemberRule],
    public_api_entry_points: &'a FxHashSet<FileId>,
    declared_deps: &'a FxHashSet<String>,
}

fn run_member_detectors(input: MemberDetectorInput<'_>) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    let store_members_active = store_member_rule_is_active(input.config, input.declared_deps);
    if member_rules_are_disabled(input.config, store_members_active) {
        return results;
    }

    let member_results = find_unused_members_with_public_api_entry_points(UnusedMemberScanInput {
        graph: input.graph,
        resolved_modules: input.resolved_modules,
        modules: input.modules,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        user_class_member_allowlist: input.user_class_members,
        ignore_decorators: &input.config.ignore_decorators,
        public_api_entry_points: input.public_api_entry_points,
        lit_active: input.declared_deps.contains("lit")
            || input.declared_deps.contains("lit-element")
            || input.declared_deps.contains("@lit/reactive-element"),
    });
    populate_unused_enum_member_findings(&mut results, input.config, member_results.enum_members);
    populate_unused_class_member_findings(&mut results, input.config, member_results.class_members);
    populate_unused_store_member_findings(
        &mut results,
        store_members_active,
        member_results.store_members,
    );
    results
}

fn member_rules_are_disabled(config: &ResolvedConfig, store_members_active: bool) -> bool {
    config.rules.unused_enum_members == Severity::Off
        && config.rules.unused_class_members == Severity::Off
        && !store_members_active
}

fn store_member_rule_is_active(config: &ResolvedConfig, declared_deps: &FxHashSet<String>) -> bool {
    // Store-member detection activates only when Pinia is a declared dependency,
    // so an unrelated user `defineStore`-named helper in a non-Pinia project
    // never fires. The harvest is intentionally loose at extraction time; this
    // is the activation boundary.
    config.rules.unused_store_members != Severity::Off
        && (declared_deps.contains("pinia") || declared_deps.contains("@pinia/nuxt"))
}

fn populate_unused_enum_member_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    enum_members: Vec<UnusedMember>,
) {
    if config.rules.unused_enum_members == Severity::Off {
        return;
    }
    results.unused_enum_members = enum_members
        .into_iter()
        .map(UnusedEnumMemberFinding::with_actions)
        .collect();
}

fn populate_unused_class_member_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    class_members: Vec<UnusedMember>,
) {
    if config.rules.unused_class_members == Severity::Off {
        return;
    }
    results.unused_class_members = class_members
        .into_iter()
        .map(UnusedClassMemberFinding::with_actions)
        .collect();
}

fn populate_unused_store_member_findings(
    results: &mut AnalysisResults,
    store_members_active: bool,
    store_members: Vec<UnusedMember>,
) {
    if !store_members_active {
        return;
    }
    results.unused_store_members = store_members
        .into_iter()
        .map(UnusedStoreMemberFinding::with_actions)
        .collect();
}

#[derive(Clone, Copy)]
struct DependencyDetectorInput<'a> {
    graph: &'a ModuleGraph,
    pkg: Option<&'a PackageJson>,
    config: &'a ResolvedConfig,
    plugin_result: Option<&'a crate::plugins::AggregatedPluginResult>,
    workspaces: &'a [fallow_config::WorkspaceInfo],
    resolved_modules: &'a [ResolvedModule],
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn run_dependency_detectors(input: DependencyDetectorInput<'_>) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    let Some(pkg) = input.pkg else {
        return results;
    };

    populate_unused_dependency_findings(input, pkg, &mut results);
    populate_unlisted_dependency_findings(input, pkg, &mut results);
    populate_type_only_dependency_findings(input, pkg, &mut results);
    populate_test_only_dependency_findings(input, pkg, &mut results);
    results
}

fn populate_unlisted_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if input.config.rules.unlisted_dependencies != Severity::Off {
        results.unlisted_dependencies = find_unlisted_dependencies(UnlistedDependencyInput {
            graph: input.graph,
            pkg,
            config: input.config,
            workspaces: input.workspaces,
            plugin_result: input.plugin_result,
            resolved_modules: input.resolved_modules,
            line_offsets_by_file: input.line_offsets_by_file,
        })
        .into_iter()
        .map(UnlistedDependencyFinding::with_actions)
        .collect();
    }
}

fn populate_type_only_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if input.config.production {
        results.type_only_dependencies =
            find_type_only_dependencies(input.graph, pkg, input.config, input.workspaces)
                .into_iter()
                .map(TypeOnlyDependencyFinding::with_actions)
                .collect();
    }
}

fn populate_test_only_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if !input.config.production && input.config.rules.test_only_dependencies != Severity::Off {
        results.test_only_dependencies =
            find_test_only_dependencies(input.graph, pkg, input.config, input.workspaces)
                .into_iter()
                .map(TestOnlyDependencyFinding::with_actions)
                .collect();
    }
}

/// Populate the unused-dependency family (prod / dev / optional) on `results`,
/// each gated on its own rule severity. The three collections share one
/// `find_unused_dependencies` computation, so they are populated together.
#[expect(
    deprecated,
    reason = "ADR-008 deprecates detector helpers for external callers; core orchestration still calls them internally"
)]
fn populate_unused_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if unused_dependency_rules_are_disabled(input.config) {
        return;
    }

    let (deps, dev_deps, optional_deps) = find_unused_dependencies(
        input.graph,
        pkg,
        input.config,
        input.plugin_result,
        input.workspaces,
    );
    populate_unused_prod_dependency_findings(results, input.config, deps);
    populate_unused_dev_dependency_findings(results, input.config, dev_deps);
    populate_unused_optional_dependency_findings(results, input.config, optional_deps);
}

fn unused_dependency_rules_are_disabled(config: &ResolvedConfig) -> bool {
    config.rules.unused_dependencies == Severity::Off
        && config.rules.unused_dev_dependencies == Severity::Off
        && config.rules.unused_optional_dependencies == Severity::Off
}

fn populate_unused_prod_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_dependencies == Severity::Off {
        return;
    }
    results.unused_dependencies = deps
        .into_iter()
        .map(UnusedDependencyFinding::with_actions)
        .collect();
}

fn populate_unused_dev_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    dev_deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_dev_dependencies == Severity::Off {
        return;
    }
    results.unused_dev_dependencies = dev_deps
        .into_iter()
        .map(UnusedDevDependencyFinding::with_actions)
        .collect();
}

fn populate_unused_optional_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    optional_deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_optional_dependencies == Severity::Off {
        return;
    }
    results.unused_optional_dependencies = optional_deps
        .into_iter()
        .map(UnusedOptionalDependencyFinding::with_actions)
        .collect();
}

#[derive(Clone, Copy)]
struct UnresolvedImportDetectorInput<'a> {
    resolved_modules: &'a [ResolvedModule],
    config: &'a ResolvedConfig,
    suppressions: &'a crate::suppress::SuppressionContext<'a>,
    virtual_prefixes: &'a [&'a str],
    generated_patterns: &'a [&'a str],
    generated_type_prefixes: &'a [&'a str],
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn run_unresolved_import_detector(
    input: UnresolvedImportDetectorInput<'_>,
) -> Vec<UnresolvedImportFinding> {
    if input.config.rules.unresolved_imports == Severity::Off || input.resolved_modules.is_empty() {
        return Vec::new();
    }
    find_unresolved_imports(
        input.resolved_modules,
        input.config,
        input.suppressions,
        input.virtual_prefixes,
        input.generated_patterns,
        input.generated_type_prefixes,
        input.line_offsets_by_file,
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

    // Exercises the public-API entry-point fallback (`resolve_entry_via_scoped_canonical`)
    // for the intra-project-symlink case it exists to handle: a module whose
    // discovered (raw) path goes through a symlinked directory, so its raw path
    // differs from the canonicalized entry-point path. The common no-symlink path
    // is covered by the byte-identical integration corpus; this pins the residual
    // branch that the raw-map lookup cannot reach.
    #[cfg(unix)]
    #[cfg_attr(miri, ignore)]
    #[test]
    fn scoped_canonical_matches_module_reached_through_symlink() {
        use fallow_types::discover::FileId;

        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let real_file = real_dir.join("mod.ts");
        std::fs::write(&real_file, "export const x = 1;\n").unwrap();
        // `link/` resolves to `real/`, so the module discovered at `link/mod.ts`
        // canonicalizes to `real/mod.ts`.
        let link_dir = dir.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();

        let module_raw_path = link_dir.join("mod.ts");
        let canonical_entry = dunce::canonicalize(&real_file).unwrap();
        let package_root = dir.path();

        // The symlinked module under the package is found by canonical match.
        let candidates = [(module_raw_path.as_path(), FileId(7))];
        assert_eq!(
            super::match_canonical_entry_under_package(
                candidates.iter().copied(),
                package_root,
                &canonical_entry,
            ),
            Some(FileId(7)),
        );

        // A candidate outside the package_root is filtered out, even on a match.
        let outside_root = dir.path().join("other-package");
        assert_eq!(
            super::match_canonical_entry_under_package(
                candidates.iter().copied(),
                &outside_root,
                &canonical_entry,
            ),
            None,
        );

        // A non-matching canonical target yields no entry point.
        let unrelated = dunce::canonicalize(dir.path()).unwrap().join("nope.ts");
        assert_eq!(
            super::match_canonical_entry_under_package(
                candidates.iter().copied(),
                package_root,
                &unrelated,
            ),
            None,
        );
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
                unused_component_inputs: Severity::Off,
                unused_component_outputs: Severity::Off,
                unused_svelte_events: Severity::Off,
                unused_server_actions: Severity::Off,
                unused_load_data_keys: Severity::Off,
                prop_drilling: Severity::Off,
                thin_wrapper: Severity::Off,
                duplicate_prop_shape: Severity::Off,
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
            let mut graph = ModuleGraph::build(&resolved, &entry_points, &files);
            graph.modules[0].exports = vec![ExportSymbol {
                name: ExportName::Named("myExport".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
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
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);
            let config = make_config_with_rules(RulesConfig::default());

            let results = find_dead_code(&graph, &config);
            assert!(results.unused_exports.is_empty());
        }

        #[test]
        #[expect(
            clippy::too_many_lines,
            reason = "test fixture; linear setup/assert, length is not a maintainability concern"
        )]
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
                    semantic_facts: Box::default(),
                    whole_object_uses: Box::default(),
                    has_cjs_exports: false,
                    has_angular_component_template_url: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                    namespace_object_aliases: vec![],
                    exported_factory_returns: Box::default(),
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
                package_path_references: Box::default(),
                member_accesses: vec![],
                semantic_facts: Box::default(),
                whole_object_uses: Box::default(),
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
                exported_factory_returns: Box::default(),
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
                inline_server_action_exports: Vec::new(),
                di_key_sites: Vec::new(),
                has_dynamic_provide: false,
                referenced_import_bindings: Vec::new(),
                component_props: Vec::new(),
                has_props_attrs_fallthrough: false,
                has_define_expose: false,
                has_define_model: false,
                has_unharvestable_props: false,
                component_emits: Vec::new(),
                angular_inputs: Vec::new(),
                angular_outputs: Vec::new(),
                has_unharvestable_emits: false,
                has_dynamic_emit: false,
                has_emit_whole_object_use: false,
                load_return_keys: Vec::new(),
                has_unharvestable_load: false,
                has_load_data_whole_use: false,
                has_page_data_store_whole_use: false,
                component_functions: Vec::new(),
                react_props: Vec::new(),
                hook_uses: Vec::new(),
                render_edges: Vec::new(),
                svelte_dispatched_events: Vec::new(),
                svelte_listened_events: Vec::new(),
                angular_component_selectors: Vec::new(),
                registered_custom_elements: Vec::new(),
                used_custom_element_tags: Vec::new(),
                angular_used_selectors: Vec::new(),
                angular_entry_component_refs: Vec::new(),
                has_dynamic_component_render: false,
                has_dynamic_dispatch: false,
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
