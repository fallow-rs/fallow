//! `fallow audit --brief` (alias `fallow review`): a deterministic rendering
//! mode layered over the existing audit analysis.
//!
//! The brief answers "where do I look?" rather than "will CI block this?". It
//! is composition + rendering over the same [`crate::audit::AuditResult`] that
//! drives `fallow audit`; it runs no new analysis and, critically, ALWAYS exits
//! 0 so a reviewer or agent can read the orientation even when the underlying
//! audit verdict is `fail`. The verdict is still computed and carried in the
//! brief JSON informationally, but it never drives the exit code on this path.
//!
//! The JSON envelope is independently versioned and tagged as
//! `kind: "audit-brief"` so the brief shape evolves on its own cadence without
//! bumping the main `--format json` contract.

use std::process::ExitCode;

pub use fallow_output::{
    CoordinationGapFact, DiffTriage, GraphFacts, ImpactClosureFacts, PartitionFacts,
    ReviewBriefSchemaVersion, ReviewBriefSubtractSections, ReviewDeltas, ReviewEffort,
    ReviewUnitFact, RiskClass,
};
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::audit::AuditResult;
use crate::report::sink::outln;

pub type ReviewBriefOutput = fallow_output::StandardReviewBriefOutput;

/// A file count at or above which a changeset is classified [`RiskClass::High`].
const RISK_HIGH_FILES: usize = 20;
/// A net-line count at or above which a changeset is classified
/// [`RiskClass::High`]. Stub-only in v1 (net lines are not yet threaded); kept
/// as a named constant so the threshold is documented where the classifier
/// lives.
const RISK_HIGH_LINES: i64 = 500;
/// A file count at or above which a changeset is classified
/// [`RiskClass::Medium`].
const RISK_MEDIUM_FILES: usize = 5;
/// A net-line count at or above which a changeset is classified
/// [`RiskClass::Medium`].
const RISK_MEDIUM_LINES: i64 = 100;

/// The honest-scope note stamped on every coordination-gap entry (ADR-001).
const COORDINATION_GAP_NOTE: &str = "syntactic attention pointer, not a correctness proof";

/// Build the deltas from head sets vs a base set, sorted for determinism.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "callers always pass the audit FxHashSet key sets; generalizing the hasher adds noise"
)]
pub fn build_review_deltas(
    head_boundary: &FxHashSet<String>,
    base_boundary: &FxHashSet<String>,
    head_cycles: &FxHashSet<String>,
    base_cycles: &FxHashSet<String>,
    head_public_api: &FxHashSet<String>,
    base_public_api: &FxHashSet<String>,
) -> ReviewDeltas {
    use crate::audit::review_deltas::introduced_keys;
    ReviewDeltas {
        boundary_introduced: introduced_keys(head_boundary, base_boundary),
        cycle_introduced: introduced_keys(head_cycles, base_cycles),
        public_api_added: introduced_keys(head_public_api, base_public_api),
    }
}

/// Classify a changeset's risk purely from its size. `net_lines` is consulted
/// when present (it is `None` on the v1 file-level audit path).
#[must_use]
pub fn classify_risk(files: usize, net_lines: Option<i64>) -> RiskClass {
    let lines = net_lines.unwrap_or(0).abs();
    if files >= RISK_HIGH_FILES || lines >= RISK_HIGH_LINES {
        RiskClass::High
    } else if files >= RISK_MEDIUM_FILES || lines >= RISK_MEDIUM_LINES {
        RiskClass::Medium
    } else {
        RiskClass::Low
    }
}

/// Map a [`RiskClass`] to the suggested reviewer effort.
#[must_use]
pub fn review_effort_for(risk: RiskClass) -> ReviewEffort {
    match risk {
        RiskClass::Low => ReviewEffort::Glance,
        RiskClass::Medium => ReviewEffort::Review,
        RiskClass::High => ReviewEffort::DeepDive,
    }
}

/// Build the Stage 0 triage facts from the audit result.
#[must_use]
pub fn build_triage(result: &AuditResult) -> DiffTriage {
    let files = result.changed_files_count;
    // v1: no diff index is threaded into the file-level audit, so hunks and net
    // lines are honestly absent. They populate on `--diff-file`/`--diff-stdin`.
    let hunks = None;
    let net_lines = None;
    let risk_class = classify_risk(files, net_lines);
    DiffTriage {
        files,
        hunks,
        net_lines,
        risk_class,
        review_effort: review_effort_for(risk_class),
    }
}

/// Derive the Stage 1 graph facts from the analysis results plus the impact
/// closure.
///
/// `boundaries_touched` is the deduped, sorted boundary-violation zone set;
/// `reachable_from` is the impact closure's affected-not-shown set (modules the
/// changed code reaches / affects, none in the diff). `exports_added` /
/// `api_width_delta` stay stubbed until the export-surface delta.
#[must_use]
pub fn derive_graph_facts(
    results: &AnalysisResults,
    closure: Option<&fallow_engine::ImpactClosurePaths>,
) -> GraphFacts {
    let mut zones: FxHashSet<String> = FxHashSet::default();
    for finding in &results.boundary_violations {
        zones.insert(finding.violation.from_zone.clone());
        zones.insert(finding.violation.to_zone.clone());
    }
    let mut boundaries_touched: Vec<String> = zones.into_iter().collect();
    boundaries_touched.sort();

    let reachable_from = closure
        .map(|c| c.affected_not_shown.clone())
        .unwrap_or_default();

    GraphFacts {
        exports_added: 0,
        api_width_delta: 0,
        reachable_from,
        boundaries_touched,
    }
}

