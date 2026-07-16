//! Typed data contract and builder for `fallow viz`.
//!
//! The CLI runs one project analysis (dead code + duplication + complexity)
//! through [`crate::session::AnalysisSession`] and hands the retained
//! artifacts to [`build_viz_data`]. The resulting [`VizData`] is embedded as
//! JSON in the self-contained interactive HTML the `viz` command writes.
//!
//! The contract is engine-owned so the graph internals never leak past the
//! engine boundary: everything the frontend needs is resolved to file
//! indices, relative paths, and plain counts here.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::Serialize;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::DiscoveredFile;
use fallow_types::duplicates::DuplicationReport;
use fallow_types::extract::{FunctionComplexity, ModuleInfo};
use fallow_types::results::AnalysisResults;

use crate::module_graph::RetainedModuleGraph;

/// Maximum spotlighted functions serialized per file.
const MAX_FUNCTIONS_PER_FILE: usize = 5;
/// A function is spotlighted when any of these floors are crossed.
const FUNCTION_CYCLOMATIC_FLOOR: u16 = 8;
const FUNCTION_COGNITIVE_FLOOR: u16 = 10;
const FUNCTION_HOOK_FLOOR: u16 = 5;
/// A file counts as a complexity hotspot at or above this cyclomatic score.
const HOTSPOT_CYCLOMATIC_FLOOR: u16 = 10;
/// Maximum characters of clone-fragment preview shipped per clone group.
const CLONE_PREVIEW_MAX_CHARS: usize = 480;
/// Maximum lines of clone-fragment preview shipped per clone group.
const CLONE_PREVIEW_MAX_LINES: usize = 10;
/// Edge flag bit: every import of this edge is type-only.
const EDGE_FLAG_TYPE_ONLY: u32 = 1;

/// Everything [`build_viz_data`] needs from one project analysis run.
pub struct VizBuildInput<'a> {
    /// Dead-code analysis results (unused files/exports, cycles, boundaries).
    pub results: &'a AnalysisResults,
    /// Retained module graph for edges, entry points, and export counts.
    pub graph: &'a RetainedModuleGraph,
    /// Parsed modules with complexity data, when retained.
    pub modules: Option<&'a [ModuleInfo]>,
    /// Discovered source files, in `FileId` order.
    pub files: &'a [DiscoveredFile],
    /// Duplication report from the same session.
    pub duplication: &'a DuplicationReport,
    /// Discovered monorepo workspaces.
    pub workspaces: &'a [WorkspaceInfo],
    /// Resolved config (project root + boundary zones).
    pub config: &'a ResolvedConfig,
}

/// Serialized payload embedded in the viz HTML.
#[derive(Serialize)]
pub struct VizData {
    /// Project display name (root directory basename).
    pub root: String,
    /// One entry per analyzed source file, indexed by position.
    pub files: Vec<VizFile>,
    /// Import edges as `[from, to, flags]` file-index pairs.
    /// `flags` bit 0 marks an edge whose imports are all type-only.
    pub edges: Vec<[u32; 3]>,
    /// Project-wide totals for the header stat boxes.
    pub summary: VizSummary,
    /// Discovered workspaces; `VizFile.workspace` indexes into this.
    pub workspaces: Vec<VizWorkspace>,
    /// Boundary zones; `VizFile.zone` and violations index into this.
    pub zones: Vec<VizZone>,
    /// Circular-dependency cycles as file-index lists.
    pub cycles: Vec<Vec<u32>>,
    /// Clone groups; `VizFile.clone_groups` indexes into this.
    pub clones: Vec<VizCloneGroup>,
    /// Boundary violations resolved to file indices.
    pub violations: Vec<VizViolation>,
}

