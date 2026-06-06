//! Reachability-weighted ranking of security candidates (issue #860).
//!
//! Reuses the EXISTING module graph (runtime reachability + reverse-dependency
//! fan-in) to rank `fallow security` candidates so a candidate reachable from a
//! runtime/application entry point (route handlers, server entry, framework
//! runtime roots) surfaces above an isolated helper or script. This is graph-side
//! glue plus output ordering only: it touches neither the extract cache nor the
//! finding enum, and adds no new BFS infrastructure beyond a bounded fan-in walk
//! over the graph's reverse-dependency index.
//!
//! The ranking is a relative-ordering signal, NOT proof of exploitability:
//! candidates remain CANDIDATES for downstream agent verification.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ExportName, ModuleInfo};
use fallow_types::output::{FixAction, FixActionType, IssueAction};
use fallow_types::output_dead_code::{UnusedExportFinding, UnusedFileFinding};
use fallow_types::results::{
    SecurityDeadCodeContext, SecurityDeadCodeKind, SecurityFinding, SecurityFindingKind,
    SecurityReachability,
};

use crate::discover::FileId;
use crate::graph::ModuleGraph;

use super::{LineOffsetsMap, byte_offset_to_line_col};

const UNUSED_FILE_GUIDANCE: &str = "This sink sits in a file fallow also reports as unused. Verify the dead-code finding, then delete the file instead of hardening the sink.";
const UNUSED_EXPORT_GUIDANCE: &str = "This sink sits on an export fallow also reports as unused. Verify the dead-code finding, then remove the export instead of hardening the sink.";

/// Annotate tainted-sink candidates that overlap dead-code findings from the same
/// analysis run. Client-server leak findings stay unchanged because #884 was
/// narrowed to sink candidates.
pub fn annotate_dead_code_cross_links(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    line_offsets_by_file: &LineOffsetsMap<'_>,
    unused_files: &[UnusedFileFinding],
    unused_exports: &[UnusedExportFinding],
    findings: &mut [SecurityFinding],
) {
    if findings.is_empty() || (unused_files.is_empty() && unused_exports.is_empty()) {
        return;
    }

    let unused_file_paths: FxHashSet<&Path> =
        unused_files.iter().map(|f| f.file.path.as_path()).collect();
    let modules_by_id: FxHashMap<FileId, &ModuleInfo> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    let module_by_path: FxHashMap<&Path, &ModuleInfo> = graph
        .modules
        .iter()
        .filter_map(|node| {
            modules_by_id
                .get(&node.file_id)
                .map(|module| (node.path.as_path(), *module))
        })
        .collect();

    for finding in findings {
        if !matches!(finding.kind, SecurityFindingKind::TaintedSink) {
            continue;
        }
        if unused_file_paths.contains(finding.path.as_path()) {
            finding.dead_code = Some(SecurityDeadCodeContext {
                kind: SecurityDeadCodeKind::UnusedFile,
                export_name: None,
                line: None,
                guidance: UNUSED_FILE_GUIDANCE.to_string(),
            });
            prepend_dead_code_action(finding);
            continue;
        }

        if let Some(export) = matching_unused_export(
            module_by_path.get(finding.path.as_path()).copied(),
            line_offsets_by_file,
            unused_exports,
            finding,
        ) {
            finding.dead_code = Some(SecurityDeadCodeContext {
                kind: SecurityDeadCodeKind::UnusedExport,
                export_name: Some(export.export.export_name.clone()),
                line: Some(export.export.line),
                guidance: UNUSED_EXPORT_GUIDANCE.to_string(),
            });
            prepend_dead_code_action(finding);
        }
    }
}

fn matching_unused_export<'a>(
    module: Option<&ModuleInfo>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    unused_exports: &'a [UnusedExportFinding],
    finding: &SecurityFinding,
) -> Option<&'a UnusedExportFinding> {
    let same_file = unused_exports
        .iter()
        .filter(|export| export.export.path == finding.path);

    if let Some(module) = module {
        for export in same_file.clone() {
            let Some(info) = module
                .exports
                .iter()
                .find(|info| export_name_matches(&info.name, &export.export.export_name))
            else {
                continue;
            };
            let (start_line, _) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, info.span.start);
            let (end_line, _) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, info.span.end);
            if start_line <= finding.line && finding.line <= end_line.max(start_line) {
                return Some(export);
            }
        }
    }

    same_file
        .into_iter()
        .find(|export| export.export.line == finding.line)
}

fn export_name_matches(name: &ExportName, candidate: &str) -> bool {
    match name {
        ExportName::Named(name) => name == candidate,
        ExportName::Default => candidate == "default",
    }
}