/// Build the Stage 3 impact-closure facts from the audit result's retained
/// closure (computed on the brief path). Returns an empty closure when no graph
/// was retained (the closure is `None`).
#[must_use]
fn build_impact_closure_facts(result: &AuditResult) -> ImpactClosureFacts {
    let Some(closure) = result
        .check
        .as_ref()
        .and_then(|c| c.impact_closure.as_ref())
    else {
        return ImpactClosureFacts::default();
    };
    let coordination_gap = closure
        .coordination_gap
        .iter()
        .map(|gap| CoordinationGapFact {
            changed_file: gap.changed_file.clone(),
            consumer_file: gap.consumer_file.clone(),
            consumed_symbols: gap.consumed_symbols.clone(),
            note: COORDINATION_GAP_NOTE.to_string(),
        })
        .collect();
    ImpactClosureFacts {
        affected_not_shown: closure.affected_not_shown.clone(),
        coordination_gap,
    }
}

/// Build the Stage 2 partition facts from the audit result's retained
/// partition+order (computed on the brief path). Returns an empty partition when
/// no graph was retained (the partition is `None`).
#[must_use]
fn build_partition_facts(result: &AuditResult) -> PartitionFacts {
    let Some(partition) = result
        .check
        .as_ref()
        .and_then(|c| c.partition_order.as_ref())
    else {
        return PartitionFacts::default();
    };
    let units = partition
        .units
        .iter()
        .map(|unit| ReviewUnitFact {
            module_dir: unit.module_dir.clone(),
            files: unit.files.clone(),
        })
        .collect();
    PartitionFacts {
        units,
        order: partition.order.clone(),
    }
}

/// Build the Stage 4 weighted focus map from the audit result's retained
/// per-file graph facts plus the deltas / coordination signals. Returns an
/// empty focus map when no graph facts were retained (off the brief path or no
/// changed file mapped to a module).
///
/// The boundary risk-zone signal reuses the `from_path` of boundary violations
/// whose introduced edge is in `deltas.boundary_introduced` (the same surface
/// the decision surface reads). The security taint signal is wired as an EMPTY
/// slice today: the brief path runs the bare dead-code analysis, not the opt-in
/// `fallow security` taint engine, so `results.security_findings` is empty. The
/// seam lights up the moment a security pass is threaded onto the brief, with no
/// focus-map code change.
#[must_use]
fn build_focus_map(result: &AuditResult, deltas: &ReviewDeltas) -> crate::audit_focus::FocusMap {
    use crate::audit_focus::{BoundaryZoneFile, FocusInputs, build_focus_map};

    let Some(check) = result.check.as_ref() else {
        return crate::audit_focus::FocusMap::default();
    };
    let Some(graph_facts) = check.focus_facts.as_ref() else {
        return crate::audit_focus::FocusMap::default();
    };
    let root = &check.config.root;

    // Boundary risk-zone files: the importing `from_path` of each boundary
    // violation whose introduced zone-pair edge is in the delta set, deduped.
    let mut seen_pairs: FxHashSet<String> = FxHashSet::default();
    let mut boundary_files: Vec<BoundaryZoneFile> = Vec::new();
    for finding in &check.results.boundary_violations {
        let key = crate::audit::review_deltas::boundary_edge_key(finding);
        if !deltas.boundary_introduced.contains(&key) || !seen_pairs.insert(key) {
            continue;
        }
        boundary_files.push(BoundaryZoneFile {
            from_file: crate::audit::keys::relative_key_path(&finding.violation.from_path, root),
        });
    }

    // Coordination-gap changed files (the signature-change change-shape proxy):
    // the changed files whose contract is consumed outside the diff.
    let coordination_changed_files: Vec<String> = check
        .impact_closure
        .as_ref()
        .map(|c| {
            let mut files: Vec<String> = c
                .coordination_gap
                .iter()
                .map(|gap| gap.changed_file.clone())
                .collect();
            files.sort_unstable();
            files.dedup();
            files
        })
        .unwrap_or_default();

    // Security taint touch: the brief path carries no security findings (the taint
    // engine is the opt-in `fallow security` command), so this is empty today. The
    // seam is a pure function of this slice; it lights up when a security pass is
    // threaded onto the brief.
    let taint_touched_files = taint_touched_files(result.check.as_ref());

    // Runtime evidence (paid): `Some` only on the `--runtime-coverage` path.
    // It weights hot files and enables safe-skip; `None` in free mode, where the
    // focus map degrades to the deterministic no-runtime baseline byte-for-byte.
    let runtime_focus = build_runtime_focus(result, root);

    build_focus_map(&FocusInputs {
        graph_facts,
        boundary_files: &boundary_files,
        public_api_added: &deltas.public_api_added,
        coordination_changed_files: &coordination_changed_files,
        taint_touched_files: &taint_touched_files,
        runtime: runtime_focus.as_ref(),
    })
}

