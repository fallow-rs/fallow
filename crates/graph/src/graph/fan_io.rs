//! Fan-in / fan-out + focus graph facts: from a changed-file set, compute
//! the per-file graph blast-radius signals (fan-IN = importers, fan-OUT = forward
//! deps) and the two confidence-flag signals (dynamic dispatch, re-export
//! indirection) that the weighted focus map (`audit_focus.rs`) consumes.
//!
//! This is the graph-crate half of the focus map: all `ModuleGraph` access
//! lives here (mirroring `impact_closure` / `partition_order`), so the CLI focus
//! extractor stays a pure function of these resolved facts. The fan-in/out is the
//! roadmap stage-4 "fan-in / fan-out (graph): reverse-deps + forward-deps; high
//! fan-in = high blast radius" signal; the confidence flags are the
//! "dynamically-wired / re-export-heavy code is not silently de-prioritized"
//! guard.
//!
//! Determinism (matching the partition + order engine): the engine is a pure function of
//! `(graph, changed_file_ids)`. No timestamps, no randomness, no float scoring.
//! No `FxHashMap` iteration order reaches output: every collection is sorted
//! before serialization in the path-resolved view.

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use rustc_hash::FxHashSet;

use super::{ModuleGraph, ReferenceKind};

/// Per-file graph facts for one changed file, used by the focus map.
///
/// `fan_in` / `fan_out` are the blast-radius signals; `dynamic_dispatch` and
/// `re_export_indirection` are the confidence-flag signals (a file that MAY be
/// reached through dynamic dispatch or re-export indirection carries the flag so
/// its static-reachability signal is not trusted as complete). `FileId`-keyed;
/// the caller path-resolves via [`ModuleGraph::focus_facts_with_paths`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusFileFacts {
    /// The changed file these facts describe.
    pub file: FileId,
    /// Count of DISTINCT files importing this file (fan-in / blast radius).
    /// Excludes the changed file itself.
    pub fan_in: u32,
    /// Count of DISTINCT forward-dependency files this file imports (fan-out).
    /// Excludes the changed file itself.
    pub fan_out: u32,
    /// Whether this file is wired through dynamic dispatch: it has any outgoing
    /// dynamic-import edge OR is referenced by another file via a `DynamicImport`
    /// reference (DI / decorators / plugin-loader / `React.lazy` patterns the
    /// static graph cannot fully resolve). Drives the `low: dynamic dispatch
    /// detected` confidence flag. Conservative (over-flags): a file that MAY be
    /// dynamically wired carries the flag.
    pub dynamic_dispatch: bool,
    /// Whether this file's reachability runs through re-export indirection: it is
    /// a re-export barrel (has its own `re_exports`), is a re-export SOURCE of a
    /// barrel, or is referenced via a `ReExport` reference. Drives the `low:
    /// re-export indirection` confidence flag.
    pub re_export_indirection: bool,
}

/// The same per-file facts with the `FileId` resolved to a root-relative,
/// forward-slashed path string, sorted for deterministic output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusFileFactsPaths {
    /// Root-relative, forward-slashed path of the changed file.
    pub file: String,
    /// Fan-in count (importers).
    pub fan_in: u32,
    /// Fan-out count (forward deps).
    pub fan_out: u32,
    /// Dynamic-dispatch confidence signal.
    pub dynamic_dispatch: bool,
    /// Re-export-indirection confidence signal.
    pub re_export_indirection: bool,
}

/// Signal sets for `focus_file_facts`, built in one pass over every module.
struct ReferenceSignalSets {
    /// Files referenced via a `DynamicImport` reference (target direction).
    dynamic_targets: FxHashSet<FileId>,
    /// Files referenced via a `ReExport` reference (target direction).
    re_export_ref_targets: FxHashSet<FileId>,
    /// Files that originate a `DynamicImport` reference (source direction).
    dynamic_sources: FxHashSet<FileId>,
    /// Files some barrel re-exports from (re-export source direction).
    re_export_sources: FxHashSet<FileId>,
}

