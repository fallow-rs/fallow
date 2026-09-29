//! Package cycle detector: finds dependency cycles between workspace
//! packages.
//!
//! Each workspace package is a node. A resolved import from a file in one
//! package to a file in another package is an edge. Declared `package.json`
//! dependencies are not edges. Type-only imports are edges too, because they
//! can still force a build order; each hop reports whether it is type-only.
//! Imports from test, spec, story, fixture and tooling config files are not
//! edges, because those files are not part of the package build.
//!
//! A `// fallow-ignore-next-line package-cycle` comment removes one import
//! or re-export statement from the package graph. Other statements in the
//! same file that import the same module stay. A `// fallow-ignore-file package-cycle` comment,
//! or a per-file override that sets `package-cycle` to `off`, removes every
//! import in that file. A hop disappears when all of its imports are
//! removed, so a cycle stays while one import keeps each hop.
//!
//! A package is labelled by its name. When two or more workspace packages
//! share a name, the label also carries the project-relative package root,
//! so that the output, the baseline keys and the audit keys name one
//! package. The listing of one group of packages can stop early; each cycle
//! in such a group has `group_truncated` set.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::WorkspaceInfo;
use fallow_types::extract::ImportedName;
use fallow_types::output_dead_code::PackageCycleFinding;
use fallow_types::results::{PackageCycle, PackageCycleEdge};

use super::predicates::{is_config_file, is_test_or_spec_file};
use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{ImportedSymbol, ModuleGraph};
use crate::resolve::{ResolvedModule, ResolvedReExport};
use crate::suppress::{IssueKind, SuppressionContext};

/// Maximum number of cycles reported for one strongly connected group of
/// packages. Matches the file-level cycle cap.
const MAX_CYCLES_PER_SCC: usize = 20;

/// Maximum number of search steps for one strongly connected group. Keeps
/// the enumeration bounded on a dense package graph.
const MAX_SEARCH_STEPS_PER_SCC: usize = 200_000;

/// Limits for the cycle listing of one strongly connected group.
#[derive(Clone, Copy)]
struct SearchLimits {
    max_cycles: usize,
    max_steps: usize,
}

const DEFAULT_LIMITS: SearchLimits = SearchLimits {
    max_cycles: MAX_CYCLES_PER_SCC,
    max_steps: MAX_SEARCH_STEPS_PER_SCC,
};

/// The cycles of one strongly connected group, and whether the listing
/// stopped before it found every cycle.
#[derive(Debug, PartialEq, Eq)]
struct GroupCycles {
    cycles: Vec<Vec<usize>>,
    truncated: bool,
}

/// One cross-package import, before suppression.
struct CrossImport {
    from_pkg: usize,
    to_pkg: usize,
    file_id: FileId,
    target: FileId,
    line: u32,
    col: u32,
    type_only: bool,
}

/// Find every package cycle in the workspace graph.
///
/// `rule_off_for` tells whether a per-file override turns the rule off for
/// a file. The imports of such a file are not package edges.
#[expect(
    clippy::too_many_arguments,
    reason = "each analysis input stays a separate borrowed argument"
)]
pub fn find_package_cycles(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    workspaces: &[WorkspaceInfo],
    project_root: &Path,
    line_offsets_map: &LineOffsetsMap<'_>,
    suppressions: &SuppressionContext<'_>,
    rule_off_for: &dyn Fn(&Path) -> bool,
) -> Vec<PackageCycleFinding> {
    if workspaces.len() < 2 {
        return Vec::new();
    }
    let packages = PackageIndex::new(workspaces, project_root);
    let imports = collect_cross_imports(
        graph,
        resolved_modules,
        &packages,
        line_offsets_map,
        rule_off_for,
    );
    if imports.is_empty() {
        return Vec::new();
    }

    // Only imports on a hop that can be part of a cycle consult the
    // suppressions. A suppression on an import outside every cycle stays
    // unused, so the stale-suppression check reports it.
    let candidate_sccs = strongly_connected_packages(packages.len(), imports.iter());
    let imports: Vec<CrossImport> = imports
        .into_iter()
        .filter(|import| {
            !candidate_sccs.same_group(import.from_pkg, import.to_pkg)
                || !is_import_suppressed(import, suppressions)
        })
        .collect();

    let hops = HopTable::new(graph, packages.len(), imports);
    let sccs = strongly_connected_packages(packages.len(), hops.imports.iter());
    let mut findings: Vec<PackageCycle> = Vec::new();
    for group in sccs.groups() {
        let listed = enumerate_cycles(&group, &hops.successors, DEFAULT_LIMITS);
        findings.extend(
            listed
                .cycles
                .iter()
                .map(|cycle| hops.package_cycle(cycle, listed.truncated, &packages, graph)),
        );
    }
    findings.sort_by(|a, b| {
        a.length
            .cmp(&b.length)
            .then_with(|| a.packages.cmp(&b.packages))
    });
    findings
        .into_iter()
        .map(PackageCycleFinding::with_actions)
        .collect()
}