/// Build the per-file [`crate::audit_focus::RuntimeFocus`] from the
/// runtime-coverage health report, or `None` when the run carried no
/// `--runtime-coverage` data (free mode, where the focus map stays byte-identical
/// to the no-runtime baseline).
///
/// Hot files come from the report's `hot_paths` (peak invocation per file). Cold
/// files are the runtime-proven-unused ones: a file with at least one
/// `safe_to_delete` finding, NO finding of any other verdict, and no hot path.
///
/// Honest boundary: the report's `findings` omit `active` functions, so a file
/// can carry a live, executed function this signal never sees. `hot_paths` only
/// surfaces functions at/above the configured hot threshold (`min_invocations_hot`,
/// default 100, raisable via `--min-invocations-hot`), so an `active` function in
/// the `[low_traffic .. hot)` band shows up in NEITHER list:
/// such a file, if its only retained finding is `safe_to_delete`, is classified
/// cold here despite having run. This is why the cold signal is never trusted on
/// its own: the safe-skip label additionally requires zero static risk and no
/// confidence flag, applies only to a file already in the diff, and is always
/// advisory (the skip stays in the escape-hatch list, never hidden). Paths are
/// normalized to the brief's root-relative forward-slashed space so the
/// focus-map joins are byte-exact.
fn build_runtime_focus(
    result: &AuditResult,
    root: &std::path::Path,
) -> Option<crate::audit_focus::RuntimeFocus> {
    let report = result.health.as_ref()?.report.runtime_coverage.as_ref()?;

    let hot_pairs: Vec<(String, u64)> = report
        .hot_paths
        .iter()
        .map(|hot| {
            (
                crate::audit::keys::relative_key_path(&hot.path, root),
                hot.invocations,
            )
        })
        .collect();

    // Partition findings into safe_to_delete (cold candidate) vs any other verdict
    // (the disqualifier that keeps a mixed-verdict file out of the cold set).
    let mut safe_to_delete: FxHashSet<String> = FxHashSet::default();
    let mut other_verdict: FxHashSet<String> = FxHashSet::default();
    for finding in &report.findings {
        let file = crate::audit::keys::relative_key_path(&finding.path, root);
        if matches!(
            finding.verdict,
            fallow_output::RuntimeCoverageVerdict::SafeToDelete
        ) {
            safe_to_delete.insert(file);
        } else {
            other_verdict.insert(file);
        }
    }

    reconcile_runtime_focus(hot_pairs, &safe_to_delete, &other_verdict)
}

/// Reconcile the projected runtime signals into a [`crate::audit_focus::RuntimeFocus`]:
/// peak-aggregate hot invocations per file, and keep a file cold only when it has
/// a `safe_to_delete` finding, no other-verdict finding, and no hot path (so the
/// hot and cold lists are disjoint by construction). Returns `None` when both
/// lists are empty. Pure (no I/O), so the mixed-verdict exclusion, the
/// hot-excludes-cold filter, and the peak aggregation are unit-tested without
/// constructing a full health report.
fn reconcile_runtime_focus(
    hot_pairs: Vec<(String, u64)>,
    safe_to_delete: &FxHashSet<String>,
    other_verdict: &FxHashSet<String>,
) -> Option<crate::audit_focus::RuntimeFocus> {
    use crate::audit_focus::{RuntimeFocus, RuntimeHotFile};

    // Peak invocation per hot file (max across the file's hot functions).
    let mut hot_by_file: rustc_hash::FxHashMap<String, u64> = rustc_hash::FxHashMap::default();
    for (file, invocations) in hot_pairs {
        let entry = hot_by_file.entry(file).or_insert(0);
        *entry = (*entry).max(invocations);
    }

    let mut hot_files: Vec<RuntimeHotFile> = hot_by_file
        .into_iter()
        .map(|(file, invocations)| RuntimeHotFile { file, invocations })
        .collect();
    hot_files.sort_by(|a, b| a.file.cmp(&b.file));
    let hot_set: FxHashSet<&str> = hot_files.iter().map(|hot| hot.file.as_str()).collect();

    let mut cold_files: Vec<String> = safe_to_delete
        .iter()
        .filter(|file| !other_verdict.contains(*file) && !hot_set.contains(file.as_str()))
        .cloned()
        .collect();
    cold_files.sort();

    if hot_files.is_empty() && cold_files.is_empty() {
        return None;
    }
    Some(RuntimeFocus {
        hot_files,
        cold_files,
    })
}

/// Collect the root-relative file paths a security source -> sink taint trace
/// touches, from any retained `security_findings` (anchor + every trace hop).
///
/// Today the brief path runs the bare dead-code analysis, so `security_findings`
/// is empty and this returns an empty Vec (the security-taint seam contributes
/// 0). The function is a pure projection over the findings slice, so the moment a
/// future epic threads a security pass onto the brief, the focus map's taint
/// signal lights up with no focus-map code change.
fn taint_touched_files(check: Option<&crate::check::CheckResult>) -> Vec<String> {
    let Some(check) = check else {
        return Vec::new();
    };
    let root = &check.config.root;
    let mut touched: FxHashSet<String> = FxHashSet::default();
    for finding in &check.results.security_findings {
        touched.insert(crate::audit::keys::relative_key_path(&finding.path, root));
        for hop in &finding.trace {
            touched.insert(crate::audit::keys::relative_key_path(&hop.path, root));
        }
    }
    let mut files: Vec<String> = touched.into_iter().collect();
    files.sort();
    files
}

/// Assemble the structured [`ReviewBriefOutput`] for an audit result. Pure: no
/// timestamps, no randomness, so two runs over the same tree serialize
/// byte-identically.
#[must_use]
pub fn build_brief_output(result: &AuditResult) -> ReviewBriefOutput {
    let triage = build_triage(result);
    let closure = result
        .check
        .as_ref()
        .and_then(|c| c.impact_closure.as_ref());
    let deltas = result.review_deltas.clone().unwrap_or_default();
    let mut graph_facts = result.check.as_ref().map_or_else(
        || GraphFacts {
            exports_added: 0,
            api_width_delta: 0,
            reachable_from: Vec::new(),
            boundaries_touched: Vec::new(),
        },
        |check| derive_graph_facts(&check.results, closure),
    );
    // The exports-aware delta fills the previously-stubbed export facts:
    // `exports_added` / `api_width_delta` count the public-API surface the change
    // widened, not raw internal churn.
    let added = deltas.public_api_added.len();
    graph_facts.exports_added = added;
    graph_facts.api_width_delta = i64::try_from(added).unwrap_or(i64::MAX);
    let partition = build_partition_facts(result);
    let impact_closure = build_impact_closure_facts(result);
    let focus = build_focus_map(result, &deltas);
    ReviewBriefOutput {
        schema_version: ReviewBriefSchemaVersion::default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: "audit-brief".to_string(),
        triage,
        graph_facts,
        partition,
        impact_closure,
        focus,
        deltas,
        weakening: result.weakening_signals.clone(),
        routing: result.routing.clone().unwrap_or_default(),
        decisions: result.decision_surface.clone().unwrap_or_default(),
    }
}

