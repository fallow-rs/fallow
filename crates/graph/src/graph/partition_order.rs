//! Partition + order engine: from a changed-file set, split the change into
//! coherent, independently-reviewable UNITS and suggest a dependency-sensible
//! review ORDER.
//!
//! v1 partitioning is BY-MODULE only (the load-bearing panel decision): a "unit"
//! is the parent directory of a changed file, root-relative. This is the only
//! clustering definition that is byte-identical-deterministic straight from the
//! graph with zero heuristics; feature-cluster and concern partitioning are
//! explicitly DEFERRED (they need scoring heuristics whose tie-breaks are a fresh
//! nondeterminism + false-positive surface).
//!
//! The ORDER is a dependency-sensible topological sequence over the unit DAG: a
//! unit that DEFINES what another CONSUMES comes first (review the load-bearing
//! definition before its consumers), mechanical/leaf units last, ties broken by
//! the path sort. Inter-unit edges come from the graph's forward edges
//! ([`super::ModuleGraph::edges_for`], `mod.rs` L255; the inverse is
//! `reverse_deps`, L75).
//!
//! Determinism (the roadmap done-condition "Same PR run twice -> byte-identical
//! unit assignment and order"): the engine is a pure function of
//! `(graph, changed_file_ids)`. No timestamps, no randomness. No `FxHashMap`
//! iteration order ever reaches output; every collection is materialized into a
//! `Vec` and explicitly sorted before use. FileIds are path-sorted and stable
//! cross-run (ADR-004), so sorting by FileId == sorting by path. The only choice
//! point in the topological sort is a min-pick over sorted `module_dir` strings.

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use super::ModuleGraph;

/// A single review unit: a coherent by-module cluster of the changed set. The
/// `module_dir` is the root-relative parent directory shared by `files`; the
/// changed root file (one with no parent directory) clusters under the
/// repository-root key (the empty string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUnit {
    /// The module directory the unit covers (root-relative, forward-slashed).
    /// The empty string is the repository-root group for changed files with no
    /// parent directory.
    module_dir: String,
    /// The changed files in this unit, `FileId`-sorted (== path-sorted, ADR-004).
    files: Vec<FileId>,
}

/// Result of a partition + order computation, keyed by `FileId` / `module_dir`.
/// The caller relativizes via [`ModuleGraph::partition_order_with_paths`] for
/// serialization, mirroring the `impact_closure` / `closure_with_paths` pair.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionOrder {
    /// The by-module units, sorted by `module_dir` string.
    units: Vec<ReviewUnit>,
    /// The dependency-sensible review order: `module_dir` strings, definitions
    /// before consumers, mechanical/leaf units last, ties broken by the path
    /// sort. One entry per unit; a permutation of the `units` `module_dir` set.
    order: Vec<String>,
}

/// The same partition + order with each unit's `FileId`s resolved to
/// root-relative, forward-slashed path strings, sorted for deterministic output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionOrderPaths {
    /// The by-module units with file paths resolved, sorted by `module_dir`.
    pub units: Vec<ReviewUnitPaths>,
    /// The dependency-sensible review order of `module_dir` strings.
    pub order: Vec<String>,
}

/// A [`ReviewUnit`] with `FileId`s resolved to root-relative paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUnitPaths {
    /// The module directory the unit covers (root-relative, forward-slashed).
    pub module_dir: String,
    /// The changed files in this unit, path-sorted.
    pub files: Vec<String>,
}

impl ModuleGraph {
    /// Compute the by-module partition and dependency-sensible order for a
    /// changed-file seed set.
    ///
    /// Out-of-range or duplicate ids in `changed` are tolerated (dropped /
    /// deduped). The partition groups each changed file by its parent directory;
    /// the order is a deterministic topological sort over the inter-unit DAG
    /// (definitions before consumers, ties broken by the `module_dir` sort).
    #[must_use]
    pub fn partition_order(&self, changed: &[FileId]) -> PartitionOrder {
        // Dedup + drop out-of-range ids, keeping a path-stable working set.
        let mut seen = FxHashSet::default();
        let mut changed_ids: Vec<FileId> = Vec::with_capacity(changed.len());
        for &id in changed {
            if (id.0 as usize) < self.modules.len() && seen.insert(id) {
                changed_ids.push(id);
            }
        }
        changed_ids.sort_unstable_by_key(|f| f.0);

        let units = self.build_units(&changed_ids);
        let order = self.order_units(&units, &changed_ids);
        PartitionOrder { units, order }
    }

