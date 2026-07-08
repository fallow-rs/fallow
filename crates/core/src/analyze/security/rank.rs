//! Reachability-weighted ranking of security candidates (issues #860 and #885).
//!
//! Reuses the existing module graph to rank `fallow security` candidates by
//! runtime reachability, source-backed evidence, module-level untrusted-source
//! reachability, reverse-dependency fan-in, boundary crossings, and dead-code
//! context. This is graph-side glue plus output ordering only: it touches
//! neither the extract cache nor detector semantics.
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
    SecurityAttackSurfaceEntry, SecurityCandidateBoundary, SecurityDeadCodeContext,
    SecurityDeadCodeKind, SecurityDefensiveBoundary, SecurityDefensiveControl, SecurityFinding,
    SecurityFindingKind, SecurityReachability, SecurityRuntimeState, SecuritySeverity,
    SecurityTaintFlow, SecurityZoneCrossing, TaintConfidence, TaintEndpoint, TaintPath, TraceHop,
    TraceHopRole,
};

use crate::discover::FileId;
use crate::graph::ModuleGraph;

use super::{LineOffsetsMap, byte_offset_to_line_col};
use fallow_security::catalogue;

const UNUSED_FILE_GUIDANCE: &str = "This sink sits in a file fallow also reports as unused. Verify the dead-code finding, then delete the file instead of hardening the sink.";
const UNUSED_EXPORT_GUIDANCE: &str = "This sink sits on an export fallow also reports as unused. Verify the dead-code finding, then remove the export instead of hardening the sink.";
const ZERO_CONTROL_PROMPT: &str = "No known control library was detected on this path. Should validation, sanitization, or auth be required before this sink?";
const CONTROL_PRESENT_PROMPT: &str = "Known defensive controls were detected on this path. Are they sufficient for this sink and untrusted input?";

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
        annotate_finding_dead_code(
            finding,
            &unused_file_paths,
            &module_by_path,
            line_offsets_by_file,
            unused_exports,
        );
    }
}

fn annotate_finding_dead_code(
    finding: &mut SecurityFinding,
    unused_file_paths: &FxHashSet<&Path>,
    module_by_path: &FxHashMap<&Path, &ModuleInfo>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    unused_exports: &[UnusedExportFinding],
) {
    if unused_file_paths.contains(finding.path.as_path()) {
        finding.dead_code = Some(SecurityDeadCodeContext {
            kind: SecurityDeadCodeKind::UnusedFile,
            export_name: None,
            line: None,
            guidance: UNUSED_FILE_GUIDANCE.to_string(),
        });
        prepend_dead_code_action(finding);
        return;
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
/// `boundary_crossings` maps each file path that participates in an
/// architecture-boundary violation found in the same run (importing or imported
/// side) to the `(from_zone, to_zone)` names of that crossing; a finding whose
/// anchor is a key is flagged `crosses_boundary` and its candidate records the
/// zone names (issue #900).
///
/// Sort order (descending priority): reachable-from-entry first, same-module
/// source-backed sinks, module-level source-reachable sinks, larger blast
/// radius, crosses-boundary, active-code over dead-code candidates, then the
/// existing deterministic `(path, line, col, category)` tiebreak so output stays
/// stable across runs.
/// Graph and run inputs that drive security-finding ranking.
pub struct SecurityRankingInput<'a> {
    pub graph: &'a ModuleGraph,
    pub modules: &'a [ModuleInfo],
    pub line_offsets_by_file: &'a LineOffsetsMap<'a>,
    pub declared_deps: &'a FxHashSet<String>,
    pub request_receivers: &'a FxHashSet<String>,
    pub boundary_crossings: &'a FxHashMap<PathBuf, (String, String)>,
}

pub fn rank_security_findings(input: &SecurityRankingInput<'_>, findings: &mut [SecurityFinding]) {
    if findings.is_empty() {
        return;
    }

    let context = SecurityRankingContext::build(input);

    for finding in findings.iter_mut() {
        enrich_ranked_security_finding(finding, &context);
    }

    findings.sort_by(compare_ranked_findings);
}

struct SecurityRankingContext<'a> {
    graph: &'a ModuleGraph,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    boundary_crossings: &'a FxHashMap<PathBuf, (String, String)>,
    path_to_id: FxHashMap<&'a Path, FileId>,
    source_index: UntrustedSourceIndex,
    modules_by_path: FxHashMap<&'a Path, &'a ModuleInfo>,
}