/// Build the reused "subtract" section (dead-code / duplication / complexity)
/// for the brief JSON value, mirroring `fallow audit --format json`.
fn build_brief_subtract_sections(
    result: &AuditResult,
) -> Result<ReviewBriefSubtractSections, ExitCode> {
    let mut obj = serde_json::Map::new();
    if let Some(ref check) = result.check {
        crate::audit::insert_audit_dead_code_json(&mut obj, result, check)?;
    }
    if let Some(ref dupes) = result.dupes {
        crate::audit::insert_audit_duplication_json(&mut obj, result, dupes)?;
    }
    if let Some(ref health) = result.health {
        crate::audit::insert_audit_health_json(&mut obj, result, health)?;
    }
    Ok(ReviewBriefSubtractSections {
        dead_code: obj.remove("dead_code"),
        duplication: obj.remove("duplication"),
        complexity: obj.remove("complexity"),
    })
}

/// Build the complete brief JSON value: the versioned brief header, the
/// informational audit verdict header, the triage + graph-facts stages, and the
/// reused subtract section.
fn build_brief_json(result: &AuditResult) -> Result<serde_json::Value, ExitCode> {
    let brief = build_brief_output(result);
    let audit_header = fallow_api::build_audit_header_map(crate::audit::audit_json_header_input(
        result,
    ))
    .map_err(|err| {
        crate::error::emit_error(
            &format!("JSON serialization error: {err}"),
            2,
            fallow_config::OutputFormat::Json,
        )
    })?;
    let subtract = build_brief_subtract_sections(result)?;
    fallow_output::build_review_brief_json_output(&brief, audit_header, subtract).map_err(|err| {
        crate::error::emit_error(
            &format!("JSON serialization error: {err}"),
            2,
            fallow_config::OutputFormat::Json,
        )
    })
}