fn prepend_dead_code_action(finding: &mut SecurityFinding) {
    let Some(context) = &finding.dead_code else {
        return;
    };
    let action = match context.kind {
        SecurityDeadCodeKind::UnusedFile => IssueAction::Fix(FixAction {
            kind: FixActionType::DeleteFile,
            auto_fixable: false,
            description: "Delete this unused file instead of hardening the sink".to_string(),
            note: Some(
                "Verify the unused-file finding before deleting production code".to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        }),
        SecurityDeadCodeKind::UnusedExport => IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveExport,
            auto_fixable: false,
            description: "Remove the unused export instead of hardening the sink".to_string(),
            note: context
                .export_name
                .as_ref()
                .map(|name| format!("Verify that export `{name}` is unused before removing it")),
            available_in_catalogs: None,
            suggested_target: None,
        }),
    };
    finding.actions.insert(0, action);
}

/// Rank security findings in place: fill each finding's [`SecurityReachability`]
/// from the graph, then re-sort so the highest-priority candidates sort first.
///
/// `boundary_anchor_paths` is the set of file paths that participate in an
/// architecture-boundary violation found in the same run (importing or imported
/// side); a finding whose anchor is in that set is flagged `crosses_boundary`.
///
/// Sort order (descending priority): reachable-from-entry first, then larger
/// blast radius, then crosses-boundary, then the existing deterministic
/// `(path, line, col, category)` tiebreak so output stays stable across runs.
pub fn rank_security_findings(
    graph: &ModuleGraph,
    boundary_anchor_paths: &FxHashSet<PathBuf>,
    findings: &mut [SecurityFinding],
) {
    if findings.is_empty() {
        return;
    }

    // O(1) path -> FileId index over graph modules, built once.
    let path_to_id: FxHashMap<&Path, FileId> = graph
        .modules
        .iter()
        .map(|node| (node.path.as_path(), node.file_id))
        .collect();

    for finding in findings.iter_mut() {
        let reachability = path_to_id.get(finding.path.as_path()).map(|&file_id| {
            compute_reachability(graph, file_id, &finding.path, boundary_anchor_paths)
        });
        finding.reachability = reachability;
    }

    findings.sort_by(|a, b| {
        let (ra, rb) = (a.reachability, b.reachability);
        // Reachable-from-entry findings sort first.
        let reach_a = ra.is_some_and(|r| r.reachable_from_entry);
        let reach_b = rb.is_some_and(|r| r.reachable_from_entry);
        reach_b
            .cmp(&reach_a)
            // Then larger blast radius first.
            .then_with(|| {
                let ba = ra.map_or(0, |r| r.blast_radius);
                let bb = rb.map_or(0, |r| r.blast_radius);
                bb.cmp(&ba)
            })
            // Then boundary-crossing candidates first.
            .then_with(|| {
                let ca = ra.is_some_and(|r| r.crosses_boundary);
                let cb = rb.is_some_and(|r| r.crosses_boundary);
                cb.cmp(&ca)
            })
            // Then active-code candidates before dead-code candidates.
            .then_with(|| a.dead_code.is_some().cmp(&b.dead_code.is_some()))
            // Deterministic tiebreak (matches the detectors' own ordering).
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.col.cmp(&b.col))
            .then_with(|| a.category.cmp(&b.category))
    });
}

/// Compute the reachability signal for a single anchor module.
fn compute_reachability(
    graph: &ModuleGraph,
    file_id: FileId,
    anchor_path: &Path,
    boundary_anchor_paths: &FxHashSet<PathBuf>,
) -> SecurityReachability {
    let reachable_from_entry = graph
        .modules
        .get(file_id.0 as usize)
        .is_some_and(|node| node.is_runtime_reachable());

    SecurityReachability {
        reachable_from_entry,
        blast_radius: transitive_dependent_count(graph, file_id),
        crosses_boundary: boundary_anchor_paths.contains(anchor_path),
    }
}