fn is_import_suppressed(import: &CrossImport, suppressions: &SuppressionContext<'_>) -> bool {
    suppressions.is_file_suppressed(import.file_id, IssueKind::PackageCycle)
        || suppressions.is_suppressed(import.file_id, import.line, IssueKind::PackageCycle)
}

/// Workspace packages ordered by the string order of their labels, with a
/// root lookup.
///
/// The node id is the position in label order, so a cycle that is rotated to
/// start at its smallest id starts at its lexicographically smallest label,
/// as `PackageCycle::packages` documents. For a name that one package owns,
/// the label is the name, so this is also name order.
struct PackageIndex<'a> {
    /// Workspaces in node order. The node id is the position in this list.
    ordered: Vec<&'a WorkspaceInfo>,
    /// Output label per node: the name, or `name (root)` when the name is
    /// not unique.
    labels: Vec<String>,
    by_root: FxHashMap<&'a Path, usize>,
}

impl<'a> PackageIndex<'a> {
    fn new(workspaces: &'a [WorkspaceInfo], project_root: &Path) -> Self {
        let mut name_counts: FxHashMap<&str, usize> = FxHashMap::default();
        for workspace in workspaces {
            *name_counts.entry(workspace.name.as_str()).or_default() += 1;
        }
        let mut labelled: Vec<(String, &WorkspaceInfo)> = workspaces
            .iter()
            .map(|workspace| {
                let label = if name_counts[workspace.name.as_str()] > 1 {
                    format!(
                        "{} ({})",
                        workspace.name,
                        relative_root(&workspace.root, project_root)
                    )
                } else {
                    workspace.name.clone()
                };
                (label, workspace)
            })
            .collect();
        // The root breaks a tie only when two workspaces share a name and a
        // root, which discovery does not produce.
        labelled.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.root.cmp(&b.1.root)));
        let (labels, ordered): (Vec<String>, Vec<&WorkspaceInfo>) = labelled.into_iter().unzip();
        let mut by_root = FxHashMap::default();
        for (index, workspace) in ordered.iter().enumerate() {
            by_root.entry(workspace.root.as_path()).or_insert(index);
        }
        Self {
            ordered,
            labels,
            by_root,
        }
    }

    fn len(&self) -> usize {
        self.ordered.len()
    }

    fn label(&self, index: usize) -> &str {
        &self.labels[index]
    }

    fn root(&self, index: usize) -> &Path {
        &self.ordered[index].root
    }

    /// The package that owns `path`: the workspace with the longest root
    /// prefix. Files outside every workspace have no package.
    fn package_of(&self, path: &Path) -> Option<usize> {
        path.ancestors()
            .skip(1)
            .find_map(|ancestor| self.by_root.get(ancestor).copied())
    }
}

/// The package root relative to the project root, with forward slashes.
/// A package at the project root is `.`.
fn relative_root(root: &Path, project_root: &Path) -> String {
    let relative = root.strip_prefix(project_root).unwrap_or(root);
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        ".".to_owned()
    } else {
        text
    }
}