/// Render the brief as JSON. Always returns `SUCCESS`; a serialization failure
/// surfaces the error but the brief contract still exits 0.
fn print_brief_json(result: &AuditResult) -> ExitCode {
    match build_brief_json(result) {
        Ok(output) => {
            let Ok(output) = fallow_output::serialize_review_brief_json_output(
                output,
                crate::output_runtime::current_root_envelope_mode(),
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) else {
                return ExitCode::SUCCESS;
            };
            let _ = crate::report::emit_json(&output, "audit-brief");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::SUCCESS,
    }
}

/// Render the brief in human / compact / markdown form: a short orientation
/// header (scope, risk, effort, boundaries) followed by the same findings
/// sections `fallow audit` prints.
fn print_brief_human(result: &AuditResult, quiet: bool, explain: bool, show_deprioritized: bool) {
    let brief = build_brief_output(result);

    if !quiet {
        eprintln!();
        // The decision surface is the apex; it LEADS (collapse-by-default).
        print_decision_surface_human(&brief.decisions);
        // The upstream stages are the decision surface's drill-down derivation.
        eprintln!(
            "Review brief (drill-down): {} changed file{} vs {} \u{00b7} risk {} \u{00b7} effort {}",
            result.changed_files_count,
            crate::report::plural(result.changed_files_count),
            result.base_ref,
            risk_label(brief.triage.risk_class),
            effort_label(brief.triage.review_effort),
        );
        if !brief.graph_facts.boundaries_touched.is_empty() {
            eprintln!(
                "  boundaries touched: {}",
                brief.graph_facts.boundaries_touched.join(", ")
            );
        }
        print_partition_human(&brief.partition);
        print_impact_closure_human(&brief.impact_closure);
        print_focus_human(&brief.focus, show_deprioritized);
        print_deltas_human(&brief.deltas);
        print_weakening_human(&brief.weakening);
        print_routing_human(&brief.routing);
    }

    // Always render the findings sections so the brief shows WHERE to look, even
    // when the underlying verdict is a fail. Headers stay off (the brief owns its
    // own header line above).
    crate::audit::print_audit_findings(result, quiet, explain, false);
}

/// Print the Stage 2 partition + order on the human brief: the by-module units
/// and the dependency-sensible review order (definitions before consumers).
/// Caller has already gated on `!quiet`. Renders nothing when no unit was
/// computed (no graph retained, or every changed file is non-source).
fn print_partition_human(partition: &PartitionFacts) {
    if partition.units.is_empty() {
        return;
    }
    eprintln!(
        "  partition: {} unit{} (by module)",
        partition.units.len(),
        crate::report::plural(partition.units.len()),
    );
    if !partition.order.is_empty() {
        let labeled: Vec<String> = partition.order.iter().map(|dir| unit_label(dir)).collect();
        eprintln!("  review order: {}", labeled.join(" \u{2192} "));
    }
}

/// Label a unit's module directory for human output; the empty root-group key
/// renders as `<root>` so it is not a blank token.
fn unit_label(module_dir: &str) -> String {
    if module_dir.is_empty() {
        "<root>".to_string()
    } else {
        module_dir.to_string()
    }
}

/// Print the Stage 3 impact-closure summary on the human brief: the count of
/// affected-but-not-shown files and each coordination gap (the precise
/// inter-module attention pointer). Caller has already gated on `!quiet`.
fn print_impact_closure_human(closure: &ImpactClosureFacts) {
    if !closure.affected_not_shown.is_empty() {
        eprintln!(
            "  impact closure: {} file{} affected beyond the diff",
            closure.affected_not_shown.len(),
            crate::report::plural(closure.affected_not_shown.len()),
        );
    }
    for gap in &closure.coordination_gap {
        eprintln!(
            "  coordination gap: {} consumes {} from {} (not in this diff)",
            gap.consumer_file,
            gap.consumed_symbols.join(", "),
            gap.changed_file,
        );
    }
}

/// Print the Stage 4 weighted focus map on the human brief: the ranked
/// `review-here` units (with reason + any low-confidence flag), then the
/// de-prioritized count as a collapsed escape hatch. `--show-deprioritized`
/// re-expands the full de-prioritized list ("show me what you de-prioritized").
/// Caller has already gated on `!quiet`. Renders nothing when no unit was scored.
fn print_focus_human(focus: &crate::audit_focus::FocusMap, show_deprioritized: bool) {
    if focus.total_units() == 0 {
        return;
    }
    if !focus.review_here.is_empty() {
        eprintln!(
            "  focus: {} unit{} to review here (of {} changed)",
            focus.review_here.len(),
            crate::report::plural(focus.review_here.len()),
            focus.total_units(),
        );
        for unit in &focus.review_here {
            eprintln!(
                "    [{}] {}: {}",
                unit.label.token(),
                unit.file,
                unit.reason
            );
            for flag in &unit.confidence {
                eprintln!("      confidence {}", flag.message());
            }
        }
    }
    if focus.deprioritized.is_empty() {
        return;
    }
    if show_deprioritized {
        eprintln!("  de-prioritized ({}):", focus.deprioritized.len());
        for unit in &focus.deprioritized {
            eprintln!(
                "    [{}] {}: {}",
                unit.label.token(),
                unit.file,
                unit.reason
            );
            for flag in &unit.confidence {
                eprintln!("      confidence {}", flag.message());
            }
        }
    } else {
        eprintln!(
            "  de-prioritized: {} unit{} (run with --show-deprioritized to list)",
            focus.deprioritized.len(),
            crate::report::plural(focus.deprioritized.len()),
        );
    }
}

/// Print the diff-aware deltas (6.A): boundary/cycle introduced and the
/// exports-aware public-API surface delta (batch-consolidated per R1). Caller
/// has already gated on `!quiet`.
fn print_deltas_human(deltas: &ReviewDeltas) {
    for edge in &deltas.boundary_introduced {
        eprintln!("  new boundary edge: {edge} (not present at base)");
    }
    for cycle in &deltas.cycle_introduced {
        eprintln!("  new circular dependency: {cycle} (not present at base)");
    }
    if !deltas.public_api_added.is_empty() {
        eprintln!(
            "  public API surface widened by {} export{} (exports-aware)",
            deltas.public_api_added.len(),
            crate::report::plural(deltas.public_api_added.len()),
        );
    }
}

/// Print the weakening signals (6.F headline). Advisory, reviewer-private.
fn print_weakening_human(signals: &[crate::audit::weakening::WeakeningSignal]) {
    if signals.is_empty() {
        return;
    }
    eprintln!(
        "  weakening signals ({}, reviewer-private, advisory):",
        signals.len()
    );
    for signal in signals {
        eprintln!(
            "    {}: {} in {}",
            weakening_label(signal.kind),
            signal.evidence,
            signal.file,
        );
    }
}

/// Print the ownership routing (6.D): per-unit expert + bus-factor flag.
fn print_routing_human(routing: &crate::audit::routing::RoutingFacts) {
    for unit in &routing.units {
        if unit.expert.is_empty() {
            continue;
        }
        let bus = if unit.bus_factor_one {
            " (bus-factor 1)"
        } else {
            ""
        };
        eprintln!(
            "  review {}: ask {}{bus}",
            unit.file,
            unit.expert.join(", "),
        );
    }
}

/// Print the decision surface (the apex, 6.G): the ranked, capped set of
/// consequential structural decisions, each as a framed judgment question with
/// its routed expert. Caller has already gated on `!quiet`. Leads the brief.
fn print_decision_surface_human(surface: &crate::audit_decision_surface::DecisionSurface) {
    if surface.decisions.is_empty() {
        eprintln!("Decisions: none (no consequential structural decision in this change)");
        eprintln!();
        return;
    }
    eprintln!("Decisions to make ({}):", surface.decisions.len());
    for (i, decision) in surface.decisions.iter().enumerate() {
        // Taste ownership: the question first (never an answer), then the honest
        // graph fact, then the named trade-off. The human reads reversibility from
        // the count; the tool never labels the door or recommends a choice.
        eprintln!(
            "  {}. [{}] {}",
            i + 1,
            decision.category.tag(),
            decision.question
        );
        if !decision.tradeoff.is_empty() {
            eprintln!("     trade-off: {}", decision.tradeoff);
        }
        if !decision.expert.is_empty() {
            let bus = if decision.bus_factor_one {
                " (bus-factor 1)"
            } else {
                ""
            };
            eprintln!("     ask: {}{bus}", decision.expert.join(", "));
        }
    }
    if let Some(note) = &surface.truncated {
        eprintln!("  ... {}", note.reason);
    }
    eprintln!();
}

fn weakening_label(kind: crate::audit::weakening::WeakeningKind) -> &'static str {
    use crate::audit::weakening::WeakeningKind;
    match kind {
        WeakeningKind::TestWeakened => "test weakened",
        WeakeningKind::ThresholdLowered => "threshold lowered",
        WeakeningKind::SuppressionAdded => "suppression added",
        WeakeningKind::SecurityCheckRemoved => "security check removed",
    }
}