/// One analyzed source file.
#[derive(Serialize)]
pub struct VizFile {
    /// Root-relative path with forward slashes.
    pub path: String,
    /// File size in bytes (treemap area).
    pub size: u64,
    /// Dead-code status classification.
    pub status: VizFileStatus,
    /// Number of exports declared by the file.
    pub export_count: u16,
    /// Number of exports (values + types) reported unused.
    pub unused_export_count: u16,
    /// Whether the file is an entry point.
    pub is_entry: bool,
    /// Number of files importing this file.
    pub importer_count: u16,
    /// Number of files this file imports.
    pub import_count: u16,
    /// Index into `VizData.workspaces`, if the file belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u16>,
    /// Index into `VizData.zones`, if the file matches a boundary zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<u16>,
    /// Names of unused exports (for actionable tooltips).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unused_exports: Vec<String>,
    /// Number of functions parsed in the file.
    pub fn_count: u16,
    /// Highest cyclomatic complexity of any function in the file.
    pub max_cyclomatic: u16,
    /// Highest cognitive complexity of any function in the file.
    pub max_cognitive: u16,
    /// Total React hook calls across the file's functions.
    pub react_hooks: u16,
    /// Deepest JSX nesting across the file's functions.
    pub jsx_depth: u16,
    /// Spotlighted complex functions (capped, floor-gated).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<VizFunction>,
    /// Duplicated lines in this file across all clone groups.
    pub dup_lines: u32,
    /// Indices into `VizData.clones` this file participates in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clone_groups: Vec<u32>,
    /// Whether the file participates in any circular dependency.
    pub in_cycle: bool,
}

/// Dead-code status of a file, ordered by severity in the frontend.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VizFileStatus {
    /// No findings.
    Clean,
    /// Live file with one or more unused exports.
    HasUnusedExports,
    /// Entire file is unreachable.
    Unused,
    /// Configured or detected entry point.
    EntryPoint,
}

/// One spotlighted complex function inside a file.
#[derive(Serialize)]
pub struct VizFunction {
    /// Function name, or `<anonymous>`.
    pub name: String,
    /// 1-based start line.
    pub line: u32,
    /// McCabe cyclomatic complexity.
    pub cyclomatic: u16,
    /// SonarSource cognitive complexity.
    pub cognitive: u16,
    /// Body line count.
    pub lines: u32,
    /// React hook calls made directly in the body.
    pub hooks: u16,
    /// Deepest JSX nesting in the body.
    pub jsx_depth: u16,
    /// Props destructured from the first parameter.
    pub props: u16,
}

/// Project-wide totals for the header stat boxes.
#[derive(Serialize)]
pub struct VizSummary {
    /// Total analyzed files.
    pub total_files: usize,
    /// Total bytes across analyzed files.
    pub total_size: u64,
    /// Total import edges.
    pub total_edges: usize,
    /// Fully unused files.
    pub unused_files: usize,
    /// Unused exports (values + types).
    pub unused_exports: usize,
    /// Unused exported types.
    pub unused_types: usize,
    /// Unused dependencies (prod + dev + optional).
    pub unused_deps: usize,
    /// Imports that resolve to nothing.
    pub unresolved_imports: usize,
    /// Circular dependency cycles.
    pub circular_deps: usize,
    /// Clone groups detected.
    pub clone_groups: usize,
    /// Total duplicated lines across clone groups.
    pub duplicated_lines: usize,
    /// Boundary violations.
    pub boundary_violations: usize,
    /// Files at or above the complexity hotspot floor.
    pub hotspot_files: usize,
}

/// One discovered workspace.
#[derive(Serialize)]
pub struct VizWorkspace {
    /// Package name.
    pub name: String,
    /// Root-relative workspace root.
    pub root: String,
}

/// One configured boundary zone.
#[derive(Serialize)]
pub struct VizZone {
    /// Zone name from the boundaries config.
    pub name: String,
    /// Number of files classified into this zone.
    pub files: u32,
}

/// One clone group resolved to file indices.
#[derive(Serialize)]
pub struct VizCloneGroup {
    /// Lines per duplicated block.
    pub lines: usize,
    /// Tokens per duplicated block.
    pub tokens: usize,
    /// Where the duplicated block appears.
    pub instances: Vec<VizCloneInstance>,
    /// Truncated source preview of the duplicated block.
    pub preview: String,
}

