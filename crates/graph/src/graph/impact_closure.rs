//! Impact-closure engine: from a changed-file set, compute the transitive
//! affected-but-NOT-in-diff set plus a coordination-gap detector.
//!
//! The differentiator a diff tool cannot do: a diff is changed lines, but the
//! real risk is the transitive set of code those lines affect, most of which is
//! NOT in the diff. This walks [`ModuleGraph::reverse_deps`] (which already folds
//! re-export chains in, because a `export {x} from './changed'` is a real graph
//! edge barrel->changed) and partitions the reached files into
//! `{ in_diff, affected_not_shown }`, then reports the coordination gap: a changed
//! EXPORTED symbol whose consumer modules are absent from the diff.
//!
//! Honest scope (ADR-001, syntactic): the coordination gap is an attention
//! pointer at the exact inter-module failure mode, NOT a correctness proof.

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use fixedbitset::FixedBitSet;
use rustc_hash::FxHashMap;

use super::ModuleGraph;

/// A single coordination-gap entry: a changed file exports symbols consumed by a
/// `consumer` module that is NOT in the diff. Deduped per (changed, consumer)
/// PAIR (firing-precision rule R2): one entry per distinct consumer module, the
/// consumed-symbol names folded in, never one entry per import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationGap {
    /// The changed file whose exported contract a non-diff module consumes.
    changed_file: FileId,
    /// The consumer module that imports the changed contract and is NOT in the diff.
    consumer_file: FileId,
    /// The exported symbol names the consumer references, sorted and deduped.
    consumed_symbols: Vec<String>,
}

/// Result of an impact-closure computation. File partitions are `FileId` sets so
/// the caller relativizes paths in its own path-space; [`ModuleGraph::closure_with_paths`]
/// produces the root-relative path view for serialization.
#[derive(Debug, Clone, Default)]
pub struct ImpactClosure {
    /// The seed (changed) files, the diff itself.
    in_diff: Vec<FileId>,
    /// Files transitively affected through `reverse_deps` (importers + re-export
    /// chains) that do NOT appear in the diff. The differentiator set.
    affected_not_shown: Vec<FileId>,
    /// Coordination gaps: changed contracts consumed by non-diff modules.
    coordination_gap: Vec<CoordinationGap>,
}

/// The same closure with `FileId`s resolved to root-relative, forward-slashed
/// path strings, sorted for deterministic output.
#[derive(Debug, Clone, Default)]
pub struct ImpactClosurePaths {
    /// Root-relative changed-file paths, sorted.
    pub in_diff: Vec<String>,
    /// Root-relative affected-but-not-shown paths, sorted.
    pub affected_not_shown: Vec<String>,
    /// Coordination gaps with paths resolved, sorted by (changed, consumer).
    pub coordination_gap: Vec<CoordinationGapPaths>,
}

/// A [`CoordinationGap`] with `FileId`s resolved to root-relative paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationGapPaths {
    /// Root-relative path of the changed file.
    pub changed_file: String,
    /// Root-relative path of the non-diff consumer.
    pub consumer_file: String,
    /// Consumed symbol names, sorted.
    pub consumed_symbols: Vec<String>,
}

impl ModuleGraph {
    /// Compute the impact closure for a changed-file seed set.
    ///
    /// BFS over `reverse_deps` from every changed file yields the transitive
    /// affected set; the seed partitions into `in_diff`, the rest into
    /// `affected_not_shown`. The coordination gap walks each changed file's
    /// exported-symbol references and reports those whose consumer is outside the
    /// diff (rule R2: one entry per distinct consumer module).
    ///
    /// `changed` is a slice of `FileId`s; out-of-range or duplicate ids are
    /// tolerated. Type-only re-export edges are skipped for the gap evidence so a
    /// `import type`-only consumer (erased at build, no runtime contract) does not
    /// fire.
    #[must_use]
    pub fn impact_closure(&self, changed: &[FileId]) -> ImpactClosure {
        let capacity = self.modules.len();
        let mut in_diff_set = FixedBitSet::with_capacity(capacity);
        for &id in changed {
            let idx = id.0 as usize;
            if idx < capacity {
                in_diff_set.insert(idx);
            }
        }

        let affected = self.collect_reverse_closure(&in_diff_set, capacity);
        let coordination_gap = self.collect_coordination_gaps(&in_diff_set);

        let mut in_diff: Vec<FileId> = in_diff_set.ones().map(|i| FileId(i as u32)).collect();
        in_diff.sort_unstable_by_key(|f| f.0);
        let mut affected_not_shown: Vec<FileId> =
            affected.ones().map(|i| FileId(i as u32)).collect();
        affected_not_shown.sort_unstable_by_key(|f| f.0);

        ImpactClosure {
            in_diff,
            affected_not_shown,
            coordination_gap,
        }
    }