fn risk_label(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::Low => "low",
        RiskClass::Medium => "medium",
        RiskClass::High => "high",
    }
}

fn effort_label(effort: ReviewEffort) -> &'static str {
    match effort {
        ReviewEffort::Glance => "glance",
        ReviewEffort::Review => "review",
        ReviewEffort::DeepDive => "deep-dive",
    }
}

/// Print the brief and return an exit code that is ALWAYS `SUCCESS`.
///
/// This is the exit-0 seam: `fallow review` (and `fallow audit --brief`) never
/// gate on the audit verdict. The verdict is still carried in the JSON output
/// informationally. Format dispatch mirrors `print_audit_result`, but every arm
/// forces success: JSON renders the brief envelope; human / compact / markdown
/// render the brief orientation header plus findings; any other format
/// (SARIF, CodeClimate, PR/review envelopes, badge) is rendered through the
/// standard audit path and then forced to success so the format stays usable
/// without re-implementing it for the brief.
#[must_use]
pub fn print_brief_result(
    result: &AuditResult,
    quiet: bool,
    explain: bool,
    show_deprioritized: bool,
) -> ExitCode {
    use fallow_config::OutputFormat;

    match result.output {
        OutputFormat::Json => print_brief_json(result),
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown => {
            print_brief_human(result, quiet, explain, show_deprioritized);
            ExitCode::SUCCESS
        }
        _ => {
            // For machine/CI formats not specific to the brief, delegate to the
            // standard audit renderer for the body, then force success: the
            // brief invariant is exit-0 regardless of verdict.
            let _ = crate::audit::print_audit_result(result, quiet, explain);
            ExitCode::SUCCESS
        }
    }
}

/// Render the SEPARABLE decision-surface envelope (the `decision_surface` MCP
/// tool's output + `fallow decision-surface`). Emits ONLY the ranked, capped
/// decisions with structured `actions[]`, never the full brief. Always exit 0.
///
/// JSON renders the typed decision-surface envelope (`kind:
/// "decision-surface"`); human / compact / markdown render the apex header.
#[must_use]
pub fn print_decision_surface_result(result: &AuditResult, quiet: bool) -> ExitCode {
    use fallow_config::OutputFormat;

    let surface = result.decision_surface.clone().unwrap_or_default();
    match result.output {
        OutputFormat::Json => {
            let output = crate::audit_decision_surface::build_decision_surface_output(&surface);
            match fallow_output::serialize_decision_surface_json_output(
                output,
                crate::output_runtime::current_root_envelope_mode(),
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => {
                    let _ = crate::report::emit_json(&value, "decision-surface");
                    ExitCode::SUCCESS
                }
                Err(_) => ExitCode::SUCCESS,
            }
        }
        _ => {
            if !quiet {
                print_decision_surface_human(&surface);
            }
            ExitCode::SUCCESS
        }
    }
}

/// Render the agent-contract WALKTHROUGH GUIDE: the digest (brief +
/// decision surface), the review direction, the JSON schema the agent returns,
/// and the deterministic graph-snapshot pin. JSON renders the typed guide
/// envelope (`kind: "review-walkthrough-guide"`). Every format emits the guide
/// as JSON: the guide is an agent-facing contract, not a human walkthrough.
/// Always exit 0.
#[must_use]
pub fn print_walkthrough_guide_result(result: &AuditResult) -> ExitCode {
    let guide = crate::audit_walkthrough::build_guide_from_result(result);
    if let Ok(value) = fallow_output::serialize_walkthrough_guide_json_output(
        guide,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        let _ = crate::report::emit_json(&value, "review-walkthrough-guide");
    }
    ExitCode::SUCCESS
}

/// Ingest the agent's judgment JSON from `path` and POST-VALIDATE it against
/// the live graph: reject unanchored signal_ids (anti-hallucination), refuse the
/// whole payload when the echoed graph-snapshot hash is stale (the tree moved).
/// JSON renders the typed walkthrough-validation envelope (`kind:
/// "review-walkthrough-validation"`). Always exit 0 (advisory).
///
/// A path that cannot be read yields an empty agent payload (default `""` hash),
/// which never matches the current hash, so it is refused as stale, the safe
/// direction: a missing or garbled agent file never accepts a judgment.
#[must_use]
pub fn print_walkthrough_file_result(result: &AuditResult, path: &std::path::Path) -> ExitCode {
    let contents = std::fs::read_to_string(path).unwrap_or_default();
    let agent = crate::audit_walkthrough::parse_agent_walkthrough(&contents);
    let surface = result.decision_surface.clone().unwrap_or_default();
    let current_hash = result.graph_snapshot_hash.clone().unwrap_or_default();
    let change_anchor_ids =
        crate::audit_walkthrough::change_anchor_allowlist(&result.change_anchors);
    let validation = crate::audit_walkthrough::validate_walkthrough(
        &agent,
        &surface,
        &change_anchor_ids,
        &current_hash,
    );
    if let Ok(value) = fallow_output::serialize_walkthrough_validation_json_output(
        validation,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        let _ = crate::report::emit_json(&value, "review-walkthrough-validation");
    }
    ExitCode::SUCCESS
}

