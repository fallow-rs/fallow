use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_output::ReviewDeltas;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AnalysisOptions, AuditOptions, DecisionSurfaceOptions, DecisionSurfaceProgrammaticOutput,
    ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run,
        resolve_programmatic_analysis_context_deferred_workspace, workspace_roots_for_session,
    },
    decision_surface::{
        BoundaryAnchor, CoordinationAnchor, DEFAULT_DECISION_CAP, DecisionInputs,
        extract_decision_surface,
    },
};

use super::{ProgrammaticResult, root_envelope_mode};

/// Run changed-code decision-surface analysis through the typed programmatic API.
///
/// # Errors
///
/// Returns a structured error for invalid options, base-ref discovery failures,
/// git changed-file failures, or analysis failures.
pub fn run_decision_surface(
    options: &DecisionSurfaceOptions,
) -> ProgrammaticResult<DecisionSurfaceProgrammaticOutput> {
    let start = Instant::now();
    let audit_options = audit_options_for_decision_surface(options);
    let resolved_base = super::audit::resolve_audit_base_ref(&audit_options)?;
    let analysis = AnalysisOptions {
        changed_since: Some(resolved_base.git_ref.clone()),
        ..options.analysis.clone()
    };
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&analysis)?;
    let changed_files = changed_files_for_run(&resolved)?.unwrap_or_default();
    if changed_files.is_empty() {
        return Ok(DecisionSurfaceProgrammaticOutput {
            surface: fallow_output::DecisionSurface::default(),
            elapsed: start.elapsed(),
            envelope_mode: root_envelope_mode(),
            telemetry_analysis_run_id: None,
        });
    }

    let head = run_decision_analysis(&resolved, Some(&changed_files))?;
    let base = compute_base_decision_snapshot(options, &resolved.root, &resolved_base.git_ref)?;
    let deltas = build_decision_deltas(&head, &base);
    let surface = build_surface(options, &head, &deltas);

    Ok(DecisionSurfaceProgrammaticOutput {
        surface,
        elapsed: start.elapsed(),
        envelope_mode: root_envelope_mode(),
        telemetry_analysis_run_id: None,
    })
}

fn audit_options_for_decision_surface(options: &DecisionSurfaceOptions) -> AuditOptions {
    AuditOptions {
        analysis: options.analysis.clone(),
        base: options.base.clone(),
        ..AuditOptions::default()
    }
}

struct DecisionAnalysis {
    root: PathBuf,
    results: fallow_types::results::AnalysisResults,
    public_api: FxHashSet<String>,
    impact_closure: Option<fallow_engine::module_graph::ImpactClosurePaths>,
    export_lines: Option<FxHashMap<String, Vec<(String, u32)>>>,
    internal_consumers: Option<FxHashMap<String, u64>>,
    routing: fallow_output::RoutingFacts,
}

fn run_decision_analysis(
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
) -> ProgrammaticResult<DecisionAnalysis> {
    let session = super::dead_code::load_dead_code_session(
        &super::dead_code::default_dead_code_options_for_context(resolved),
        resolved,
    )?;
    let root = session.root().to_path_buf();
    let root_pkg = fallow_config::PackageJson::load(&root.join("package.json")).ok();
    let artifacts = session
        .analyze_dead_code_with_session_artifacts(false, true, changed_files.cloned())
        .map_err(|err| {
            ProgrammaticError::new(format!("decision-surface analysis failed: {err}"), 2)
                .with_code("FALLOW_DECISION_SURFACE_FAILED")
                .with_context("decision-surface")
        })?;
    let fallow_engine::session::AnalysisSessionArtifacts {
        analysis: mut output,
        changed_files,
        ..
    } = artifacts;
    let changed_files = changed_files.as_ref();

    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    if let Some(workspace_roots) = workspace_roots.as_ref() {
        fallow_engine::dead_code::filter_to_workspaces(&mut output.results, workspace_roots);
    }
    if let Some(changed_files) = changed_files {
        fallow_engine::dead_code::filter_by_changed_files(&mut output.results, changed_files);
    }

    let public_api = output
        .graph
        .as_ref()
        .map_or_else(FxHashSet::default, |graph| {
            crate::review_deltas::public_export_keys_for(
                graph,
                session.config(),
                root_pkg.as_ref(),
                session.workspaces(),
                &root,
            )
        });
    let impact_closure = output.graph.as_ref().and_then(|graph| {
        changed_files.and_then(|files| {
            fallow_engine::module_graph::impact_closure_for_changed_paths(graph, &root, files)
        })
    });
    let export_lines = output.graph.as_ref().and_then(|graph| {
        changed_files.and_then(|files| {
            fallow_engine::module_graph::export_lines_for_changed_paths(graph, &root, files)
        })
    });
    let internal_consumers = output.graph.as_ref().and_then(|graph| {
        changed_files.and_then(|files| {
            fallow_engine::module_graph::internal_consumers_for_changed_paths(graph, &root, files)
        })
    });
    let routing = changed_files.map_or_else(fallow_output::RoutingFacts::default, |files| {
        crate::routing::compute_routing(&root, session.config(), files)
    });

    Ok(DecisionAnalysis {
        root,
        results: output.results,
        public_api,
        impact_closure,
        export_lines,
        internal_consumers,
        routing,
    })
}

fn compute_base_decision_snapshot(
    options: &DecisionSurfaceOptions,
    current_root: &Path,
    base_ref: &str,
) -> ProgrammaticResult<DecisionSnapshot> {
    let worktree = super::audit::BaseWorktree::create(current_root, base_ref)?;
    let base_root = super::audit::base_analysis_root(current_root, worktree.path());
    let base_analysis = AnalysisOptions {
        root: Some(base_root),
        config_path: options.analysis.config_path.clone(),
        changed_since: None,
        explain: false,
        ..options.analysis.clone()
    };
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&base_analysis)?;
    let base = run_decision_analysis(&resolved, None)?;
    Ok(snapshot_from_decision_analysis(&base))
}