    /// BFS over `reverse_deps` from the seed set, returning the bitset of files
    /// reached but NOT in the seed (the affected-not-shown partition).
    fn collect_reverse_closure(&self, seed: &FixedBitSet, capacity: usize) -> FixedBitSet {
        let mut visited = seed.clone();
        let mut affected = FixedBitSet::with_capacity(capacity);
        let mut queue: Vec<FileId> = seed.ones().map(|i| FileId(i as u32)).collect();

        while let Some(current) = queue.pop() {
            let Some(importers) = self.reverse_deps.get(current.0 as usize) else {
                continue;
            };
            for &importer in importers {
                let idx = importer.0 as usize;
                if idx >= capacity || visited.contains(idx) {
                    continue;
                }
                visited.insert(idx);
                if !seed.contains(idx) {
                    affected.insert(idx);
                }
                queue.push(importer);
            }
        }
        affected
    }

    /// For each changed file, collect the consumers (via exported-symbol
    /// references) that are OUTSIDE the diff, one [`CoordinationGap`] per distinct
    /// (changed, consumer) pair with the consumed symbol names folded in.
    fn collect_coordination_gaps(&self, in_diff_set: &FixedBitSet) -> Vec<CoordinationGap> {
        let mut gaps: Vec<CoordinationGap> = Vec::new();
        for changed_idx in in_diff_set.ones() {
            let Some(module) = self.modules.get(changed_idx) else {
                continue;
            };
            // (changed, consumer) -> consumed symbol name set. R2: one entry per
            // distinct consumer module, never per import statement.
            let mut by_consumer: FxHashMap<FileId, Vec<String>> = FxHashMap::default();
            for export in &module.exports {
                if export.is_type_only {
                    continue;
                }
                let symbol_name = export.name.to_string();
                for reference in &export.references {
                    let consumer_idx = reference.from_file.0 as usize;
                    if in_diff_set.contains(consumer_idx) {
                        // Consumer is inside the diff: updated alongside, no gap.
                        continue;
                    }
                    // Dev-only glue (stories / specs / tests) co-located with the
                    // changed module is not a cross-module coordination contract: if
                    // the symbol's shape changes, the story/spec fails loudly in its
                    // own dev/CI run rather than hiding a production coordination
                    // risk. Skip it here; it still appears in `affected_not_shown`.
                    if self
                        .modules
                        .get(consumer_idx)
                        .is_some_and(|m| is_dev_glue_path(&m.path))
                    {
                        continue;
                    }
                    by_consumer
                        .entry(reference.from_file)
                        .or_default()
                        .push(symbol_name.clone());
                }
            }
            for (consumer_file, mut symbols) in by_consumer {
                symbols.sort_unstable();
                symbols.dedup();
                gaps.push(CoordinationGap {
                    changed_file: FileId(changed_idx as u32),
                    consumer_file,
                    consumed_symbols: symbols,
                });
            }
        }
        gaps.sort_unstable_by(|a, b| {
            a.changed_file
                .0
                .cmp(&b.changed_file.0)
                .then_with(|| a.consumer_file.0.cmp(&b.consumer_file.0))
        });
        gaps
    }

    /// Resolve a closure's `FileId`s to root-relative, forward-slashed paths,
    /// sorted for deterministic output. Files whose module is missing are dropped.
    #[must_use]
    pub fn closure_with_paths(&self, closure: &ImpactClosure, root: &Path) -> ImpactClosurePaths {
        let resolve = |id: FileId| -> Option<String> {
            self.modules
                .get(id.0 as usize)
                .map(|m| relativize(&m.path, root))
        };

        let mut in_diff: Vec<String> = closure
            .in_diff
            .iter()
            .filter_map(|&id| resolve(id))
            .collect();
        in_diff.sort();
        let mut affected_not_shown: Vec<String> = closure
            .affected_not_shown
            .iter()
            .filter_map(|&id| resolve(id))
            .collect();
        affected_not_shown.sort();

        let mut coordination_gap: Vec<CoordinationGapPaths> = closure
            .coordination_gap
            .iter()
            .filter_map(|gap| {
                Some(CoordinationGapPaths {
                    changed_file: resolve(gap.changed_file)?,
                    consumer_file: resolve(gap.consumer_file)?,
                    consumed_symbols: gap.consumed_symbols.clone(),
                })
            })
            .collect();
        coordination_gap.sort_by(|a, b| {
            a.changed_file
                .cmp(&b.changed_file)
                .then_with(|| a.consumer_file.cmp(&b.consumer_file))
        });

        ImpactClosurePaths {
            in_diff,
            affected_not_shown,
            coordination_gap,
        }
    }
}