/// Render the EXISTING walkthrough guide as a HUMAN terminal tour (or markdown
/// with `--format markdown`). The guide data is built unchanged by
/// [`crate::audit_walkthrough::build_guide_from_result`]; this only renders it.
///
/// Format dispatch on `result.output`:
/// - `Json` delegates verbatim to [`print_walkthrough_guide_result`], so
///   `--walkthrough --format json` is byte-identical to `--walkthrough-guide
///   --format json` (the json-reuse seam, zero duplication).
/// - `Markdown` emits a paste-into-PR markdown tour to STDOUT.
/// - every other format (Human / Compact / the CI envelopes) emits the colored
///   staged terminal tour: the Review Focus header + final status to stderr, the
///   tour body to stdout. The guide is advisory, never a CI gate envelope, so
///   SARIF / CodeClimate / PR-comment formats fall through to the human tour
///   rather than implying gate semantics.
///
/// `root` and `cache_dir` are threaded from `AuditOptions` because `AuditResult`
/// carries neither: `root` displays paths, `cache_dir` locates the local
/// viewed-state ledger. `mark_viewed` records files as viewed BEFORE rendering;
/// the render itself is read-only. Always exit 0, even when the verdict is Fail.
#[must_use]
pub fn print_walkthrough_human_result(
    result: &AuditResult,
    root: &std::path::Path,
    cache_dir: &std::path::Path,
    mark_viewed: &[std::path::PathBuf],
    show_cleared: bool,
    quiet: bool,
) -> ExitCode {
    use fallow_config::OutputFormat;

    // JSON reuses the single guide JSON path verbatim (no second serializer).
    if matches!(result.output, OutputFormat::Json) {
        return print_walkthrough_guide_result(result);
    }

    let guide = crate::audit_walkthrough::build_guide_from_result(result);
    record_walkthrough_marks(&guide, root, cache_dir, mark_viewed);

    // Load the viewed-state ledger ONCE and share it across both surfaces, so the
    // markdown render honors `--mark-viewed` the same way the human render does
    // (the two formats agree on the same on-disk state instead of markdown
    // silently ignoring it).
    let viewed = crate::walkthrough_state::load_viewed_state(cache_dir);

    if matches!(result.output, OutputFormat::Markdown) {
        let viewed_files = crate::report::walkthrough_viewed_files(&guide, &viewed);
        let markdown = fallow_api::build_walkthrough_markdown(&guide, root, &viewed_files);
        outln!("{markdown}");
        return ExitCode::SUCCESS;
    }

    let render = crate::report::build_walkthrough_human(&guide, &viewed, show_cleared);
    if !quiet {
        for line in &render.header {
            eprintln!("{line}");
        }
    }
    for line in &render.body {
        outln!("{line}");
    }
    if !quiet {
        eprintln!("{}", render.status);
    }
    ExitCode::SUCCESS
}

/// Record each `--mark-viewed` path as viewed against the current guide hash.
///
/// Paths are normalized to the guide's root-relative VIEW key (the guide stores
/// root-relative paths in `direction.order`). IO failures are swallowed: the
/// viewed-state is a local convenience and must never change the exit code.
fn record_walkthrough_marks(
    guide: &crate::audit_walkthrough::WalkthroughGuide,
    root: &std::path::Path,
    cache_dir: &std::path::Path,
    mark_viewed: &[std::path::PathBuf],
) {
    if mark_viewed.is_empty() {
        return;
    }
    let keys: Vec<String> = mark_viewed
        .iter()
        .map(|path| walkthrough_view_key(path, root))
        .collect();
    let _ = crate::walkthrough_state::mark_viewed(cache_dir, &keys, &guide.graph_snapshot_hash);
}