#[derive(Default)]
struct DecisionSnapshot {
    boundary_edges: FxHashSet<String>,
    cycles: FxHashSet<String>,
    public_api: FxHashSet<String>,
}

fn snapshot_from_decision_analysis(analysis: &DecisionAnalysis) -> DecisionSnapshot {
    DecisionSnapshot {
        boundary_edges: crate::review_deltas::boundary_edge_keys(
            &analysis.results.boundary_violations,
        ),
        cycles: crate::review_deltas::cycle_keys(
            &analysis.results.circular_dependencies,
            &analysis.root,
        ),
        public_api: analysis.public_api.clone(),
    }
}

fn build_decision_deltas(head: &DecisionAnalysis, base: &DecisionSnapshot) -> ReviewDeltas {
    let head_snapshot = snapshot_from_decision_analysis(head);
    fallow_output::ReviewDeltas {
        boundary_introduced: crate::review_deltas::introduced_keys(
            &head_snapshot.boundary_edges,
            &base.boundary_edges,
        ),
        cycle_introduced: crate::review_deltas::introduced_keys(
            &head_snapshot.cycles,
            &base.cycles,
        ),
        public_api_added: crate::review_deltas::introduced_keys(
            &head_snapshot.public_api,
            &base.public_api,
        ),
    }
}

fn build_surface(
    options: &DecisionSurfaceOptions,
    head: &DecisionAnalysis,
    deltas: &ReviewDeltas,
) -> fallow_output::DecisionSurface {
    let boundary_anchors = boundary_anchors(head, deltas);
    let mut coordination = coordination_anchors(head.impact_closure.as_ref());
    let resolve_line = export_line_resolver(head.export_lines.as_ref());
    for anchor in &mut coordination {
        anchor.line = resolve_line(&anchor.changed_file, &anchor.consumed_symbols);
    }
    let public_api_anchor_line = deltas.public_api_added.first().map_or(0, |key| {
        let mut parts = key.splitn(2, "::");
        let path = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        resolve_line(path, &[name.to_string()])
    });
    let affected_not_shown = head
        .impact_closure
        .as_ref()
        .map_or(0, |closure| closure.affected_not_shown.len() as u64);
    let root = head.root.clone();
    let head_source = move |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    let rename_old_path = |_rel: &str| -> Option<String> { None };
    let internal_consumers_map = head.internal_consumers.as_ref();
    let internal_consumers = |rel: &str| -> u64 {
        internal_consumers_map
            .and_then(|map| map.get(rel))
            .copied()
            .unwrap_or(0)
    };
    extract_decision_surface(&DecisionInputs {
        deltas,
        boundary_anchors: &boundary_anchors,
        coordination: &coordination,
        public_api_anchor_line,
        affected_not_shown,
        routing: &head.routing,
        head_source: &head_source,
        rename_old_path: &rename_old_path,
        internal_consumers: &internal_consumers,
        cap: options.max_decisions.unwrap_or(DEFAULT_DECISION_CAP),
    })
}

fn boundary_anchors(head: &DecisionAnalysis, deltas: &ReviewDeltas) -> Vec<BoundaryAnchor> {
    let mut boundary_anchors = Vec::new();
    let mut seen_pairs = FxHashSet::default();
    for finding in &head.results.boundary_violations {
        let key = crate::review_deltas::boundary_edge_key(finding);
        if !deltas.boundary_introduced.contains(&key) || !seen_pairs.insert(key.clone()) {
            continue;
        }
        boundary_anchors.push(BoundaryAnchor {
            zone_pair_key: key,
            from_file: crate::audit_keys::relative_key_path(
                &finding.violation.from_path,
                &head.root,
            ),
            from_zone: finding.violation.from_zone.clone(),
            to_zone: finding.violation.to_zone.clone(),
            line: finding.violation.line,
        });
    }
    boundary_anchors
}

fn coordination_anchors(
    closure: Option<&fallow_engine::module_graph::ImpactClosurePaths>,
) -> Vec<CoordinationAnchor> {
    let Some(closure) = closure else {
        return Vec::new();
    };
    let mut by_file: FxHashMap<String, (u64, FxHashSet<String>)> = FxHashMap::default();
    for gap in &closure.coordination_gap {
        let entry = by_file
            .entry(gap.changed_file.clone())
            .or_insert_with(|| (0, FxHashSet::default()));
        entry.0 += 1;
        for symbol in &gap.consumed_symbols {
            entry.1.insert(symbol.clone());
        }
    }
    let mut anchors = by_file
        .into_iter()
        .map(|(changed_file, (consumer_count, symbols))| {
            let mut consumed_symbols: Vec<String> = symbols.into_iter().collect();
            consumed_symbols.sort_unstable();
            CoordinationAnchor {
                changed_file,
                consumed_symbols,
                consumer_count,
                line: 0,
            }
        })
        .collect::<Vec<_>>();
    anchors.sort_by(|a, b| a.changed_file.cmp(&b.changed_file));
    anchors
}

fn export_line_resolver(
    export_lines: Option<&FxHashMap<String, Vec<(String, u32)>>>,
) -> impl Fn(&str, &[String]) -> u32 + '_ {
    move |rel: &str, symbols: &[String]| -> u32 {
        let Some(exports) = export_lines.and_then(|map| map.get(rel)) else {
            return 0;
        };
        exports
            .iter()
            .find(|(name, _)| symbols.iter().any(|symbol| name == symbol))
            .or_else(|| exports.first())
            .map_or(0, |(_, line)| *line)
    }
}