/// One location of a duplicated block.
#[derive(Serialize)]
pub struct VizCloneInstance {
    /// File index into `VizData.files`.
    pub file: u32,
    /// 1-based start line.
    pub start_line: u32,
    /// 1-based end line.
    pub end_line: u32,
}

/// One boundary violation resolved to file indices.
#[derive(Serialize)]
pub struct VizViolation {
    /// Importing file index.
    pub from: u32,
    /// Imported file index.
    pub to: u32,
    /// Index into `VizData.zones` for the importing file's zone.
    pub from_zone: u16,
    /// Index into `VizData.zones` for the imported file's zone.
    pub to_zone: u16,
    /// 1-based line of the offending import.
    pub line: u32,
    /// Raw import specifier.
    pub specifier: String,
}

/// Build the viz payload from one project analysis run.
#[must_use]
pub fn build_viz_data(input: &VizBuildInput<'_>) -> VizData {
    let root = &input.config.root;
    let index = FileIndex::new(input.files);
    let workspaces = build_workspaces(input.workspaces, root);
    let (zones, zone_by_file) = classify_zones(input, &index);
    let (clones, clone_groups_by_file, dup_lines_by_file) = build_clones(input.duplication, &index);
    let cycles = build_cycles(input.results, &index);
    let violations = build_violations(input.results, &zones, &index);

    let files = build_files(
        input,
        &index,
        &FilePropertyMaps {
            zone_by_file: &zone_by_file,
            clone_groups_by_file: &clone_groups_by_file,
            dup_lines_by_file: &dup_lines_by_file,
            cycles: &cycles,
        },
    );

    let summary = build_summary(input, &files, &clones);

    VizData {
        root: display_root(root),
        files,
        edges: build_edges(input.graph, &index),
        summary,
        workspaces,
        zones,
        cycles,
        clones,
        violations,
    }
}

/// Maps absolute paths to dense viz file indices in `FileId` order.
struct FileIndex<'a> {
    ordered: Vec<&'a DiscoveredFile>,
    by_path: FxHashMap<&'a Path, u32>,
    by_file_id: FxHashMap<u32, u32>,
}

impl<'a> FileIndex<'a> {
    fn new(files: &'a [DiscoveredFile]) -> Self {
        let mut ordered: Vec<&DiscoveredFile> = files.iter().collect();
        ordered.sort_by_key(|f| f.id.0);
        let mut by_path = FxHashMap::default();
        let mut by_file_id = FxHashMap::default();
        for (i, f) in ordered.iter().enumerate() {
            let idx = clamp_u32(i);
            by_path.insert(f.path.as_path(), idx);
            by_file_id.insert(f.id.0, idx);
        }
        Self {
            ordered,
            by_path,
            by_file_id,
        }
    }

    fn index_of_path(&self, path: &Path) -> Option<u32> {
        self.by_path.get(path).copied()
    }

    fn index_of_file_id(&self, file_id: u32) -> Option<u32> {
        self.by_file_id.get(&file_id).copied()
    }
}

/// Per-file lookup maps threaded into [`build_files`].
struct FilePropertyMaps<'a> {
    zone_by_file: &'a FxHashMap<u32, u16>,
    clone_groups_by_file: &'a FxHashMap<u32, Vec<u32>>,
    dup_lines_by_file: &'a FxHashMap<u32, u32>,
    cycles: &'a [Vec<u32>],
}