/// True when `path` is a dev-only glue file (a Storybook story, a test/spec, a
/// Cypress spec, or a file under a `__tests__` / `__mocks__` / `__stories__`
/// directory). Such a consumer is NOT a cross-module coordination contract: a
/// contract change surfaces in its own dev/CI run, never as a hidden production
/// coordination gap. Co-located stories pairing with their component were the
/// dominant low-value noise in the coordination-gap evidence.
fn is_dev_glue_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if [".stories.", ".story.", ".spec.", ".test.", ".cy."]
        .iter()
        .any(|marker| name.contains(marker))
    {
        return true;
    }
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("__tests__" | "__mocks__" | "__stories__")
        )
    })
}

/// Strip `root` and forward-slash-normalize a module path for cross-platform
/// JSON parity with the trace output path relativization.
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

    /// Plain reverse-dep chain: core (0) <- mid (1) <- app (2).
    /// app imports mid imports core; entry is app.
    fn build_reverse_dep_graph() -> ModuleGraph {
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

    /// Re-export chain: impl (0) -> barrel (1) re-exports -> consumer (2) imports
    /// from the barrel. entry is consumer.
    fn build_re_export_graph() -> ModuleGraph {
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
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn reverse_dep_closure_equals_hand_computed_set() {
        let graph = build_reverse_dep_graph();
        // Change core.ts. Hand-computed reverse-dep closure = {mid, app}.
        let closure = graph.impact_closure(&[FileId(0)]);
        assert_eq!(closure.in_diff, vec![FileId(0)]);
        assert_eq!(closure.affected_not_shown, vec![FileId(1), FileId(2)]);
    }

    #[test]
    fn coordination_gap_fires_when_consumer_outside_diff() {
        let graph = build_reverse_dep_graph();
        // core changed, mid (consumer of core.compute) is NOT in the diff -> fires.
        let closure = graph.impact_closure(&[FileId(0)]);
        assert_eq!(closure.coordination_gap.len(), 1);
        let gap = &closure.coordination_gap[0];
        assert_eq!(gap.changed_file, FileId(0));
        assert_eq!(gap.consumer_file, FileId(1));
        assert_eq!(gap.consumed_symbols, vec!["compute".to_string()]);
    }

    #[test]
    fn coordination_gap_skips_story_and_test_consumers() {
        use fallow_types::discover::{EntryPoint, EntryPointSource};
        // button.component (0) is changed; consumed by a co-located story (1) AND a
        // real panel component (2), both OUTSIDE the diff. Only the real consumer is
        // a coordination gap; the story is dev-only glue that fails in its own run.
        let files = vec![
            file(0, "/p/src/button.component.ts"),
            file(1, "/p/src/button.stories.ts"),
            file(2, "/p/src/panel.component.ts"),
        ];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/panel.component.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/button.component.ts"),
                exports: vec![named_export("BzmButton")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/button.stories.ts"),
                resolved_imports: vec![named_import("./button.component", "BzmButton", FileId(0))],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/p/src/panel.component.ts"),
                resolved_imports: vec![named_import("./button.component", "BzmButton", FileId(0))],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let closure = graph.impact_closure(&[FileId(0)]);
        // Exactly one gap, on the real consumer; the story is NOT a gap.
        assert_eq!(closure.coordination_gap.len(), 1);
        assert_eq!(closure.coordination_gap[0].consumer_file, FileId(2));
        // The story is still surfaced as affected (declassified, never hidden).
        assert!(closure.affected_not_shown.contains(&FileId(1)));
    }

    #[test]
    fn coordination_gap_does_not_fire_when_consumer_inside_diff() {
        let graph = build_reverse_dep_graph();
        // core AND mid both changed. mid is the only consumer of core.compute and
        // it IS in the diff -> no gap for the core->mid pair. (mid->app may still
        // fire, app is outside the diff; the invariant under test is that NO gap
        // ever names a consumer that is inside the diff.)
        let closure = graph.impact_closure(&[FileId(0), FileId(1)]);
        assert!(
            closure
                .coordination_gap
                .iter()
                .all(|gap| gap.consumer_file != FileId(0) && gap.consumer_file != FileId(1)),
            "no gap may name an in-diff consumer: {:?}",
            closure.coordination_gap
        );
        // Specifically, the core->mid pair (consumer mid is in the diff) must not fire.
        assert!(
            !closure
                .coordination_gap
                .iter()
                .any(|gap| gap.changed_file == FileId(0) && gap.consumer_file == FileId(1)),
            "core->mid must not fire when mid is in the diff"
        );
    }

    #[test]
    fn re_export_chain_closure_equals_hand_computed_set() {
        let graph = build_re_export_graph();
        // Change impl.ts. Hand-computed closure through the re-export chain =
        // {barrel, consumer}: barrel re-exports impl (a graph edge), consumer
        // imports from barrel.
        let closure = graph.impact_closure(&[FileId(0)]);
        assert_eq!(closure.in_diff, vec![FileId(0)]);
        assert_eq!(closure.affected_not_shown, vec![FileId(1), FileId(2)]);
    }

    #[test]
    fn re_export_chain_coordination_gap_fires_through_barrel() {
        let graph = build_re_export_graph();
        // impl changed; re-export chain resolution credits impl.widget's reference
        // to the TRUE consumer (consumer.ts, FileId 2), which imports it through the
        // barrel. consumer is outside the diff -> fires on the real consumer (the
        // higher-signal target than the intermediate barrel).
        let closure = graph.impact_closure(&[FileId(0)]);
        assert_eq!(closure.coordination_gap.len(), 1);
        let gap = &closure.coordination_gap[0];
        assert_eq!(gap.changed_file, FileId(0));
        assert_eq!(gap.consumer_file, FileId(2));
        assert_eq!(gap.consumed_symbols, vec!["widget".to_string()]);
    }

    #[test]
    fn coordination_gap_dedups_per_consumer_pair_r2() {
        // R2: a consumer importing TWO symbols from one changed file is ONE gap
        // entry with both symbols, never two entries.
        let files = vec![file(0, "/p/src/core.ts"), file(1, "/p/src/app.ts")];
        let entry_points = vec![EntryPoint {
            path: PathBuf::from("/p/src/app.ts"),
            source: EntryPointSource::PackageJsonMain,
        }];
        let resolved = vec![
            ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/p/src/core.ts"),
                exports: vec![named_export("alpha"), named_export("beta")],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(1),
                path: PathBuf::from("/p/src/app.ts"),
                resolved_imports: vec![
                    named_import("./core", "alpha", FileId(0)),
                    named_import("./core", "beta", FileId(0)),
                ],
                ..Default::default()
            },
        ];
        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        let closure = graph.impact_closure(&[FileId(0)]);
        assert_eq!(
            closure.coordination_gap.len(),
            1,
            "R2: one entry per consumer pair"
        );
        assert_eq!(
            closure.coordination_gap[0].consumed_symbols,
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn closure_with_paths_relativizes_and_sorts() {
        let graph = build_reverse_dep_graph();
        let closure = graph.impact_closure(&[FileId(0)]);
        let paths = graph.closure_with_paths(&closure, Path::new("/p"));
        assert_eq!(paths.in_diff, vec!["src/core.ts".to_string()]);
        assert_eq!(
            paths.affected_not_shown,
            vec!["src/app.ts".to_string(), "src/mid.ts".to_string()]
        );
        assert_eq!(paths.coordination_gap.len(), 1);
        assert_eq!(paths.coordination_gap[0].changed_file, "src/core.ts");
        assert_eq!(paths.coordination_gap[0].consumer_file, "src/mid.ts");
    }

    #[test]
    fn empty_changed_set_yields_empty_closure() {
        let graph = build_reverse_dep_graph();
        let closure = graph.impact_closure(&[]);
        assert!(closure.in_diff.is_empty());
        assert!(closure.affected_not_shown.is_empty());
        assert!(closure.coordination_gap.is_empty());
    }
}