/// Whether an import from `path` can force a build order between packages.
///
/// Test, spec, story and fixture files and tooling config files are not part
/// of the package build output, so their imports are not package edges. A
/// package commonly imports a sibling package in its tests only; counting that
/// import would report a cycle that no build sees. The match runs on the path
/// relative to the package root, so a parent directory named `test` does not
/// make every file a test file.
fn is_build_source(path: &Path, package_root: &Path) -> bool {
    let relative = path.strip_prefix(package_root).unwrap_or(path);
    !is_test_or_spec_file(relative) && !is_config_file(relative)
}

fn collect_cross_imports(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    packages: &PackageIndex<'_>,
    line_offsets_map: &LineOffsetsMap<'_>,
    rule_off_for: &dyn Fn(&Path) -> bool,
) -> Vec<CrossImport> {
    let module_packages: Vec<Option<usize>> = graph
        .modules
        .iter()
        .map(|module| packages.package_of(&module.path))
        .collect();
    let re_exports_of: FxHashMap<FileId, &[ResolvedReExport]> = resolved_modules
        .iter()
        .filter(|module| !module.re_exports.is_empty())
        .map(|module| (module.file_id, module.re_exports.as_slice()))
        .collect();

    let mut imports = Vec::new();
    for (index, module) in graph.modules.iter().enumerate() {
        let Some(from_pkg) = module_packages[index] else {
            continue;
        };
        if !is_build_source(&module.path, packages.root(from_pkg)) {
            continue;
        }
        // Resolve the per-file rule once, and only for a file that has a
        // cross-package import.
        let mut rule_off: Option<bool> = None;
        for (target, symbols) in graph.outgoing_edge_symbols(module.file_id) {
            let Some(to_pkg) = module_packages.get(target.0 as usize).copied().flatten() else {
                continue;
            };
            if to_pkg == from_pkg {
                continue;
            }
            if *rule_off.get_or_insert_with(|| rule_off_for(&module.path)) {
                break;
            }
            let re_exports = re_exports_of
                .get(&module.file_id)
                .copied()
                .unwrap_or_default();
            for statement in edge_statements(target, symbols, re_exports) {
                let (line, col) = statement.start.map_or((1, 0), |start| {
                    byte_offset_to_line_col(line_offsets_map, module.file_id, start)
                });
                imports.push(CrossImport {
                    from_pkg,
                    to_pkg,
                    file_id: module.file_id,
                    target,
                    line,
                    col,
                    type_only: statement.type_only,
                });
            }
        }
    }
    imports
}

/// One import or re-export statement on a graph edge.
#[derive(Debug, PartialEq, Eq)]
struct EdgeStatement {
    /// Byte offset of the statement, or `None` when the graph has no span
    /// for it, for example for an `import.meta.glob` pattern.
    start: Option<u32>,
    /// Whether every binding of the statement is type-only.
    type_only: bool,
}