/// Normalize a `--mark-viewed` path to the guide's root-relative, forward-slashed
/// VIEW key, so a user can pass either an absolute or a relative path.
fn walkthrough_view_key(path: &std::path::Path, root: &std::path::Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fallow_config::{AuditGate, OutputFormat};
    use fallow_output::REVIEW_BRIEF_SCHEMA_VERSION;
    use rustc_hash::FxHashSet;

    use crate::audit::{AuditAttribution, AuditResult, AuditSummary, AuditVerdict};

    fn str_set(files: &[&str]) -> FxHashSet<String> {
        files.iter().map(|file| (*file).to_string()).collect()
    }

    // Producer: the runtime hot/cold reconciliation. A file with a peak hot
    // path is hot (peak = max over its functions); a file with only a
    // safe_to_delete finding is cold; a mixed-verdict file is excluded; a file
    // that is both safe_to_delete AND hot stays hot (disjoint lists).
    #[test]
    fn reconcile_runtime_focus_classifies_hot_cold_and_excludes_mixed() {
        let hot_pairs = vec![
            ("src/hot.ts".to_string(), 120),
            ("src/hot.ts".to_string(), 900),  // peak wins
            ("src/both.ts".to_string(), 300), // also safe_to_delete below -> stays hot
        ];
        let safe = str_set(&["src/cold.ts", "src/mixed.ts", "src/both.ts"]);
        let other = str_set(&["src/mixed.ts"]); // mixed.ts also has a review_required -> not cold
        let focus =
            super::reconcile_runtime_focus(hot_pairs, &safe, &other).expect("non-empty focus");

        // Hot files: peak-aggregated, sorted.
        let hot: Vec<(&str, u64)> = focus
            .hot_files
            .iter()
            .map(|hot| (hot.file.as_str(), hot.invocations))
            .collect();
        assert_eq!(hot, vec![("src/both.ts", 300), ("src/hot.ts", 900)]);

        // Cold = safe_to_delete minus other-verdict minus hot. Only `cold.ts`.
        assert_eq!(focus.cold_files, vec!["src/cold.ts".to_string()]);
    }

    // Producer: no signal at all -> None (free-mode fall-through).
    #[test]
    fn reconcile_runtime_focus_is_none_when_empty() {
        assert!(super::reconcile_runtime_focus(Vec::new(), &str_set(&[]), &str_set(&[])).is_none());
    }

    use super::*;

    fn audit_result(verdict: AuditVerdict, output: OutputFormat) -> AuditResult {
        AuditResult {
            verdict,
            summary: AuditSummary {
                dead_code_issues: 0,
                dead_code_has_errors: false,
                complexity_findings: 0,
                max_cyclomatic: None,
                duplication_clone_groups: 0,
            },
            attribution: AuditAttribution {
                gate: AuditGate::NewOnly,
                ..AuditAttribution::default()
            },
            base_snapshot: None,
            base_snapshot_skipped: false,
            changed_files_count: 0,
            changed_files: Vec::new(),
            base_ref: "origin/main".to_string(),
            base_description: None,
            head_sha: None,
            output,
            performance: false,
            check: None,
            dupes: None,
            health: None,
            elapsed: Duration::ZERO,
            review_deltas: None,
            weakening_signals: Vec::new(),
            routing: None,
            decision_surface: None,
            graph_snapshot_hash: None,
            change_anchors: Vec::new(),
        }
    }

    #[test]
    fn brief_mode_always_returns_success_even_when_verdict_is_fail() {
        // Human path.
        let human = audit_result(AuditVerdict::Fail, OutputFormat::Human);
        assert_eq!(
            print_brief_result(&human, true, false, false),
            ExitCode::SUCCESS
        );

        // JSON path.
        let json = audit_result(AuditVerdict::Fail, OutputFormat::Json);
        assert_eq!(
            print_brief_result(&json, true, false, false),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn brief_json_validates_against_audit_brief_schema_variant() {
        let result = audit_result(AuditVerdict::Fail, OutputFormat::Json);
        let value = fallow_output::serialize_review_brief_json_output(
            build_brief_json(&result).expect("brief json must build"),
            crate::output_runtime::current_root_envelope_mode(),
            crate::output_runtime::telemetry_analysis_run_id().as_deref(),
        )
        .expect("brief json must serialize");

        assert_eq!(value["kind"], "audit-brief");
        assert_eq!(value["command"], "audit-brief");
        assert_eq!(value["schema_version"], REVIEW_BRIEF_SCHEMA_VERSION);
    }

    #[test]
    fn brief_json_is_byte_identical_on_repeated_serialization() {
        // `elapsed: Duration::ZERO` and no telemetry: the brief JSON carries no
        // timestamps or randomness, so two builds serialize byte-identically.
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let first = build_brief_json(&result).expect("first build");
        let second = build_brief_json(&result).expect("second build");
        let first_str = serde_json::to_string_pretty(&first).expect("serialize first");
        let second_str = serde_json::to_string_pretty(&second).expect("serialize second");
        assert_eq!(first_str, second_str);
    }

    #[test]
    fn risk_class_thresholds_are_pure_functions_of_size() {
        assert_eq!(classify_risk(0, None), RiskClass::Low);
        assert_eq!(classify_risk(RISK_MEDIUM_FILES, None), RiskClass::Medium);
        assert_eq!(classify_risk(RISK_HIGH_FILES, None), RiskClass::High);
        assert_eq!(classify_risk(1, Some(RISK_HIGH_LINES)), RiskClass::High);
        assert_eq!(review_effort_for(RiskClass::High), ReviewEffort::DeepDive);
    }

    #[test]
    fn brief_json_includes_empty_impact_closure_when_no_graph_retained() {
        // check: None -> no closure; the impact_closure object must still be
        // present and empty so consumers can rely on its presence.
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let value = build_brief_json(&result).expect("brief json must build");
        assert!(value.get("impact_closure").is_some(), "{value}");
        assert_eq!(
            value["impact_closure"]["affected_not_shown"],
            serde_json::json!([])
        );
        assert_eq!(
            value["impact_closure"]["coordination_gap"],
            serde_json::json!([])
        );
    }

    #[test]
    fn derive_graph_facts_populates_reachable_from_from_closure() {
        use fallow_engine::{CoordinationGapPaths, ImpactClosurePaths};
        let results = AnalysisResults::default();
        let closure = ImpactClosurePaths {
            in_diff: vec!["src/core.ts".to_string()],
            affected_not_shown: vec!["src/app.ts".to_string(), "src/mid.ts".to_string()],
            coordination_gap: vec![CoordinationGapPaths {
                changed_file: "src/core.ts".to_string(),
                consumer_file: "src/mid.ts".to_string(),
                consumed_symbols: vec!["compute".to_string()],
            }],
        };
        let facts = derive_graph_facts(&results, Some(&closure));
        assert_eq!(
            facts.reachable_from,
            vec!["src/app.ts".to_string(), "src/mid.ts".to_string()]
        );
    }

    #[test]
    fn coordination_gap_fact_carries_honest_scope_note() {
        let gap = CoordinationGapFact {
            changed_file: "src/core.ts".to_string(),
            consumer_file: "src/mid.ts".to_string(),
            consumed_symbols: vec!["compute".to_string()],
            note: COORDINATION_GAP_NOTE.to_string(),
        };
        assert!(gap.note.contains("attention pointer"));
        assert!(gap.note.contains("not a correctness proof"));
    }
}