/// Count the distinct modules that transitively depend on `target` (fan-in) via
/// the graph's reverse-dependency index. Bounded BFS with a visited set; the
/// target itself is excluded from the count.
fn transitive_dependent_count(graph: &ModuleGraph, target: FileId) -> u32 {
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    let mut queue: VecDeque<FileId> = VecDeque::new();
    queue.push_back(target);
    visited.insert(target);

    while let Some(current) = queue.pop_front() {
        let Some(dependents) = graph.reverse_deps.get(current.0 as usize) else {
            continue;
        };
        for &dep in dependents {
            if visited.insert(dep) {
                queue.push_back(dep);
            }
        }
    }

    // Exclude the target itself; saturate into u32 for the wire type.
    u32::try_from(visited.len().saturating_sub(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use fallow_types::output::{FixActionType, IssueAction};
    use fallow_types::output_dead_code::{UnusedExportFinding, UnusedFileFinding};
    use fallow_types::results::{
        SecurityDeadCodeKind, SecurityFindingKind, TraceHop, TraceHopRole, UnusedExport, UnusedFile,
    };

    use crate::graph::ModuleGraph;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};

    const ROOT: &str = "/proj";

    /// Build a graph from `file_names` and `(from, to)` import edges. `entry`
    /// indices become runtime entry points (so their cones are runtime-reachable).
    fn build_graph(file_names: &[&str], edges: &[(usize, usize)], entry: &[usize]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = file_names
            .iter()
            .enumerate()
            .map(|(i, name)| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(ROOT).join(name),
                size_bytes: 100,
            })
            .collect();

        let resolved: Vec<ResolvedModule> = files
            .iter()
            .map(|f| {
                let imports: Vec<ResolvedImport> = edges
                    .iter()
                    .filter(|(from, _)| *from == f.id.0 as usize)
                    .map(|&(_, to)| ResolvedImport {
                        target: ResolveResult::InternalModule(FileId(to as u32)),
                        info: fallow_types::extract::ImportInfo {
                            source: format!("./{}", file_names[to]),
                            imported_name: fallow_types::extract::ImportedName::Default,
                            local_name: "x".to_string(),
                            is_type_only: false,
                            from_style: false,
                            span: oxc_span::Span::new(0, 10),
                            source_span: oxc_span::Span::new(0, 10),
                        },
                    })
                    .collect();
                ResolvedModule {
                    file_id: f.id,
                    path: f.path.clone(),
                    exports: vec![],
                    re_exports: vec![],
                    resolved_imports: imports,
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
                }
            })
            .collect();

        let entry_points: Vec<EntryPoint> = entry
            .iter()
            .map(|&i| EntryPoint {
                path: files[i].path.clone(),
                source: EntryPointSource::ManualEntry,
            })
            .collect();

        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    fn finding(name: &str) -> SecurityFinding {
        let path = PathBuf::from(ROOT).join(name);
        SecurityFinding {
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path: path.clone(),
            line: 1,
            col: 0,
            evidence: "candidate".to_string(),
            trace: vec![TraceHop {
                path,
                line: 1,
                col: 0,
                role: TraceHopRole::Sink,
            }],
            actions: Vec::<IssueAction>::new(),
            dead_code: None,
            reachability: None,
            source_backed: false,
        }
    }

    #[test]
    fn reachable_from_entry_sorts_first() {
        // entry(0) -> reachable(1); orphan(2) is not in any entry cone.
        let graph = build_graph(&["entry.ts", "reachable.ts", "orphan.ts"], &[(0, 1)], &[0]);
        let empty = FxHashSet::default();
        // Seed in the "wrong" order to prove the sort moves the reachable one up.
        let mut findings = vec![finding("orphan.ts"), finding("reachable.ts")];
        rank_security_findings(&graph, &empty, &mut findings);

        assert!(findings[0].path.ends_with("reachable.ts"));
        assert!(
            findings[0]
                .reachability
                .expect("ranked")
                .reachable_from_entry
        );
        assert!(findings[1].path.ends_with("orphan.ts"));
        assert!(
            !findings[1]
                .reachability
                .expect("ranked")
                .reachable_from_entry
        );
    }

    #[test]
    fn higher_blast_radius_wins_among_reachable() {
        // entry(0) imports both hub(1) and leaf(2); extra(3) also imports hub(1).
        // hub has fan-in {entry, extra} = 2; leaf has fan-in {entry} = 1.
        let graph = build_graph(
            &["entry.ts", "hub.ts", "leaf.ts", "extra.ts"],
            &[(0, 1), (0, 2), (3, 1)],
            &[0, 3],
        );
        let empty = FxHashSet::default();
        let mut findings = vec![finding("leaf.ts"), finding("hub.ts")];
        rank_security_findings(&graph, &empty, &mut findings);

        assert!(findings[0].path.ends_with("hub.ts"));
        let hub = findings[0].reachability.expect("ranked");
        let leaf = findings[1].reachability.expect("ranked");
        assert!(hub.reachable_from_entry && leaf.reachable_from_entry);
        assert!(
            hub.blast_radius > leaf.blast_radius,
            "hub {} should exceed leaf {}",
            hub.blast_radius,
            leaf.blast_radius
        );
    }

    #[test]
    fn boundary_crossing_breaks_tie() {
        // Two equally-reachable, equal-fan-in siblings imported by entry(0).
        let graph = build_graph(&["entry.ts", "a.ts", "b.ts"], &[(0, 1), (0, 2)], &[0]);
        let mut boundary = FxHashSet::default();
        boundary.insert(PathBuf::from(ROOT).join("b.ts"));
        // Seed a-first; b crosses a boundary so it should sort ahead.
        let mut findings = vec![finding("a.ts"), finding("b.ts")];
        rank_security_findings(&graph, &boundary, &mut findings);

        assert!(findings[0].path.ends_with("b.ts"));
        assert!(findings[0].reachability.expect("ranked").crosses_boundary);
        assert!(!findings[1].reachability.expect("ranked").crosses_boundary);
    }

    #[test]
    fn full_tie_is_deterministic_by_path() {
        let graph = build_graph(&["entry.ts", "a.ts", "b.ts"], &[(0, 1), (0, 2)], &[0]);
        let empty = FxHashSet::default();
        let mut findings = vec![finding("b.ts"), finding("a.ts")];
        rank_security_findings(&graph, &empty, &mut findings);
        assert!(findings[0].path.ends_with("a.ts"));
        assert!(findings[1].path.ends_with("b.ts"));
    }

    #[test]
    fn dead_code_cross_link_marks_unused_file_sink() {
        let graph = build_graph(&["dead.ts"], &[], &[]);
        let mut findings = vec![finding("dead.ts")];
        let unused_files = vec![UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from(ROOT).join("dead.ts"),
        })];
        let line_offsets = FxHashMap::default();

        annotate_dead_code_cross_links(
            &graph,
            &[],
            &line_offsets,
            &unused_files,
            &[],
            &mut findings,
        );

        let context = findings[0].dead_code.as_ref().expect("dead-code context");
        assert_eq!(context.kind, SecurityDeadCodeKind::UnusedFile);
        assert_eq!(context.export_name, None);
        match &findings[0].actions[0] {
            IssueAction::Fix(action) => assert_eq!(action.kind, FixActionType::DeleteFile),
            other => panic!("expected delete-file action, got {other:?}"),
        }
    }

    #[test]
    fn dead_code_cross_link_marks_same_line_unused_export_sink() {
        let graph = build_graph(&["sink.ts"], &[], &[]);
        let mut findings = vec![finding("sink.ts")];
        let unused_exports = vec![UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(ROOT).join("sink.ts"),
            export_name: "dangerous".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })];
        let line_offsets = FxHashMap::default();

        annotate_dead_code_cross_links(
            &graph,
            &[],
            &line_offsets,
            &[],
            &unused_exports,
            &mut findings,
        );

        let context = findings[0].dead_code.as_ref().expect("dead-code context");
        assert_eq!(context.kind, SecurityDeadCodeKind::UnusedExport);
        assert_eq!(context.export_name.as_deref(), Some("dangerous"));
        assert_eq!(context.line, Some(1));
        match &findings[0].actions[0] {
            IssueAction::Fix(action) => assert_eq!(action.kind, FixActionType::RemoveExport),
            other => panic!("expected remove-export action, got {other:?}"),
        }
    }

    #[test]
    fn dead_code_cross_link_skips_client_server_leak_findings() {
        let graph = build_graph(&["dead.ts"], &[], &[]);
        let mut findings = vec![finding("dead.ts")];
        findings[0].kind = SecurityFindingKind::ClientServerLeak;
        let unused_files = vec![UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from(ROOT).join("dead.ts"),
        })];
        let line_offsets = FxHashMap::default();

        annotate_dead_code_cross_links(
            &graph,
            &[],
            &line_offsets,
            &unused_files,
            &[],
            &mut findings,
        );

        assert!(findings[0].dead_code.is_none());
        assert!(findings[0].actions.is_empty());
    }

    #[test]
    fn active_code_sorts_ahead_of_dead_code_when_rank_signals_tie() {
        let graph = build_graph(
            &["entry.ts", "active.ts", "dead.ts"],
            &[(0, 1), (0, 2)],
            &[0],
        );
        let empty = FxHashSet::default();
        let mut dead = finding("dead.ts");
        dead.dead_code = Some(SecurityDeadCodeContext {
            kind: SecurityDeadCodeKind::UnusedFile,
            export_name: None,
            line: None,
            guidance: UNUSED_FILE_GUIDANCE.to_string(),
        });
        let mut findings = vec![dead, finding("active.ts")];

        rank_security_findings(&graph, &empty, &mut findings);

        assert!(findings[0].path.ends_with("active.ts"));
        assert!(findings[0].dead_code.is_none());
        assert!(findings[1].path.ends_with("dead.ts"));
        assert!(findings[1].dead_code.is_some());
    }

    #[test]
    fn empty_findings_is_noop() {
        let graph = build_graph(&["entry.ts"], &[], &[0]);
        let empty = FxHashSet::default();
        let mut findings: Vec<SecurityFinding> = vec![];
        rank_security_findings(&graph, &empty, &mut findings);
        assert!(findings.is_empty());
    }
}