/// Split one graph edge into its import and re-export statements.
///
/// A graph edge holds every import from one file to one target file. Two
/// statements that import the same target, for example `import type { Y }`
/// and `import { y }`, are one edge. Each statement is a separate import of
/// the package graph, so a suppression, the example import and the type-only
/// flag of a hop apply to one statement.
///
/// The graph stores a re-export with an empty span, so the span comes from
/// the resolved re-exports of the file that point at `target`. The statement
/// span is used, so a suppression above a multi-line `export { .. } from`
/// statement covers the whole statement. A symbol that has no span and no
/// matching re-export becomes one statement without a span.
fn edge_statements(
    target: FileId,
    symbols: &[ImportedSymbol],
    re_exports: &[ResolvedReExport],
) -> Vec<EdgeStatement> {
    let mut statements: Vec<EdgeStatement> = Vec::new();
    let mut add = |start: Option<u32>, type_only: bool| match statements
        .iter_mut()
        .find(|statement| statement.start == start)
    {
        Some(statement) => statement.type_only &= type_only,
        None => statements.push(EdgeStatement { start, type_only }),
    };
    if symbols.is_empty() {
        add(None, false);
        return statements;
    }
    let mut unspanned_side_effects: Vec<bool> = Vec::new();
    for symbol in symbols {
        let span = symbol.import_span;
        if span.end > span.start {
            add(Some(span.start), symbol.is_type_only);
        } else if matches!(symbol.imported_name, ImportedName::SideEffect) {
            unspanned_side_effects.push(symbol.is_type_only);
        } else {
            add(None, symbol.is_type_only);
        }
    }
    if unspanned_side_effects.is_empty() {
        return statements;
    }
    // The graph adds one side-effect symbol with an empty span for each
    // re-export that resolves to `target`.
    let mut matched = 0usize;
    for re_export in re_exports
        .iter()
        .filter(|re_export| re_export.target.internal_file_id() == Some(target))
    {
        let statement = re_export.info.statement_span;
        let span = if statement.end > statement.start {
            statement
        } else {
            re_export.info.span
        };
        let start = (span.end > span.start).then_some(span.start);
        add(start, re_export.info.is_type_only);
        matched += 1;
    }
    for &type_only in unspanned_side_effects.iter().skip(matched) {
        add(None, type_only);
    }
    statements
}

/// The example import and type-only flag for each package hop.
struct HopTable {
    imports: Vec<CrossImport>,
    /// Sorted, deduplicated successors per package.
    successors: Vec<Vec<usize>>,
    /// Index into `imports` of the example import for each hop.
    example: FxHashMap<(usize, usize), usize>,
    /// Hops that have at least one runtime import.
    runtime_hops: FxHashSet<(usize, usize)>,
}

impl HopTable {
    fn new(graph: &ModuleGraph, package_count: usize, imports: Vec<CrossImport>) -> Self {
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); package_count];
        let mut example: FxHashMap<(usize, usize), usize> = FxHashMap::default();
        let mut runtime_hops: FxHashSet<(usize, usize)> = FxHashSet::default();
        let sort_key = |import: &CrossImport| {
            (
                import.type_only,
                graph.modules[import.file_id.0 as usize].path.as_path(),
                import.line,
                import.col,
            )
        };
        for (index, import) in imports.iter().enumerate() {
            let hop = (import.from_pkg, import.to_pkg);
            successors[import.from_pkg].push(import.to_pkg);
            if !import.type_only {
                runtime_hops.insert(hop);
            }
            example
                .entry(hop)
                .and_modify(|current| {
                    if sort_key(import) < sort_key(&imports[*current]) {
                        *current = index;
                    }
                })
                .or_insert(index);
        }
        for list in &mut successors {
            list.sort_unstable();
            list.dedup();
        }
        Self {
            imports,
            successors,
            example,
            runtime_hops,
        }
    }

    fn package_cycle(
        &self,
        cycle: &[usize],
        group_truncated: bool,
        packages: &PackageIndex<'_>,
        graph: &ModuleGraph,
    ) -> PackageCycle {
        let edges = cycle
            .iter()
            .enumerate()
            .map(|(position, &from_pkg)| {
                let to_pkg = cycle[(position + 1) % cycle.len()];
                let import = &self.imports[self.example[&(from_pkg, to_pkg)]];
                PackageCycleEdge {
                    from_package: packages.label(from_pkg).to_owned(),
                    to_package: packages.label(to_pkg).to_owned(),
                    path: module_path(graph, import.file_id),
                    target_path: module_path(graph, import.target),
                    line: import.line,
                    col: import.col,
                    type_only: !self.runtime_hops.contains(&(from_pkg, to_pkg)),
                }
            })
            .collect();
        PackageCycle {
            packages: cycle
                .iter()
                .map(|&index| packages.label(index).to_owned())
                .collect(),
            package_roots: cycle
                .iter()
                .map(|&index| packages.root(index).to_path_buf())
                .collect(),
            length: cycle.len(),
            edges,
            group_truncated,
        }
    }
}