fn display_root(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn build_workspaces(workspaces: &[WorkspaceInfo], root: &Path) -> Vec<VizWorkspace> {
    workspaces
        .iter()
        .map(|ws| VizWorkspace {
            name: ws.name.clone(),
            root: relative_path(&ws.root, root),
        })
        .collect()
}

fn workspace_index_for(path: &Path, workspaces: &[WorkspaceInfo]) -> Option<u16> {
    let mut best: Option<(usize, usize)> = None;
    for (i, ws) in workspaces.iter().enumerate() {
        if path.starts_with(&ws.root) {
            let depth = ws.root.components().count();
            if best.is_none_or(|(_, d)| depth > d) {
                best = Some((i, depth));
            }
        }
    }
    best.map(|(i, _)| clamp_u16(i))
}

fn classify_zones(
    input: &VizBuildInput<'_>,
    index: &FileIndex<'_>,
) -> (Vec<VizZone>, FxHashMap<u32, u16>) {
    let boundaries = &input.config.boundaries;
    let mut zones: Vec<VizZone> = boundaries
        .zones
        .iter()
        .map(|z| VizZone {
            name: z.name.clone(),
            files: 0,
        })
        .collect();
    let name_to_index: FxHashMap<&str, u16> = boundaries
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| (z.name.as_str(), clamp_u16(i)))
        .collect();

    let mut zone_by_file = FxHashMap::default();
    if zones.is_empty() {
        return (zones, zone_by_file);
    }

    for (i, file) in index.ordered.iter().enumerate() {
        let rel = relative_path(&file.path, &input.config.root);
        if let Some(zone_name) = boundaries.classify_zone(&rel)
            && let Some(&zone_idx) = name_to_index.get(zone_name)
        {
            zone_by_file.insert(clamp_u32(i), zone_idx);
            zones[zone_idx as usize].files += 1;
        }
    }

    (zones, zone_by_file)
}

type CloneMaps = (
    Vec<VizCloneGroup>,
    FxHashMap<u32, Vec<u32>>,
    FxHashMap<u32, u32>,
);

fn build_clones(duplication: &DuplicationReport, index: &FileIndex<'_>) -> CloneMaps {
    let mut clones = Vec::new();
    let mut groups_by_file: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    let mut dup_lines_by_file: FxHashMap<u32, u32> = FxHashMap::default();

    for group in &duplication.clone_groups {
        let instances: Vec<VizCloneInstance> = group
            .instances
            .iter()
            .filter_map(|inst| {
                index
                    .index_of_path(&inst.file)
                    .map(|file| VizCloneInstance {
                        file,
                        start_line: clamp_u32(inst.start_line),
                        end_line: clamp_u32(inst.end_line),
                    })
            })
            .collect();
        if instances.len() < 2 {
            continue;
        }

        let group_idx = clamp_u32(clones.len());
        for inst in &instances {
            let entry = groups_by_file.entry(inst.file).or_default();
            if entry.last() != Some(&group_idx) {
                entry.push(group_idx);
            }
            *dup_lines_by_file.entry(inst.file).or_default() +=
                inst.end_line.saturating_sub(inst.start_line) + 1;
        }

        let preview = group
            .instances
            .first()
            .map(|inst| truncate_preview(&inst.fragment))
            .unwrap_or_default();

        clones.push(VizCloneGroup {
            lines: group.line_count,
            tokens: group.token_count,
            instances,
            preview,
        });
    }

    (clones, groups_by_file, dup_lines_by_file)
}