impl ModuleGraph {
    /// Compute the per-file focus graph facts (fan-in/out + the two
    /// confidence-flag signals) for a changed-file seed set.
    ///
    /// Out-of-range or duplicate ids in `changed` are tolerated (dropped /
    /// deduped). Each fact is keyed by the changed file's `FileId`; the caller
    /// relativizes via [`ModuleGraph::focus_facts_with_paths`] for serialization.
    #[must_use]
    pub fn focus_file_facts(&self, changed: &[FileId]) -> Vec<FocusFileFacts> {
        // Dedup + drop out-of-range ids into a stable working set.
        let mut seen = FxHashSet::default();
        let mut changed_ids: Vec<FileId> = Vec::with_capacity(changed.len());
        for &id in changed {
            if (id.0 as usize) < self.modules.len() && seen.insert(id) {
                changed_ids.push(id);
            }
        }
        changed_ids.sort_unstable_by_key(|f| f.0);

        // A file participates in DynamicImport / ReExport when ANY export on
        // ANY module carries such a reference TO or FROM it. Build the signal
        // sets once, so the per-changed-file lookups are O(1).
        let reference_signals = self.collect_reference_signal_sets();

        changed_ids
            .iter()
            .map(|&id| {
                let fan_in = self.fan_in_count(id);
                let fan_out = self.fan_out_count(id);
                let dynamic_dispatch = reference_signals.dynamic_targets.contains(&id)
                    || reference_signals.dynamic_sources.contains(&id);
                let re_export_indirection = self.is_re_export_participant(id, &reference_signals);
                FocusFileFacts {
                    file: id,
                    fan_in,
                    fan_out,
                    dynamic_dispatch,
                    re_export_indirection,
                }
            })
            .collect()
    }

    /// Distinct count of files importing `file` (fan-in), excluding `file`.
    fn fan_in_count(&self, file: FileId) -> u32 {
        let Some(importers) = self.reverse_deps.get(file.0 as usize) else {
            return 0;
        };
        let mut distinct: FxHashSet<FileId> = FxHashSet::default();
        for &importer in importers {
            if importer != file {
                distinct.insert(importer);
            }
        }
        u32::try_from(distinct.len()).unwrap_or(u32::MAX)
    }

    /// Distinct count of forward-dependency files `file` imports (fan-out),
    /// excluding self-edges.
    fn fan_out_count(&self, file: FileId) -> u32 {
        let mut distinct: FxHashSet<FileId> = FxHashSet::default();
        for target in self.edges_for(file) {
            if target != file {
                distinct.insert(target);
            }
        }
        u32::try_from(distinct.len()).unwrap_or(u32::MAX)
    }

    /// Build reference signal sets in one pass over every module.
    fn collect_reference_signal_sets(&self) -> ReferenceSignalSets {
        let mut dynamic_targets: FxHashSet<FileId> = FxHashSet::default();
        let mut re_export_ref_targets: FxHashSet<FileId> = FxHashSet::default();
        let mut dynamic_sources: FxHashSet<FileId> = FxHashSet::default();
        let mut re_export_sources: FxHashSet<FileId> = FxHashSet::default();
        for node in &self.modules {
            for edge in &node.re_exports {
                re_export_sources.insert(edge.source_file);
            }
            for export in &node.exports {
                for reference in &export.references {
                    match reference.kind {
                        ReferenceKind::DynamicImport => {
                            dynamic_targets.insert(node.file_id);
                            dynamic_sources.insert(reference.from_file);
                        }
                        ReferenceKind::ReExport => {
                            re_export_ref_targets.insert(node.file_id);
                        }
                        _ => {}
                    }
                }
            }
        }
        ReferenceSignalSets {
            dynamic_targets,
            re_export_ref_targets,
            dynamic_sources,
            re_export_sources,
        }
    }

    /// Whether `file` participates in re-export indirection: it is a re-export
    /// barrel (declares its own `re_exports`), it is a re-export SOURCE of some
    /// barrel, or it is referenced via a `ReExport` reference (the
    /// `re_export_ref_targets` membership).
    fn is_re_export_participant(&self, file: FileId, sets: &ReferenceSignalSets) -> bool {
        if sets.re_export_ref_targets.contains(&file) {
            return true;
        }
        // Barrel: declares its own re-exports.
        if let Some(node) = self.modules.get(file.0 as usize)
            && !node.re_exports.is_empty()
        {
            return true;
        }
        // Re-export SOURCE: some barrel re-exports FROM this file.
        sets.re_export_sources.contains(&file)
    }