fn module_path(graph: &ModuleGraph, file_id: FileId) -> PathBuf {
    graph.modules[file_id.0 as usize].path.clone()
}

/// Strongly connected groups of packages with two or more members.
struct PackageSccs {
    /// Group id per package, or `None` for a package outside every cycle.
    group_of: Vec<Option<usize>>,
    group_count: usize,
}

impl PackageSccs {
    fn same_group(&self, a: usize, b: usize) -> bool {
        matches!((self.group_of[a], self.group_of[b]), (Some(x), Some(y)) if x == y)
    }

    /// Members of each group, sorted by package id.
    fn groups(&self) -> Vec<Vec<usize>> {
        let mut groups: Vec<Vec<usize>> = vec![Vec::new(); self.group_count];
        for (package, group) in self.group_of.iter().enumerate() {
            if let Some(group) = group {
                groups[*group].push(package);
            }
        }
        groups
    }
}

/// Tarjan's algorithm over the package graph, iterative to stay safe on
/// deep chains.
fn strongly_connected_packages<'a>(
    package_count: usize,
    imports: impl Iterator<Item = &'a CrossImport>,
) -> PackageSccs {
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); package_count];
    for import in imports {
        successors[import.from_pkg].push(import.to_pkg);
    }
    for list in &mut successors {
        list.sort_unstable();
        list.dedup();
    }

    let mut index = vec![usize::MAX; package_count];
    let mut lowlink = vec![0; package_count];
    let mut on_stack = vec![false; package_count];
    let mut stack: Vec<usize> = Vec::new();
    let mut group_of: Vec<Option<usize>> = vec![None; package_count];
    let mut group_count = 0;
    let mut counter = 0;

    for start in 0..package_count {
        if index[start] != usize::MAX {
            continue;
        }
        let mut frames: Vec<(usize, usize)> = vec![(start, 0)];
        index[start] = counter;
        lowlink[start] = counter;
        counter += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some(frame) = frames.last_mut() {
            let node = frame.0;
            if let Some(&child) = successors[node].get(frame.1) {
                frame.1 += 1;
                if index[child] == usize::MAX {
                    index[child] = counter;
                    lowlink[child] = counter;
                    counter += 1;
                    stack.push(child);
                    on_stack[child] = true;
                    frames.push((child, 0));
                } else if on_stack[child] {
                    lowlink[node] = lowlink[node].min(index[child]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                lowlink[parent] = lowlink[parent].min(lowlink[node]);
            }
            if lowlink[node] != index[node] {
                continue;
            }
            let mut members = Vec::new();
            while let Some(member) = stack.pop() {
                on_stack[member] = false;
                members.push(member);
                if member == node {
                    break;
                }
            }
            if members.len() >= 2 {
                for member in members {
                    group_of[member] = Some(group_count);
                }
                group_count += 1;
            }
        }
    }

    PackageSccs {
        group_of,
        group_count,
    }
}

/// Elementary cycles inside one strongly connected group, shortest first.
///
/// Iterative deepening: for each length, a depth-limited search from every
/// member finds the cycles that start at that member and only visit members
/// with a larger id. So each cycle is found once, rotated to start at its
/// smallest package id (the smallest label). The search stops after
/// `limits.max_cycles` cycles or `limits.max_steps` steps. `truncated` is
/// true when the group has more cycles than the list holds, or when the step
/// limit stopped the search before it explored every path. The search finds
/// one cycle more than the cap to tell a full list from a cut one.
fn enumerate_cycles(
    group: &[usize],
    successors: &[Vec<usize>],
    limits: SearchLimits,
) -> GroupCycles {
    let members: FxHashSet<usize> = group.iter().copied().collect();
    let mut cycles = Vec::new();
    let mut steps = 0usize;
    let mut stopped = false;
    'lengths: for length in 2..=group.len() {
        for &start in group {
            let mut search = CycleSearch {
                start,
                length,
                members: &members,
                successors,
                path: vec![start],
                cycles: &mut cycles,
                steps: &mut steps,
                limits,
            };
            if search.run() == SearchEnd::Stopped {
                stopped = true;
                break 'lengths;
            }
        }
    }
    let truncated = cycles.len() > limits.max_cycles || stopped;
    cycles.truncate(limits.max_cycles);
    GroupCycles { cycles, truncated }
}