fn truncate_preview(fragment: &str) -> String {
    let mut out = String::new();
    for (i, line) in fragment.lines().enumerate() {
        if i >= CLONE_PREVIEW_MAX_LINES || out.len() + line.len() > CLONE_PREVIEW_MAX_CHARS {
            out.push('\u{2026}');
            break;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn build_cycles(results: &AnalysisResults, index: &FileIndex<'_>) -> Vec<Vec<u32>> {
    results
        .circular_dependencies
        .iter()
        .filter_map(|cd| {
            let ids: Vec<u32> = cd
                .cycle
                .files
                .iter()
                .filter_map(|p| index.index_of_path(p))
                .collect();
            (ids.len() == cd.cycle.files.len()).then_some(ids)
        })
        .collect()
}

fn build_violations(
    results: &AnalysisResults,
    zones: &[VizZone],
    index: &FileIndex<'_>,
) -> Vec<VizViolation> {
    let name_to_index: FxHashMap<&str, u16> = zones
        .iter()
        .enumerate()
        .map(|(i, z)| (z.name.as_str(), clamp_u16(i)))
        .collect();

    results
        .boundary_violations
        .iter()
        .filter_map(|finding| {
            let v = &finding.violation;
            let from = index.index_of_path(&v.from_path)?;
            let to = index.index_of_path(&v.to_path)?;
            let from_zone = *name_to_index.get(v.from_zone.as_str())?;
            let to_zone = *name_to_index.get(v.to_zone.as_str())?;
            Some(VizViolation {
                from,
                to,
                from_zone,
                to_zone,
                line: v.line,
                specifier: v.import_specifier.clone(),
            })
        })
        .collect()
}

fn build_edges(graph: &RetainedModuleGraph, index: &FileIndex<'_>) -> Vec<[u32; 3]> {
    let graph = graph.as_graph();
    let mut edges = Vec::with_capacity(graph.edge_count());
    for node in &graph.modules {
        let Some(source) = index.index_of_file_id(node.file_id.0) else {
            continue;
        };
        for (target_id, all_type_only, _span) in graph.outgoing_edge_summaries(node.file_id) {
            let Some(target) = index.index_of_file_id(target_id.0) else {
                continue;
            };
            let flags = if all_type_only {
                EDGE_FLAG_TYPE_ONLY
            } else {
                0
            };
            edges.push([source, target, flags]);
        }
    }
    edges
}

/// Complexity aggregates for one file, folded from its parsed functions.
#[derive(Default)]
struct ComplexityRollup {
    fn_count: u16,
    max_cyclomatic: u16,
    max_cognitive: u16,
    react_hooks: u16,
    jsx_depth: u16,
    functions: Vec<VizFunction>,
}

fn rollup_complexity(functions: &[FunctionComplexity]) -> ComplexityRollup {
    let mut rollup = ComplexityRollup {
        fn_count: clamp_u16(functions.len()),
        ..ComplexityRollup::default()
    };
    for f in functions {
        rollup.max_cyclomatic = rollup.max_cyclomatic.max(f.cyclomatic);
        rollup.max_cognitive = rollup.max_cognitive.max(f.cognitive);
        rollup.react_hooks = rollup.react_hooks.saturating_add(f.react_hook_count);
        rollup.jsx_depth = rollup.jsx_depth.max(f.react_jsx_max_depth);
    }

    let mut spotlighted: Vec<&FunctionComplexity> = functions
        .iter()
        .filter(|f| {
            f.cyclomatic >= FUNCTION_CYCLOMATIC_FLOOR
                || f.cognitive >= FUNCTION_COGNITIVE_FLOOR
                || f.react_hook_count >= FUNCTION_HOOK_FLOOR
        })
        .collect();
    spotlighted.sort_by(|a, b| {
        b.cyclomatic
            .cmp(&a.cyclomatic)
            .then(b.cognitive.cmp(&a.cognitive))
    });
    spotlighted.truncate(MAX_FUNCTIONS_PER_FILE);
    rollup.functions = spotlighted
        .into_iter()
        .map(|f| VizFunction {
            name: f.name.clone(),
            line: f.line,
            cyclomatic: f.cyclomatic,
            cognitive: f.cognitive,
            lines: f.line_count,
            hooks: f.react_hook_count,
            jsx_depth: f.react_jsx_max_depth,
            props: f.react_prop_count,
        })
        .collect();
    rollup
}

fn build_files(
    input: &VizBuildInput<'_>,
    index: &FileIndex<'_>,
    maps: &FilePropertyMaps<'_>,
) -> Vec<VizFile> {
    let graph = input.graph.as_graph();
    let unused_file_paths: rustc_hash::FxHashSet<&Path> = input
        .results
        .unused_files
        .iter()
        .map(|f| f.file.path.as_path())
        .collect();

    let mut unused_exports_by_file: FxHashMap<&Path, Vec<String>> = FxHashMap::default();
    for export in &input.results.unused_exports {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }
    for export in &input.results.unused_types {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }

    let mut complexity_by_file_id: FxHashMap<u32, ComplexityRollup> = FxHashMap::default();
    if let Some(modules) = input.modules {
        for module in modules {
            if !module.complexity.is_empty() {
                complexity_by_file_id
                    .insert(module.file_id.0, rollup_complexity(&module.complexity));
            }
        }
    }

    let mut in_cycle = vec![false; index.ordered.len()];
    for cycle in maps.cycles {
        for &idx in cycle {
            if let Some(slot) = in_cycle.get_mut(idx as usize) {
                *slot = true;
            }
        }
    }

    index
        .ordered
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let viz_idx = clamp_u32(i);
            let node_idx = file.id.0 as usize;
            let node = graph.modules.get(node_idx);
            let is_entry = node.is_some_and(|n| n.is_entry_point());
            let export_count = node.map_or(0, |n| clamp_u16(n.exports.len()));
            let import_count = clamp_u16(graph.edges_for(file.id).len());
            let importer_count = clamp_u16(input.graph.direct_importer_count(file.id));

            let unused_export_names = unused_exports_by_file
                .remove(file.path.as_path())
                .unwrap_or_default();
            let unused_export_count = clamp_u16(unused_export_names.len());

            let status = if unused_file_paths.contains(file.path.as_path()) {
                VizFileStatus::Unused
            } else if unused_export_count > 0 {
                VizFileStatus::HasUnusedExports
            } else if is_entry {
                VizFileStatus::EntryPoint
            } else {
                VizFileStatus::Clean
            };

            let complexity = complexity_by_file_id.remove(&file.id.0).unwrap_or_default();

            VizFile {
                path: relative_path(&file.path, &input.config.root),
                size: file.size_bytes,
                status,
                export_count,
                unused_export_count,
                is_entry,
                importer_count,
                import_count,
                workspace: workspace_index_for(&file.path, input.workspaces),
                zone: maps.zone_by_file.get(&viz_idx).copied(),
                unused_exports: unused_export_names,
                fn_count: complexity.fn_count,
                max_cyclomatic: complexity.max_cyclomatic,
                max_cognitive: complexity.max_cognitive,
                react_hooks: complexity.react_hooks,
                jsx_depth: complexity.jsx_depth,
                functions: complexity.functions,
                dup_lines: maps.dup_lines_by_file.get(&viz_idx).copied().unwrap_or(0),
                clone_groups: maps
                    .clone_groups_by_file
                    .get(&viz_idx)
                    .cloned()
                    .unwrap_or_default(),
                in_cycle: in_cycle[i],
            }
        })
        .collect()
}

fn build_summary(
    input: &VizBuildInput<'_>,
    files: &[VizFile],
    clones: &[VizCloneGroup],
) -> VizSummary {
    let results = input.results;
    VizSummary {
        total_files: files.len(),
        total_size: files.iter().map(|f| f.size).sum(),
        total_edges: input.graph.edge_count(),
        unused_files: results.unused_files.len(),
        unused_exports: results.unused_exports.len() + results.unused_types.len(),
        unused_types: results.unused_types.len(),
        unused_deps: results.unused_dependencies.len()
            + results.unused_dev_dependencies.len()
            + results.unused_optional_dependencies.len(),
        unresolved_imports: results.unresolved_imports.len(),
        circular_deps: results.circular_dependencies.len(),
        clone_groups: clones.len(),
        duplicated_lines: clones.iter().map(|c| c.lines * c.instances.len()).sum(),
        boundary_violations: results.boundary_violations.len(),
        hotspot_files: files
            .iter()
            .filter(|f| f.max_cyclomatic >= HOTSPOT_CYCLOMATIC_FLOOR)
            .count(),
    }
}

fn clamp_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn clamp_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