    /// Group changed files by their parent directory (the module). Returns the
    /// units sorted by `module_dir`, each unit's files `FileId`-sorted.
    fn build_units(&self, changed_ids: &[FileId]) -> Vec<ReviewUnit> {
        // module_dir -> files. FxHashMap iteration order never reaches output:
        // the keys are pulled into a Vec and sorted below.
        let mut by_dir: FxHashMap<String, Vec<FileId>> = FxHashMap::default();
        for &id in changed_ids {
            let Some(module) = self.modules.get(id.0 as usize) else {
                continue;
            };
            let dir = module_dir_key(&module.path);
            by_dir.entry(dir).or_default().push(id);
        }

        let mut units: Vec<ReviewUnit> = by_dir
            .into_iter()
            .map(|(module_dir, mut files)| {
                files.sort_unstable_by_key(|f| f.0);
                ReviewUnit { module_dir, files }
            })
            .collect();
        units.sort_by(|a, b| a.module_dir.cmp(&b.module_dir));
        units
    }

    /// Order the units so a unit defining what another consumes comes first, ties
    /// broken by the `module_dir` sort. Deterministic Kahn topological sort with
    /// a min-pick ready set.
    fn order_units(&self, units: &[ReviewUnit], changed_ids: &[FileId]) -> Vec<String> {
        if units.is_empty() {
            return Vec::new();
        }

        // FileId -> owning unit index, for resolving inter-unit edges.
        let unit_of: FxHashMap<FileId, usize> = units
            .iter()
            .enumerate()
            .flat_map(|(i, unit)| unit.files.iter().map(move |&f| (f, i)))
            .collect();

        let unit_count = units.len();
        // `dep_count[c]` = number of distinct units `c` depends on (consumes from)
        // that are still unemitted. A unit emerges ready once all its deps are
        // emitted, so a pure definition (depends on nothing in the changed set)
        // is ready first.
        let mut deps: Vec<FxHashSet<usize>> = vec![FxHashSet::default(); unit_count];
        for &id in changed_ids {
            let Some(&consumer_unit) = unit_of.get(&id) else {
                continue;
            };
            for dep_target in self.edges_for(id) {
                let Some(&dep_unit) = unit_of.get(&dep_target) else {
                    continue;
                };
                if dep_unit != consumer_unit {
                    deps[consumer_unit].insert(dep_unit);
                }
            }
        }

        kahn_min_pick(units, &deps)
    }

    /// Resolve a partition + order's `FileId`s to root-relative, forward-slashed
    /// paths, sorted for deterministic output. Files whose module is missing are
    /// dropped; a unit left empty after that drop is omitted. The `module_dir`
    /// keys (and the `order` entries, which are `module_dir` strings) are
    /// root-relativized too so the whole shape is root-relative.
    #[must_use]
    pub fn partition_order_with_paths(
        &self,
        partition: &PartitionOrder,
        root: &Path,
    ) -> PartitionOrderPaths {
        let resolve = |id: FileId| -> Option<String> {
            self.modules
                .get(id.0 as usize)
                .map(|m| relativize(&m.path, root))
        };

        let units: Vec<ReviewUnitPaths> = partition
            .units
            .iter()
            .filter_map(|unit| {
                let mut files: Vec<String> =
                    unit.files.iter().filter_map(|&id| resolve(id)).collect();
                if files.is_empty() {
                    return None;
                }
                files.sort();
                Some(ReviewUnitPaths {
                    module_dir: relativize_dir(&unit.module_dir, root),
                    files,
                })
            })
            .collect();

        let order: Vec<String> = partition
            .order
            .iter()
            .map(|dir| relativize_dir(dir, root))
            .collect();

        PartitionOrderPaths { units, order }
    }
}