/// How one depth-limited search ended.
#[derive(PartialEq, Eq)]
enum SearchEnd {
    /// Every path from the start member was explored.
    Complete,
    /// A limit stopped the search while paths were still open.
    Stopped,
}

struct CycleSearch<'a> {
    start: usize,
    length: usize,
    members: &'a FxHashSet<usize>,
    successors: &'a [Vec<usize>],
    path: Vec<usize>,
    cycles: &'a mut Vec<Vec<usize>>,
    steps: &'a mut usize,
    limits: SearchLimits,
}

impl CycleSearch<'_> {
    fn run(&mut self) -> SearchEnd {
        let mut frames: Vec<usize> = vec![0];
        while let Some(next) = frames.last_mut() {
            if self.cycles.len() > self.limits.max_cycles || *self.steps >= self.limits.max_steps {
                return SearchEnd::Stopped;
            }
            let node = self.path[self.path.len() - 1];
            let Some(&child) = self.successors[node].get(*next) else {
                frames.pop();
                self.path.pop();
                continue;
            };
            *next += 1;
            *self.steps += 1;
            if child == self.start {
                if self.path.len() == self.length {
                    self.cycles.push(self.path.clone());
                }
                continue;
            }
            if child < self.start
                || !self.members.contains(&child)
                || self.path.len() >= self.length
                || self.path.contains(&child)
            {
                continue;
            }
            self.path.push(child);
            frames.push(0);
        }
        SearchEnd::Complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn successors(edges: &[(usize, usize)], count: usize) -> Vec<Vec<usize>> {
        let mut list = vec![Vec::new(); count];
        for &(from, to) in edges {
            list[from].push(to);
        }
        for entry in &mut list {
            entry.sort_unstable();
        }
        list
    }

    #[test]
    fn enumerates_shortest_cycles_first_and_rotates_to_smallest() {
        // 0 -> 1 -> 2 -> 0 and 1 -> 0.
        let succ = successors(&[(0, 1), (1, 2), (2, 0), (1, 0)], 3);
        let listed = enumerate_cycles(&[0, 1, 2], &succ, DEFAULT_LIMITS);
        assert_eq!(
            listed,
            GroupCycles {
                cycles: vec![vec![0, 1], vec![0, 1, 2]],
                truncated: false,
            }
        );
    }

    fn complete_graph(count: usize) -> (Vec<usize>, Vec<Vec<usize>>) {
        let edges: Vec<(usize, usize)> = (0..count)
            .flat_map(|a| (0..count).filter(move |&b| b != a).map(move |b| (a, b)))
            .collect();
        ((0..count).collect(), successors(&edges, count))
    }

    #[test]
    fn caps_cycles_per_group_and_marks_the_group_truncated() {
        // A complete graph on 8 nodes has far more than 20 cycles.
        let (group, succ) = complete_graph(8);
        let listed = enumerate_cycles(&group, &succ, DEFAULT_LIMITS);
        assert_eq!(listed.cycles.len(), MAX_CYCLES_PER_SCC);
        assert!(listed.cycles.iter().all(|cycle| cycle.len() == 2));
        assert!(listed.truncated);
    }

    #[test]
    fn a_group_with_exactly_the_cap_is_not_truncated() {
        // 0 -> 1 -> 2 -> 0 and 1 -> 0 has exactly two cycles.
        let succ = successors(&[(0, 1), (1, 2), (2, 0), (1, 0)], 3);
        let exact = SearchLimits {
            max_cycles: 2,
            max_steps: MAX_SEARCH_STEPS_PER_SCC,
        };
        let listed = enumerate_cycles(&[0, 1, 2], &succ, exact);
        assert_eq!(listed.cycles.len(), 2);
        assert!(!listed.truncated);

        let lower = SearchLimits {
            max_cycles: 1,
            ..exact
        };
        let listed = enumerate_cycles(&[0, 1, 2], &succ, lower);
        assert_eq!(listed.cycles, vec![vec![0, 1]]);
        assert!(listed.truncated);
    }

    #[test]
    fn the_step_limit_marks_the_group_truncated() {
        let (group, succ) = complete_graph(6);
        let tight = SearchLimits {
            max_cycles: MAX_CYCLES_PER_SCC,
            max_steps: 3,
        };
        let listed = enumerate_cycles(&group, &succ, tight);
        assert!(listed.cycles.len() < MAX_CYCLES_PER_SCC);
        assert!(listed.truncated);
    }

    #[test]
    fn test_and_config_files_are_not_build_sources() {
        let root = Path::new("/work/test/packages/a");
        assert!(is_build_source(&root.join("src/index.ts"), root));
        assert!(!is_build_source(&root.join("src/index.test.ts"), root));
        assert!(!is_build_source(&root.join("__tests__/a.ts"), root));
        assert!(!is_build_source(&root.join("vitest.config.ts"), root));
    }

    #[test]
    fn package_of_uses_the_longest_root() {
        let workspaces = vec![
            WorkspaceInfo {
                root: PathBuf::from("/repo/packages/outer"),
                name: "outer".to_owned(),
                is_internal_dependency: false,
            },
            WorkspaceInfo {
                root: PathBuf::from("/repo/packages/outer/inner"),
                name: "inner".to_owned(),
                is_internal_dependency: false,
            },
        ];
        let index = PackageIndex::new(&workspaces, Path::new("/repo"));
        let inner = index.package_of(Path::new("/repo/packages/outer/inner/src/a.ts"));
        let outer = index.package_of(Path::new("/repo/packages/outer/src/a.ts"));
        assert_eq!(inner.map(|i| index.label(i)), Some("inner"));
        assert_eq!(outer.map(|i| index.label(i)), Some("outer"));
        assert_eq!(index.package_of(Path::new("/repo/scripts/a.ts")), None);
    }

    #[test]
    fn a_shared_name_labels_each_package_with_its_root() {
        let workspace = |root: &str, name: &str| WorkspaceInfo {
            root: PathBuf::from(root),
            name: name.to_owned(),
            is_internal_dependency: false,
        };
        let workspaces = vec![
            workspace("/repo/examples/two", "example"),
            workspace("/repo/packages/lib", "lib"),
            workspace("/repo/examples/one", "example"),
            workspace("/repo", "root"),
        ];
        let index = PackageIndex::new(&workspaces, Path::new("/repo"));
        let labels: Vec<&str> = (0..index.len()).map(|i| index.label(i)).collect();
        assert_eq!(
            labels,
            [
                "example (examples/one)",
                "example (examples/two)",
                "lib",
                "root"
            ]
        );
        assert_eq!(relative_root(Path::new("/repo"), Path::new("/repo")), ".");
    }

    /// Node order is the string order of the label, the order the output
    /// contract documents. For a shared name, `Path` order puts `a/b` before
    /// `a-c`, because it compares whole components, while string order puts
    /// `a-c` first, because `-` sorts before `/`.
    #[test]
    fn nodes_follow_the_string_order_of_their_labels() {
        let workspace = |root: &str| WorkspaceInfo {
            root: PathBuf::from(root),
            name: "shared".to_owned(),
            is_internal_dependency: false,
        };
        let workspaces = vec![workspace("/repo/a/b"), workspace("/repo/a-c")];
        let index = PackageIndex::new(&workspaces, Path::new("/repo"));
        let labels: Vec<&str> = (0..index.len()).map(|i| index.label(i)).collect();
        assert_eq!(labels, ["shared (a-c)", "shared (a/b)"]);
        assert_eq!(index.root(0), Path::new("/repo/a-c"));
    }
}