impl<'a> SecurityRankingContext<'a> {
    fn build(input: &SecurityRankingInput<'a>) -> Self {
        let graph = input.graph;
        let modules = input.modules;
        let path_to_id = graph
            .modules
            .iter()
            .map(|node| (node.path.as_path(), node.file_id))
            .collect();
        let source_index = UntrustedSourceIndex::build(
            graph,
            modules,
            input.declared_deps,
            input.request_receivers,
        );
        let modules_by_id: FxHashMap<FileId, &ModuleInfo> = modules
            .iter()
            .map(|module| (module.file_id, module))
            .collect();
        let modules_by_path = graph
            .modules
            .iter()
            .filter_map(|node| {
                modules_by_id
                    .get(&node.file_id)
                    .map(|module| (node.path.as_path(), *module))
            })
            .collect();

        Self {
            graph,
            line_offsets_by_file: input.line_offsets_by_file,
            boundary_crossings: input.boundary_crossings,
            path_to_id,
            source_index,
            modules_by_path,
        }
    }
}

fn enrich_ranked_security_finding(
    finding: &mut SecurityFinding,
    context: &SecurityRankingContext<'_>,
) {
    finding.reachability = context
        .path_to_id
        .get(finding.path.as_path())
        .map(|&file_id| {
            compute_reachability(
                context.graph,
                file_id,
                finding,
                context.boundary_crossings,
                &context.source_index,
                context.line_offsets_by_file,
            )
        });

    enrich_candidate(finding, context.boundary_crossings.get(&finding.path));
    finding.attack_surface = build_attack_surface(
        finding,
        &context.modules_by_path,
        context.line_offsets_by_file,
    );
    finding.severity = derive_security_severity(finding);
}

/// Rank ordering for two enriched security findings: entry-reachable, then
/// source-backed, then source-reachable, blast radius, boundary crossing, active
/// before dead, then a deterministic path/line/col/category tiebreak.
fn compare_ranked_findings(a: &SecurityFinding, b: &SecurityFinding) -> std::cmp::Ordering {
    let (ra, rb) = (a.reachability.as_ref(), b.reachability.as_ref());
    // Reachable-from-entry findings sort first.
    let reach_a = ra.is_some_and(|r| r.reachable_from_entry);
    let reach_b = rb.is_some_and(|r| r.reachable_from_entry);
    reach_b
        .cmp(&reach_a)
        // Then same-module source-backed sinks first.
        .then_with(|| b.source_backed.cmp(&a.source_backed))
        // Then module-level source-reachable sinks first.
        .then_with(|| {
            let source_a = ra.is_some_and(|r| r.reachable_from_untrusted_source);
            let source_b = rb.is_some_and(|r| r.reachable_from_untrusted_source);
            source_b.cmp(&source_a)
        })
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
}

/// Derive the verification-priority tier from existing security signals. This is
/// ranking only, not a vulnerability verdict.
#[must_use]
pub fn derive_security_severity(finding: &SecurityFinding) -> SecuritySeverity {
    if finding
        .runtime
        .as_ref()
        .is_some_and(|runtime| runtime.state == SecurityRuntimeState::RuntimeHot)
        || finding.candidate.boundary.client_server
        || finding
            .candidate
            .boundary
            .architecture_zone
            .as_ref()
            .is_some()
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.crosses_boundary)
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.reachable_from_entry && finding.source_backed)
    {
        return SecuritySeverity::High;
    }

    if finding.source_backed
        || finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.reachable_from_untrusted_source)
    {
        return SecuritySeverity::Medium;
    }

    SecuritySeverity::Low
}