/// Deterministic Kahn topological sort: emit a unit only once every unit it
/// depends on (consumes from) has been emitted, so definitions precede
/// consumers. The ready set is resolved by a min-pick over the `module_dir`
/// strings, so ties (independent units) and any cycle break resolve by the path
/// sort. A residual cycle's units are appended in `module_dir`-sorted order.
fn kahn_min_pick(units: &[ReviewUnit], deps: &[FxHashSet<usize>]) -> Vec<String> {
    let unit_count = units.len();
    let mut remaining: FxHashSet<usize> = (0..unit_count).collect();
    let mut emitted: FxHashSet<usize> = FxHashSet::default();
    let mut order: Vec<String> = Vec::with_capacity(unit_count);

    while !remaining.is_empty() {
        // Find the lexicographically smallest module_dir among ready units (all
        // deps emitted). Iterating `remaining` (an FxHashSet) is fine: we pick
        // the min by module_dir, not by iteration order.
        let mut ready: Option<usize> = None;
        for &idx in &remaining {
            let all_deps_emitted = deps[idx].iter().all(|d| emitted.contains(d));
            if !all_deps_emitted {
                continue;
            }
            ready = Some(match ready {
                Some(cur) if units[cur].module_dir <= units[idx].module_dir => cur,
                _ => idx,
            });
        }

        match ready {
            Some(idx) => {
                order.push(units[idx].module_dir.clone());
                emitted.insert(idx);
                remaining.remove(&idx);
            }
            None => {
                // Cycle: no ready unit but units remain. Append the rest in
                // module_dir-sorted order (deterministic fallback).
                let mut rest: Vec<usize> = remaining.iter().copied().collect();
                rest.sort_by(|&a, &b| units[a].module_dir.cmp(&units[b].module_dir));
                for idx in rest {
                    order.push(units[idx].module_dir.clone());
                }
                break;
            }
        }
    }

    order
}