    /// Resolve a `FocusFileFacts` set's `FileId`s to root-relative, forward-
    /// slashed paths, sorted for deterministic output. Files whose module is
    /// missing are dropped.
    #[must_use]
    pub fn focus_facts_with_paths(
        &self,
        facts: &[FocusFileFacts],
        root: &Path,
    ) -> Vec<FocusFileFactsPaths> {
        let mut resolved: Vec<FocusFileFactsPaths> = facts
            .iter()
            .filter_map(|f| {
                let path = self.modules.get(f.file.0 as usize)?;
                Some(FocusFileFactsPaths {
                    file: relativize(&path.path, root),
                    fan_in: f.fan_in,
                    fan_out: f.fan_out,
                    dynamic_dispatch: f.dynamic_dispatch,
                    re_export_indirection: f.re_export_indirection,
                })
            })
            .collect();
        resolved.sort_by(|a, b| a.file.cmp(&b.file));
        resolved
    }
}

/// Strip `root` and forward-slash-normalize a module path (mirrors
/// `impact_closure::relativize` / `partition_order::relativize`).
fn relativize(path: &Path, root: &Path) -> String {
    let rel: PathBuf = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    rel.to_string_lossy().replace('\\', "/")
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

    /// core (0) <- mid (1) <- app (2). app imports mid imports core.
    fn build_chain_graph() -> ModuleGraph {
        let files = vec![
            file(0, "/p/src/core.ts"),
            file(1, "/p/src/mid.ts"),
            file(2, "/p/src/app.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/app.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/core.ts"),
                exports: vec![named_export("compute")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/mid.ts"),
                resolved_imports: vec![named_import("./core", "compute", FileId(0))],
                exports: vec![named_export("midFn")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/app.ts"),
                resolved_imports: vec![named_import("./mid", "midFn", FileId(1))],
                ..Default::default()
            },
        ];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn fan_in_counts_importers() {
        let graph = build_chain_graph();
        // core is imported by mid: fan_in = 1, fan_out = 0.
        let facts = graph.focus_file_facts(&[FileId(0)]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].fan_in, 1);
        assert_eq!(facts[0].fan_out, 0);
    }

    #[test]
    fn fan_out_counts_forward_deps() {
        let graph = build_chain_graph();
        // app imports mid: fan_out = 1, fan_in = 0 (nothing imports app).
        let facts = graph.focus_file_facts(&[FileId(2)]);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].fan_out, 1);
        assert_eq!(facts[0].fan_in, 0);
    }

    #[test]
    fn focus_facts_are_byte_identical_across_runs() {
        let graph = build_chain_graph();
        let changed = [FileId(0), FileId(1), FileId(2)];
        let first = graph.focus_file_facts(&changed);
        let second = graph.focus_file_facts(&changed);
        assert_eq!(first, second);
        let p1 = graph.focus_facts_with_paths(&first, Path::new("/p"));
        let p2 = graph.focus_facts_with_paths(&second, Path::new("/p"));
        assert_eq!(format!("{p1:?}"), format!("{p2:?}"));
    }

    #[test]
    fn re_export_barrel_flags_indirection() {
        use crate::resolve::ResolvedReExport;
        use fallow_types::extract::ReExportInfo;

        let files = vec![
            file(0, "/p/src/impl.ts"),
            file(1, "/p/src/barrel.ts"),
            file(2, "/p/src/consumer.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/consumer.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/impl.ts"),
                exports: vec![named_export("widget")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/barrel.ts"),
                re_exports: vec![ResolvedReExport {
                    info: ReExportInfo {
                        source: "./impl".to_string(),
                        imported_name: "widget".to_string(),
                        exported_name: "widget".to_string(),
                        is_type_only: false,
                        span: oxc_span::Span::new(0, 10),
                    },
                    target: ResolveResult::InternalModule(FileId(0)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/consumer.ts"),
                resolved_imports: vec![named_import("./barrel", "widget", FileId(1))],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        // barrel.ts declares re-exports -> flagged. impl.ts is a re-export source
        // -> flagged.
        let barrel = graph.focus_file_facts(&[FileId(1)]);
        assert!(barrel[0].re_export_indirection, "barrel flags indirection");
        let impl_facts = graph.focus_file_facts(&[FileId(0)]);
        assert!(
            impl_facts[0].re_export_indirection,
            "re-export source flags indirection"
        );
    }

    #[test]
    fn empty_changed_set_yields_no_facts() {
        let graph = build_chain_graph();
        assert!(graph.focus_file_facts(&[]).is_empty());
    }

    #[test]
    fn out_of_range_ids_are_dropped() {
        let graph = build_chain_graph();
        let facts = graph.focus_file_facts(&[FileId(999)]);
        assert!(facts.is_empty());
    }
}