/// Compute the reachability signal for a single anchor module.
fn compute_reachability(
    graph: &ModuleGraph,
    file_id: FileId,
    finding: &SecurityFinding,
    boundary_crossings: &FxHashMap<PathBuf, (String, String)>,
    source_index: &UntrustedSourceIndex,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> SecurityReachability {
    let reachable_from_entry = graph
        .modules
        .get(file_id.0 as usize)
        .is_some_and(|node| node.is_runtime_reachable());
    let source_trace = source_index.trace_for(graph, file_id, finding, line_offsets_by_file);

    SecurityReachability {
        reachable_from_entry,
        reachable_from_untrusted_source: source_trace.is_some(),
        // Tier the source association (issue #1093): arg-level when the sink
        // argument traces to a same-module source read (`source_backed`),
        // module-level when only the import graph connects a source module.
        // Present exactly when reachable_from_untrusted_source is true.
        taint_confidence: source_trace.as_ref().map(|_| {
            if finding.source_backed {
                TaintConfidence::ArgLevel
            } else {
                TaintConfidence::ModuleLevel
            }
        }),
        untrusted_source_hop_count: source_trace.as_ref().map(|source| source.hop_count),
        untrusted_source_trace: source_trace.map_or_else(Vec::new, |source| source.trace),
        blast_radius: transitive_dependent_count(graph, file_id),
        crosses_boundary: boundary_crossings.contains_key(&finding.path),
    }
}

/// Fill the candidate's boundary slot and the taint-flow triple from the
/// finding's trace and the reachability just computed (issue #900). Re-projection
/// only: client/server from a `ClientBoundary` trace hop, cross-module from the
/// untrusted-source hop count, and the architecture zone from the run's
/// boundary-crossing map. The `source_kind` and `sink` slots are set by the
/// detectors and left untouched here.
fn enrich_candidate(finding: &mut SecurityFinding, zone: Option<&(String, String)>) {
    let client_server = finding
        .trace
        .iter()
        .any(|hop| hop.role == TraceHopRole::ClientBoundary);
    let hop_count = finding
        .reachability
        .as_ref()
        .and_then(|reach| reach.untrusted_source_hop_count);
    finding.candidate.boundary = SecurityCandidateBoundary {
        client_server,
        cross_module: hop_count.is_some_and(|count| count > 0),
        architecture_zone: zone.map(|(from, to)| SecurityZoneCrossing {
            from: from.clone(),
            to: to.clone(),
        }),
    };
    finding.taint_flow = build_taint_flow(finding);
}

/// Build the `{ source, sink, path }` taint-flow triple when an untrusted source
/// is import-reachable to the sink. The full ordered hops are NOT duplicated:
/// `path` is the compact shape, the hops stay on `reachability.untrusted_source_trace`.
fn build_taint_flow(finding: &SecurityFinding) -> Option<SecurityTaintFlow> {
    let reach = finding.reachability.as_ref()?;
    if !reach.reachable_from_untrusted_source {
        return None;
    }
    let first = reach.untrusted_source_trace.first()?;
    let last = reach.untrusted_source_trace.last()?;
    let hop_count = reach.untrusted_source_hop_count.unwrap_or(0);
    Some(SecurityTaintFlow {
        source: TaintEndpoint {
            path: first.path.clone(),
            line: first.line,
            col: first.col,
        },
        sink: TaintEndpoint {
            path: last.path.clone(),
            line: last.line,
            col: last.col,
        },
        path: TaintPath {
            intra_module: hop_count == 0,
            cross_module_hops: hop_count,
        },
    })
}

fn build_attack_surface(
    finding: &SecurityFinding,
    modules_by_path: &FxHashMap<&Path, &ModuleInfo>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Option<SecurityAttackSurfaceEntry> {
    let flow = finding.taint_flow.as_ref()?;
    let reach = finding.reachability.as_ref()?;
    let path = reach.untrusted_source_trace.clone();
    if path.is_empty() {
        return None;
    }
    let controls = defensive_controls_for_path(&path, modules_by_path, line_offsets_by_file);
    let verification_prompt = if controls.is_empty() {
        ZERO_CONTROL_PROMPT
    } else {
        CONTROL_PRESENT_PROMPT
    }
    .to_string();

    Some(SecurityAttackSurfaceEntry {
        source: flow.source.clone(),
        sink: finding.candidate.sink.clone(),
        path,
        defensive_boundary: SecurityDefensiveBoundary {
            controls,
            verification_prompt,
        },
    })
}

fn defensive_controls_for_path(
    path: &[TraceHop],
    modules_by_path: &FxHashMap<&Path, &ModuleInfo>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<SecurityDefensiveControl> {
    let mut controls = Vec::new();
    let mut seen_files = FxHashSet::default();
    for hop in path {
        if !seen_files.insert(hop.path.as_path()) {
            continue;
        }
        let Some(module) = modules_by_path.get(hop.path.as_path()).copied() else {
            continue;
        };
        for control in &module.security_control_sites {
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, control.span_start);
            controls.push(SecurityDefensiveControl {
                kind: control.kind,
                path: hop.path.clone(),
                line,
                col,
                callee: control.callee_path.clone(),
            });
        }
    }
    controls.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.col.cmp(&b.col))
            .then_with(|| a.callee.cmp(&b.callee))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    controls.dedup_by(|a, b| {
        a.path == b.path
            && a.line == b.line
            && a.col == b.col
            && a.kind == b.kind
            && a.callee == b.callee
    });
    controls
}

#[derive(Debug, Clone, Copy)]
struct SourceParent {
    previous: FileId,
    import_span_start: Option<u32>,
}

struct UntrustedSourceIndex {
    source_for: Vec<Option<FileId>>,
    parent: Vec<Option<SourceParent>>,
}

struct UntrustedSourceTrace {
    hop_count: u32,
    trace: Vec<TraceHop>,
}

impl UntrustedSourceIndex {
    fn build(
        graph: &ModuleGraph,
        modules: &[ModuleInfo],
        declared_deps: &FxHashSet<String>,
        request_receivers: &FxHashSet<String>,
    ) -> Self {
        let modules_by_id: FxHashMap<FileId, &ModuleInfo> = modules
            .iter()
            .map(|module| (module.file_id, module))
            .collect();
        let mut source_for = vec![None; graph.modules.len()];
        let mut parent = vec![None; graph.modules.len()];
        let mut queue: VecDeque<FileId> = VecDeque::new();

        for node in &graph.modules {
            let Some(module) = modules_by_id.get(&node.file_id) else {
                continue;
            };
            if !module_contains_untrusted_source(module, declared_deps, request_receivers) {
                continue;
            }
            let idx = node.file_id.0 as usize;
            if idx >= source_for.len() || source_for[idx].is_some() {
                continue;
            }
            source_for[idx] = Some(node.file_id);
            queue.push_back(node.file_id);
        }

        while let Some(current) = queue.pop_front() {
            let Some(source_id) = source_for.get(current.0 as usize).copied().flatten() else {
                continue;
            };
            for (target, all_type_only, span) in graph.outgoing_edge_summaries(current) {
                if all_type_only {
                    continue;
                }
                let idx = target.0 as usize;
                if idx >= source_for.len() || source_for[idx].is_some() {
                    continue;
                }
                source_for[idx] = Some(source_id);
                parent[idx] = Some(SourceParent {
                    previous: current,
                    import_span_start: span,
                });
                queue.push_back(target);
            }
        }

        Self { source_for, parent }
    }

    fn trace_for(
        &self,
        graph: &ModuleGraph,
        sink_id: FileId,
        finding: &SecurityFinding,
        line_offsets_by_file: &LineOffsetsMap<'_>,
    ) -> Option<UntrustedSourceTrace> {
        if !is_source_reachability_candidate(finding) {
            return None;
        }
        let source_id = self.source_for.get(sink_id.0 as usize).copied().flatten()?;
        let mut ids = vec![sink_id];
        let mut current = sink_id;
        while current != source_id {
            let parent = self.parent.get(current.0 as usize).copied().flatten()?;
            current = parent.previous;
            ids.push(current);
        }
        ids.reverse();
        let hop_count = u32::try_from(ids.len().saturating_sub(1)).unwrap_or(u32::MAX);

        if source_id == sink_id {
            return Some(Self::same_module_trace(finding, hop_count));
        }

        let trace = self.multi_hop_trace(graph, &ids, finding, line_offsets_by_file)?;
        Some(UntrustedSourceTrace { hop_count, trace })
    }

    /// Build the two-hop arg-level / module-level trace for a sink whose source
    /// read is in the same module (issue #1093).
    fn same_module_trace(finding: &SecurityFinding, hop_count: u32) -> UntrustedSourceTrace {
        // Arg-level (source_backed): anchor the source node at the real
        // source read and label it `UntrustedSource` (a specific read is
        // implicated). Module-level (source elsewhere in the same file, no
        // arg trace): keep line 1 and label `ModuleSource` so the node is
        // never read as a proven value path (issue #1093). `source_read` is
        // Some exactly when `source_backed`.
        let (source_line, source_col, source_role) = finding
            .source_read
            .map_or((1, 0, TraceHopRole::ModuleSource), |(line, col)| {
                (line, col, TraceHopRole::UntrustedSource)
            });
        UntrustedSourceTrace {
            hop_count,
            trace: vec![
                TraceHop {
                    path: finding.path.clone(),
                    line: source_line,
                    col: source_col,
                    role: source_role,
                },
                TraceHop {
                    path: finding.path.clone(),
                    line: finding.line,
                    col: finding.col,
                    role: TraceHopRole::Sink,
                },
            ],
        }
    }

    /// Build the cross-module trace from the resolved source -> sink id chain.
    fn multi_hop_trace(
        &self,
        graph: &ModuleGraph,
        ids: &[FileId],
        finding: &SecurityFinding,
        line_offsets_by_file: &LineOffsetsMap<'_>,
    ) -> Option<Vec<TraceHop>> {
        let mut trace = Vec::with_capacity(ids.len().saturating_add(1));
        for (idx, &file_id) in ids.iter().enumerate() {
            let path = graph.modules.get(file_id.0 as usize)?.path.clone();
            if idx == ids.len() - 1 {
                trace.push(TraceHop {
                    path,
                    line: finding.line,
                    col: finding.col,
                    role: TraceHopRole::Sink,
                });
                continue;
            }

            let Some(&next_id) = ids.get(idx + 1) else {
                continue;
            };
            let next_parent = self.parent.get(next_id.0 as usize).copied().flatten();
            let (line, col) = next_parent
                .and_then(|p| p.import_span_start)
                .map_or((1, 0), |span| {
                    byte_offset_to_line_col(line_offsets_by_file, file_id, span)
                });
            trace.push(TraceHop {
                path,
                line,
                col,
                // Cross-module reachability is module-level by construction (the
                // specific source value is not shown to reach the sink), so the
                // origin hop is `ModuleSource`, not `UntrustedSource` (#1093).
                role: if idx == 0 {
                    TraceHopRole::ModuleSource
                } else {
                    TraceHopRole::Intermediate
                },
            });
        }
        Some(trace)
    }
}

fn is_source_reachability_candidate(finding: &SecurityFinding) -> bool {
    matches!(finding.kind, SecurityFindingKind::TaintedSink)
        && finding.category.as_deref() != Some(super::hardcoded_secret::CATEGORY_ID)
}

fn module_contains_untrusted_source(
    module: &ModuleInfo,
    declared_deps: &FxHashSet<String>,
    request_receivers: &FxHashSet<String>,
) -> bool {
    let cat = catalogue();
    module.tainted_bindings.iter().any(|binding| {
        cat.matching_source_for_deps_with_receivers(
            &binding.source_path,
            declared_deps,
            request_receivers,
        )
        .is_some()
    }) || module.security_sinks.iter().any(|sink| {
        sink.arg_source_paths.iter().any(|path| {
            cat.matching_source_for_deps_with_receivers(path, declared_deps, request_receivers)
                .is_some()
        })
    }) || module.member_accesses.iter().any(|access| {
        let full_path = format!("{}.{}", access.object, access.member);
        cat.matching_source_for_deps_with_receivers(&full_path, declared_deps, request_receivers)
            .is_some()
            || cat
                .matching_source_for_deps_with_receivers(
                    &access.object,
                    declared_deps,
                    request_receivers,
                )
                .is_some()
    })
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
    use fallow_types::extract::{
        MemberAccess, SecurityControlKind, SecurityControlSite, TaintedBinding,
    };
    use fallow_types::output::{FixActionType, IssueAction};
    use fallow_types::output_dead_code::{UnusedExportFinding, UnusedFileFinding};
    use fallow_types::results::{
        SecurityDeadCodeKind, SecurityFindingKind, SecurityRuntimeContext, TraceHop, TraceHopRole,
        UnusedExport, UnusedFile,
    };

    use crate::graph::ModuleGraph;
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};

    const ROOT: &str = "/proj";

    /// Build a graph from `file_names` and `(from, to)` import edges. `entry`
    /// indices become runtime entry points (so their cones are runtime-reachable).
    fn build_graph(file_names: &[&str], edges: &[(usize, usize)], entry: &[usize]) -> ModuleGraph {
        let edges: Vec<(usize, usize, bool)> =
            edges.iter().map(|&(from, to)| (from, to, false)).collect();
        build_graph_with_type_edges(file_names, &edges, entry)
    }

    fn build_graph_with_type_edges(
        file_names: &[&str],
        edges: &[(usize, usize, bool)],
        entry: &[usize],
    ) -> ModuleGraph {
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
                    .filter(|(from, _, _)| *from == f.id.0 as usize)
                    .map(|&(_, to, is_type_only)| ResolvedImport {
                        target: ResolveResult::InternalModule(FileId(to as u32)),
                        info: fallow_types::extract::ImportInfo {
                            source: format!("./{}", file_names[to]),
                            imported_name: fallow_types::extract::ImportedName::Default,
                            local_name: "x".to_string(),
                            is_type_only,
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
                    semantic_facts: Box::default(),
                    whole_object_uses: Box::default(),
                    has_cjs_exports: false,
                    has_angular_component_template_url: false,
                    unused_import_bindings: FxHashSet::default(),
                    type_referenced_import_bindings: vec![],
                    value_referenced_import_bindings: vec![],
                    namespace_object_aliases: vec![],
                    exported_factory_returns: Box::default(),
                    type_member_types: Box::default(),
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

    /// Test wrapper: accepts the boundary-anchor path set (the pre-#900 shape)
    /// and lifts each path into the `(from_zone, to_zone)` map the production
    /// signature now takes, with placeholder zone names. Existing call sites that
    /// only assert on `crosses_boundary` stay unchanged.
    fn rank(
        graph: &ModuleGraph,
        boundary_anchor_paths: &FxHashSet<PathBuf>,
        findings: &mut [SecurityFinding],
    ) {
        let modules = Vec::new();
        let line_offsets = FxHashMap::default();
        let declared_deps = FxHashSet::default();
        let request_receivers = FxHashSet::default();
        let boundary_crossings: FxHashMap<PathBuf, (String, String)> = boundary_anchor_paths
            .iter()
            .map(|path| (path.clone(), ("from".to_string(), "to".to_string())))
            .collect();
        rank_security_findings(
            &SecurityRankingInput {
                graph,
                modules: &modules,
                line_offsets_by_file: &line_offsets,
                declared_deps: &declared_deps,
                request_receivers: &request_receivers,
                boundary_crossings: &boundary_crossings,
            },
            findings,
        );
    }

    fn rank_with_modules(
        graph: &ModuleGraph,
        modules: &[ModuleInfo],
        findings: &mut [SecurityFinding],
    ) {
        let line_offsets = FxHashMap::default();
        let declared_deps = FxHashSet::default();
        let request_receivers = FxHashSet::default();
        let boundary_crossings = FxHashMap::default();
        rank_security_findings(
            &SecurityRankingInput {
                graph,
                modules,
                line_offsets_by_file: &line_offsets,
                declared_deps: &declared_deps,
                request_receivers: &request_receivers,
                boundary_crossings: &boundary_crossings,
            },
            findings,
        );
    }

    fn module(file_id: u32) -> ModuleInfo {
        ModuleInfo {
            file_id: FileId(file_id),
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
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![],
            complexity: vec![],
            flag_uses: vec![],
            class_heritage: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
            injection_tokens: vec![],
            local_type_declarations: vec![],
            public_signature_type_references: vec![],
            namespace_object_aliases: vec![],
            iconify_prefixes: vec![],
            iconify_icon_names: vec![],
            auto_import_candidates: vec![],
            directives: vec![],
            client_only_dynamic_import_spans: vec![],
            security_sinks: vec![],
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: vec![],
            sanitized_sink_args: vec![],
            security_control_sites: vec![],
            callee_uses: vec![],
            misplaced_directives: vec![],
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
        }
    }

    fn member_source_module(file_id: u32) -> ModuleInfo {
        let mut module = module(file_id);
        module.member_accesses.push(MemberAccess {
            object: "req".to_string(),
            member: "body".to_string(),
        });
        module
    }

    fn tainted_binding_source_module(file_id: u32) -> ModuleInfo {
        let mut module = module(file_id);
        module.tainted_bindings.push(TaintedBinding {
            local: "body".to_string(),
            source_path: "req.body".to_string(),
            source_span_start: 0,
        });
        module
    }

    fn validation_control_module(file_id: u32) -> ModuleInfo {
        let mut module = module(file_id);
        module.security_control_sites.push(SecurityControlSite {
            kind: SecurityControlKind::Validation,
            callee_path: "schema.parse".to_string(),
            span_start: 0,
            span_end: 12,
        });
        module
    }

    fn finding(name: &str) -> SecurityFinding {
        use fallow_types::results::{SecurityCandidate, SecurityCandidateSink};
        let path = PathBuf::from(ROOT).join(name);
        SecurityFinding {
            finding_id: String::new(),
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path: path.clone(),
            line: 1,
            col: 0,
            evidence: "candidate".to_string(),
            trace: vec![TraceHop {
                path: path.clone(),
                line: 1,
                col: 0,
                role: TraceHopRole::Sink,
            }],
            actions: Vec::<IssueAction>::new(),
            dead_code: None,
            reachability: None,
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path,
                    line: 1,
                    col: 0,
                    category: Some("dangerous-html".to_string()),
                    cwe: Some(79),
                    callee: None,
                    url_shape: None,
                },
                boundary: SecurityCandidateBoundary::default(),
                network: None,
            },
            taint_flow: None,
            runtime: None,
            attack_surface: None,
        }
    }

    fn reachability(
        reachable_from_entry: bool,
        reachable_from_untrusted_source: bool,
        crosses_boundary: bool,
    ) -> SecurityReachability {
        SecurityReachability {
            reachable_from_entry,
            reachable_from_untrusted_source,
            taint_confidence: None,
            untrusted_source_hop_count: None,
            untrusted_source_trace: vec![],
            blast_radius: 1,
            crosses_boundary,
        }
    }

    #[test]
    fn derives_low_severity_for_baseline_candidate() {
        assert_eq!(
            derive_security_severity(&finding("sink.ts")),
            SecuritySeverity::Low
        );
    }

    #[test]
    fn derives_medium_severity_for_source_signals() {
        let mut source_backed = finding("source-backed.ts");
        source_backed.source_backed = true;

        let mut source_reachable = finding("source-reachable.ts");
        source_reachable.reachability = Some(reachability(false, true, false));

        assert_eq!(
            derive_security_severity(&source_backed),
            SecuritySeverity::Medium
        );
        assert_eq!(
            derive_security_severity(&source_reachable),
            SecuritySeverity::Medium
        );
    }

    #[test]
    fn derives_high_severity_for_boundary_entry_and_runtime_signals() {
        let mut client_boundary = finding("client-boundary.ts");
        client_boundary.candidate.boundary.client_server = true;

        let mut architecture_boundary = finding("architecture-boundary.ts");
        architecture_boundary.candidate.boundary.architecture_zone = Some(SecurityZoneCrossing {
            from: "web".to_string(),
            to: "server".to_string(),
        });

        let mut crossed_boundary = finding("crossed-boundary.ts");
        crossed_boundary.reachability = Some(reachability(false, false, true));

        let mut source_backed_entry = finding("source-backed-entry.ts");
        source_backed_entry.source_backed = true;
        source_backed_entry.reachability = Some(reachability(true, false, false));

        let mut runtime_hot = finding("runtime-hot.ts");
        runtime_hot.runtime = Some(SecurityRuntimeContext {
            state: SecurityRuntimeState::RuntimeHot,
            function: "handler".to_string(),
            line: 1,
            invocations: Some(500),
            stable_id: Some("fallow:fn:test".to_string()),
            evidence: Some("runtime hot path".to_string()),
        });

        for finding in [
            client_boundary,
            architecture_boundary,
            crossed_boundary,
            source_backed_entry,
            runtime_hot,
        ] {
            assert_eq!(derive_security_severity(&finding), SecuritySeverity::High);
        }
    }

    #[test]
    fn reachable_from_entry_sorts_first() {
        // entry(0) -> reachable(1); orphan(2) is not in any entry cone.
        let graph = build_graph(&["entry.ts", "reachable.ts", "orphan.ts"], &[(0, 1)], &[0]);
        let empty = FxHashSet::default();
        // Seed in the "wrong" order to prove the sort moves the reachable one up.
        let mut findings = vec![finding("orphan.ts"), finding("reachable.ts")];
        rank(&graph, &empty, &mut findings);

        assert!(findings[0].path.ends_with("reachable.ts"));
        assert!(
            findings[0]
                .reachability
                .as_ref()
                .expect("ranked")
                .reachable_from_entry
        );
        assert!(findings[1].path.ends_with("orphan.ts"));
        assert!(
            !findings[1]
                .reachability
                .as_ref()
                .expect("ranked")
                .reachable_from_entry
        );
    }

    #[test]
    fn attack_surface_records_detected_controls_on_source_to_sink_path() {
        let graph = build_graph(&["source.ts", "sink.ts"], &[(0, 1)], &[0]);
        let modules = vec![
            tainted_binding_source_module(0),
            validation_control_module(1),
        ];
        let mut findings = vec![finding("sink.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        let surface = findings[0].attack_surface.as_ref().expect("surface entry");
        assert_eq!(surface.source.path, PathBuf::from(ROOT).join("source.ts"));
        assert_eq!(surface.sink.path, PathBuf::from(ROOT).join("sink.ts"));
        assert_eq!(surface.defensive_boundary.controls.len(), 1);
        assert_eq!(
            surface.defensive_boundary.controls[0].kind,
            SecurityControlKind::Validation
        );
        assert!(
            surface
                .defensive_boundary
                .verification_prompt
                .contains("Are they sufficient")
        );
    }

    #[test]
    fn attack_surface_zero_control_prompt_is_a_question() {
        let graph = build_graph(&["source.ts", "sink.ts"], &[(0, 1)], &[0]);
        let modules = vec![tainted_binding_source_module(0), module(1)];
        let mut findings = vec![finding("sink.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        let prompt = &findings[0]
            .attack_surface
            .as_ref()
            .expect("surface entry")
            .defensive_boundary
            .verification_prompt;
        assert!(prompt.ends_with('?'));
        assert!(prompt.contains("No known control library"));
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
        rank(&graph, &empty, &mut findings);

        assert!(findings[0].path.ends_with("hub.ts"));
        let hub = findings[0].reachability.as_ref().expect("ranked");
        let leaf = findings[1].reachability.as_ref().expect("ranked");
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
        rank(&graph, &boundary, &mut findings);

        assert!(findings[0].path.ends_with("b.ts"));
        assert!(
            findings[0]
                .reachability
                .as_ref()
                .expect("ranked")
                .crosses_boundary
        );
        assert!(
            !findings[1]
                .reachability
                .as_ref()
                .expect("ranked")
                .crosses_boundary
        );
    }

    #[test]
    fn full_tie_is_deterministic_by_path() {
        let graph = build_graph(&["entry.ts", "a.ts", "b.ts"], &[(0, 1), (0, 2)], &[0]);
        let empty = FxHashSet::default();
        let mut findings = vec![finding("b.ts"), finding("a.ts")];
        rank(&graph, &empty, &mut findings);
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

        rank(&graph, &empty, &mut findings);

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
        rank(&graph, &empty, &mut findings);
        assert!(findings.is_empty());
    }

    #[test]
    fn untrusted_source_reachability_uses_value_import_path() {
        let graph = build_graph(&["handler.ts", "helper.ts"], &[(0, 1)], &[]);
        let modules = vec![member_source_module(0), module(1)];
        let mut findings = vec![finding("helper.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        let reach = findings[0].reachability.as_ref().expect("ranked");
        assert!(reach.reachable_from_untrusted_source);
        assert_eq!(reach.untrusted_source_hop_count, Some(1));
        // Cross-module reachability is module-level: the source node is labeled
        // `ModuleSource`, and the tier says `module-level` (issue #1093).
        assert_eq!(reach.taint_confidence, Some(TaintConfidence::ModuleLevel));
        assert_eq!(
            reach
                .untrusted_source_trace
                .iter()
                .map(|hop| hop.role)
                .collect::<Vec<_>>(),
            vec![TraceHopRole::ModuleSource, TraceHopRole::Sink]
        );
    }

    #[test]
    fn arg_level_same_file_finding_anchors_source_node_at_read_line() {
        // A source-backed finding with a resolved source-read line: the trace
        // source node points at that read and is labeled `UntrustedSource`, and
        // the tier is `arg-level` (issue #1093).
        let graph = build_graph(&["handler.ts"], &[], &[]);
        let modules = vec![tainted_binding_source_module(0)];
        let mut arg_level = finding("handler.ts");
        arg_level.source_backed = true;
        arg_level.source_read = Some((7, 4));
        let mut findings = vec![arg_level];

        rank_with_modules(&graph, &modules, &mut findings);

        let reach = findings[0].reachability.as_ref().expect("ranked");
        assert!(reach.reachable_from_untrusted_source);
        assert_eq!(reach.taint_confidence, Some(TaintConfidence::ArgLevel));
        let source_hop = reach.untrusted_source_trace.first().expect("source node");
        assert_eq!(source_hop.role, TraceHopRole::UntrustedSource);
        assert_eq!((source_hop.line, source_hop.col), (7, 4));
    }

    #[test]
    fn untrusted_source_reachability_skips_type_only_import_path() {
        let graph = build_graph_with_type_edges(&["handler.ts", "helper.ts"], &[(0, 1, true)], &[]);
        let modules = vec![member_source_module(0), module(1)];
        let mut findings = vec![finding("helper.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        let reach = findings[0].reachability.as_ref().expect("ranked");
        assert!(!reach.reachable_from_untrusted_source);
        assert_eq!(reach.untrusted_source_hop_count, None);
        assert!(reach.untrusted_source_trace.is_empty());
    }

    #[test]
    fn same_file_untrusted_source_and_sink_has_zero_hop_trace() {
        let graph = build_graph(&["handler.ts"], &[], &[]);
        let modules = vec![tainted_binding_source_module(0)];
        let mut findings = vec![finding("handler.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        let reach = findings[0].reachability.as_ref().expect("ranked");
        assert!(reach.reachable_from_untrusted_source);
        assert_eq!(reach.untrusted_source_hop_count, Some(0));
        // The finding is not source-backed (the module merely contains a source),
        // so the same-file source node is module-level: `ModuleSource`, never
        // `UntrustedSource` (issue #1093).
        assert_eq!(reach.taint_confidence, Some(TaintConfidence::ModuleLevel));
        assert_eq!(
            reach
                .untrusted_source_trace
                .iter()
                .map(|hop| hop.role)
                .collect::<Vec<_>>(),
            vec![TraceHopRole::ModuleSource, TraceHopRole::Sink]
        );
    }

    #[test]
    fn source_backed_sorts_ahead_of_module_level_source_when_entry_ties() {
        let graph = build_graph(
            &["entry.ts", "source.ts", "module.ts", "direct.ts"],
            &[(0, 1), (0, 2), (0, 3), (1, 2)],
            &[0],
        );
        let modules = vec![
            module(0),
            member_source_module(1),
            module(2),
            tainted_binding_source_module(3),
        ];
        let mut direct = finding("direct.ts");
        direct.source_backed = true;
        let mut findings = vec![finding("module.ts"), direct];

        rank_with_modules(&graph, &modules, &mut findings);

        assert!(findings[0].path.ends_with("direct.ts"));
        assert!(findings[0].source_backed);
        assert!(findings[1].path.ends_with("module.ts"));
        assert!(
            findings[1]
                .reachability
                .as_ref()
                .expect("ranked")
                .reachable_from_untrusted_source
        );
    }

    #[test]
    fn runtime_entry_reachability_sorts_before_module_source_reachability() {
        let graph = build_graph(
            &["entry.ts", "reachable.ts", "source.ts", "module.ts"],
            &[(0, 1), (2, 3)],
            &[0],
        );
        let modules = vec![module(0), module(1), member_source_module(2), module(3)];
        let mut findings = vec![finding("module.ts"), finding("reachable.ts")];

        rank_with_modules(&graph, &modules, &mut findings);

        assert!(findings[0].path.ends_with("reachable.ts"));
        assert!(
            findings[0]
                .reachability
                .as_ref()
                .expect("ranked")
                .reachable_from_entry
        );
        assert!(findings[1].path.ends_with("module.ts"));
        assert!(
            findings[1]
                .reachability
                .as_ref()
                .expect("ranked")
                .reachable_from_untrusted_source
        );
    }

    #[test]
    fn hardcoded_secret_and_client_server_leak_are_not_source_annotated() {
        let graph = build_graph(&["source.ts", "candidate.ts"], &[(0, 1)], &[]);
        let modules = vec![member_source_module(0), module(1)];
        let mut hardcoded = finding("candidate.ts");
        hardcoded.category = Some(super::super::hardcoded_secret::CATEGORY_ID.to_string());
        let mut leak = finding("candidate.ts");
        leak.kind = SecurityFindingKind::ClientServerLeak;
        leak.category = None;
        let mut findings = vec![hardcoded, leak];

        rank_with_modules(&graph, &modules, &mut findings);

        for finding in findings {
            let reach = finding.reachability.as_ref().expect("ranked");
            assert!(!reach.reachable_from_untrusted_source);
            assert!(reach.untrusted_source_trace.is_empty());
        }
    }
}