/// The root-relative parent-directory key for a module path. The repository-root
/// file (no parent component) maps to the empty string (the root group).
fn module_dir_key(path: &Path) -> String {
    path.parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// Strip `root` and forward-slash-normalize a module path for cross-platform
/// JSON parity (mirrors `impact_closure::relativize`).
fn relativize(path: &Path, root: &Path) -> String {
    let rel: PathBuf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    rel.to_string_lossy().replace('\\', "/")
}

/// Root-relativize a `module_dir` key (a forward-slashed directory string).
/// The empty root-group key stays empty; otherwise the `root` prefix is stripped
/// via the same `Path`-based logic so the output matches the file path-space.
fn relativize_dir(dir: &str, root: &Path) -> String {
    if dir.is_empty() {
        return String::new();
    }
    relativize(Path::new(dir), root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::extract::{ExportInfo, ExportName, ImportInfo, ImportedName, VisibilityTag};
    use std::path::PathBuf;

    fn file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 10,
        }
    }

    fn named_import(source: &str, name: &str, target: FileId) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: source.to_string(),
                imported_name: ImportedName::Named(name.to_string()),
                local_name: name.to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 10),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn named_export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::new(0, 20),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        }
    }

    /// Three directories: `core/` defines, `mid/` consumes core, `app/` consumes
    /// mid. Files: core/a.ts, core/b.ts, mid/m.ts, app/x.ts. entry is app/x.ts.
    fn build_three_dir_graph() -> ModuleGraph {
        let files = vec![
            file(0, "/p/src/app/x.ts"),
            file(1, "/p/src/core/a.ts"),
            file(2, "/p/src/core/b.ts"),
            file(3, "/p/src/mid/m.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/app/x.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/app/x.ts"),
                resolved_imports: vec![named_import("../mid/m", "midFn", FileId(3))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/core/a.ts"),
                exports: vec![named_export("alpha")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/core/b.ts"),
                exports: vec![named_export("beta")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/p/src/mid/m.ts"),
                resolved_imports: vec![named_import("../core/a", "alpha", FileId(1))],
                exports: vec![named_export("midFn")],
                ..Default::default()
            },
        ];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn partition_groups_changed_files_by_module_directory() {
        let graph = build_three_dir_graph();
        // Change all four files.
        let partition = graph.partition_order(&[FileId(0), FileId(1), FileId(2), FileId(3)]);
        let paths = graph.partition_order_with_paths(&partition, Path::new("/p"));
        // Three units, sorted by module_dir.
        let dirs: Vec<&str> = paths.units.iter().map(|u| u.module_dir.as_str()).collect();
        assert_eq!(dirs, vec!["src/app", "src/core", "src/mid"]);
        // core/ groups its two files, path-sorted.
        let core = paths
            .units
            .iter()
            .find(|u| u.module_dir == "src/core")
            .expect("core unit");
        assert_eq!(core.files, vec!["src/core/a.ts", "src/core/b.ts"]);
    }

    #[test]
    fn order_places_definitions_before_consumers() {
        let graph = build_three_dir_graph();
        // app consumes mid consumes core, so order = core, mid, app.
        let partition = graph.partition_order(&[FileId(0), FileId(1), FileId(2), FileId(3)]);
        assert_eq!(
            partition.order,
            vec![
                "/p/src/core".to_string(),
                "/p/src/mid".to_string(),
                "/p/src/app".to_string(),
            ]
        );
    }

    #[test]
    fn independent_units_order_by_path_sort() {
        // Two unrelated directories (no inter-unit edge): order is the path sort.
        let files = vec![file(0, "/p/src/billing/b.ts"), file(1, "/p/src/auth/a.ts")];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/auth/a.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/billing/b.ts"),
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/auth/a.ts"),
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let partition = graph.partition_order(&[FileId(0), FileId(1)]);
        assert_eq!(
            partition.order,
            vec!["/p/src/auth".to_string(), "/p/src/billing".to_string()]
        );
    }

    #[test]
    fn partition_order_is_byte_identical_across_runs() {
        let graph = build_three_dir_graph();
        let changed = [FileId(0), FileId(1), FileId(2), FileId(3)];
        let first = graph.partition_order(&changed);
        let second = graph.partition_order(&changed);
        // FileId-keyed shape is structurally identical.
        assert_eq!(first, second);
        // Path-resolved shape is byte-identical when debug-rendered (a proxy for
        // serialization; the audit_brief layer serializes the same data).
        let p1 = graph.partition_order_with_paths(&first, Path::new("/p"));
        let p2 = graph.partition_order_with_paths(&second, Path::new("/p"));
        assert_eq!(format!("{p1:?}"), format!("{p2:?}"));
    }

    #[test]
    fn changed_set_order_does_not_affect_result() {
        // Feeding the changed ids in a different input order yields the same
        // partition + order (the engine sorts internally).
        let graph = build_three_dir_graph();
        let a = graph.partition_order(&[FileId(3), FileId(0), FileId(2), FileId(1)]);
        let b = graph.partition_order(&[FileId(0), FileId(1), FileId(2), FileId(3)]);
        assert_eq!(a, b);
    }

    #[test]
    fn root_file_clusters_under_root_group() {
        let files = vec![file(0, "index.ts")];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("index.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("index.ts"),
            ..Default::default()
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let partition = graph.partition_order(&[FileId(0)]);
        assert_eq!(partition.units.len(), 1);
        assert_eq!(partition.units[0].module_dir, "");
    }

    #[test]
    fn empty_changed_set_yields_empty_partition() {
        let graph = build_three_dir_graph();
        let partition = graph.partition_order(&[]);
        assert!(partition.units.is_empty());
        assert!(partition.order.is_empty());
    }

    #[test]
    fn scale_300_file_multi_module_graph_is_stable() {
        // 30 directories x 10 files = 300 files. Each dir's first file imports the
        // previous dir's first file (a chain across modules), so the order is a
        // real topological sequence, not just the path sort.
        const DIRS: u32 = 30;
        const PER_DIR: u32 = 10;
        let mut files = Vec::new();
        let mut resolved = Vec::new();
        for d in 0..DIRS {
            for f in 0..PER_DIR {
                let id = d * PER_DIR + f;
                let path = format!("/p/src/mod{d:02}/file{f:02}.ts");
                files.push(file(id, &path));
            }
        }
        for d in 0..DIRS {
            for f in 0..PER_DIR {
                let id = d * PER_DIR + f;
                let mut module = ResolvedModule {
                    file_id: FileId(id),
                    path: PathBuf::from(format!("/p/src/mod{d:02}/file{f:02}.ts")),
                    exports: vec![named_export(&format!("e{id}"))],
                    ..Default::default()
                };
                // The first file of each dir (except dir 0) imports the first file
                // of the previous dir: mod01 consumes mod00, mod02 consumes mod01.
                if f == 0 && d > 0 {
                    let dep = (d - 1) * PER_DIR;
                    module.resolved_imports =
                        vec![named_import("../prev", &format!("e{dep}"), FileId(dep))];
                }
                resolved.push(module);
            }
        }
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/mod00/file00.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);

        let changed: Vec<FileId> = (0..DIRS * PER_DIR).map(FileId).collect();
        let first = graph.partition_order(&changed);
        let second = graph.partition_order(&changed);
        assert_eq!(first, second, "300-file partition must be stable");
        assert_eq!(first.units.len(), DIRS as usize, "one unit per directory");
        // The dependency chain forces mod00 before mod01 before ... before mod29.
        let expected: Vec<String> = (0..DIRS).map(|d| format!("/p/src/mod{d:02}")).collect();
        assert_eq!(
            first.order, expected,
            "definitions precede consumers at scale"
        );
        // Path-resolved serialization proxy is byte-identical across runs.
        let p1 = graph.partition_order_with_paths(&first, Path::new("/p"));
        let p2 = graph.partition_order_with_paths(&second, Path::new("/p"));
        assert_eq!(format!("{p1:?}"), format!("{p2:?}"));
    }
}
