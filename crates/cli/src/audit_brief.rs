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
/// [`RiskClass::High`].
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
        dependency_added: Vec::new(),
        dependency_major_bumped: Vec::new(),
    }
}

/// Classify a changeset's risk purely from its size. `net_lines` is consulted
/// when diff evidence is available.
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
pub fn build_triage(
    result: &AuditResult,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> DiffTriage {
    let files = result.changed_files_count;
    let hunks = diff_index.map(fallow_output::DiffIndex::hunk_count);
    let net_lines = diff_index.map(fallow_output::DiffIndex::net_lines);
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
/// `boundaries_touched` is the deduped, sorted boundary-violation zone set.
/// `exports_added` / `api_width_delta` stay stubbed until the export-surface
/// delta. The set of modules the changed code reaches is Stage 3's impact
/// closure, which owns both its magnitude and its paths.
#[must_use]
pub fn derive_graph_facts(results: &AnalysisResults) -> GraphFacts {
    let mut zones: FxHashSet<String> = FxHashSet::default();
    for finding in &results.boundary_violations {
        zones.insert(finding.violation.from_zone.clone());
        zones.insert(finding.violation.to_zone.clone());
    }
    let mut boundaries_touched: Vec<String> = zones.into_iter().collect();
    boundaries_touched.sort();

    GraphFacts {
        exports_added: 0,
        api_width_delta: 0,
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
    ImpactClosureFacts::new(&closure.affected_not_shown, coordination_gap)
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
        independent_slices: partition.independent_slices.clone(),
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
    build_brief_output_with_diff(result, result.diff_index.as_ref())
}

/// Assemble the structured [`ReviewBriefOutput`] with optional diff evidence.
/// The caller owns diff discovery so this reusable builder stays pure.
#[must_use]
pub fn build_brief_output_with_diff(
    result: &AuditResult,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> ReviewBriefOutput {
    let triage = build_triage(result, diff_index);
    let deltas = result.review_deltas.clone().unwrap_or_default();
    let mut graph_facts = result.check.as_ref().map_or_else(
        || GraphFacts {
            exports_added: 0,
            api_width_delta: 0,
            boundaries_touched: Vec::new(),
        },
        |check| derive_graph_facts(&check.results),
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
        branching: build_branching_report(result),
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
        ownership: result.ownership.clone(),
        decisions: result.decision_surface.clone().unwrap_or_default(),
    }
}

/// Compare branching across the accounting set, or `None` when there is no
/// base to compare against.
///
/// Both sides are restricted to the changed files before comparing. The base
/// pass analyzes the whole base worktree, so an unrestricted comparison would
/// describe the repository rather than the changeset.
fn build_branching_report(result: &AuditResult) -> Option<fallow_output::BranchingReport> {
    let base = result.base_snapshot.as_ref()?;
    let health = result.health.as_ref()?;
    let root = health.config.root.as_path();
    let accounting: rustc_hash::FxHashSet<String> = result
        .changed_files
        .iter()
        .map(|path| crate::audit::keys::relative_key_path(path, root))
        .collect();
    let restrict = |source: &fallow_output::BranchingSnapshot| -> fallow_output::BranchingSnapshot {
        source
            .iter()
            .filter(|(path, _)| accounting.contains(path.as_str()))
            .map(|(path, totals)| (path.clone(), *totals))
            .collect()
    };
    let head = crate::audit::branching_keys(&health.branching_by_file, root);
    Some(fallow_output::BranchingReport::compare(
        &restrict(&base.branching),
        &restrict(&head),
        fallow_output::DEFAULT_BRANCHING_TOLERANCE,
        &fallow_engine::test_paths::is_test_path_str,
    ))
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
pub fn build_brief_json(
    result: &AuditResult,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> Result<serde_json::Value, ExitCode> {
    let brief = build_brief_output_with_diff(result, diff_index);
    let audit_header =
        fallow_api::build_review_brief_header(crate::audit::audit_json_header_input(result));
    let subtract = build_brief_subtract_sections(result)?;
    let mut output = fallow_output::build_review_brief_json_output(brief, audit_header, subtract)
        .map_err(|err| {
        crate::error::emit_error(
            &format!("JSON serialization error: {err}"),
            2,
            fallow_config::OutputFormat::Json,
        )
    })?;
    fallow_api::attach_audit_wire_attribution(&mut output);
    Ok(output)
}

/// Render the brief as JSON. Always returns `SUCCESS`; a serialization failure
/// surfaces the error but the brief contract still exits 0.
fn print_brief_json(
    result: &AuditResult,
    diff_index: Option<&fallow_output::DiffIndex>,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match build_brief_json(result, diff_index) {
        Ok(output) => {
            let Ok(output) = fallow_output::serialize_review_brief_json_output(
                output,
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) else {
                return ExitCode::SUCCESS;
            };
            crate::report::emit_report_json(&output, "audit-brief", json_style)
        }
        Err(_) => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
fn serialize_brief_json(
    value: &serde_json::Value,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    json_style.serialize(value)
}

/// Render the brief in human / compact / markdown form: a short orientation
/// header (scope, risk, effort, boundaries) followed by the same findings
/// sections `fallow audit` prints.
fn print_brief_human(
    result: &AuditResult,
    diff_index: Option<&fallow_output::DiffIndex>,
    quiet: bool,
    explain: bool,
    show_deprioritized: bool,
) {
    let brief = build_brief_output_with_diff(result, diff_index);

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
        print_branching_human(brief.branching.as_ref());
        print_partition_human(&brief.partition);
        print_impact_closure_human(&brief.impact_closure);
        print_focus_human(&brief.focus, show_deprioritized);
        print_deltas_human(&brief.deltas);
        print_weakening_human(&brief.weakening);
        print_routing_human(&brief.routing);
        print_ownership_human(brief.ownership.as_ref());
    }

    // Always render the findings sections so the brief shows WHERE to look, even
    // when the underlying verdict is a fail. Headers stay off (the brief owns its
    // own header line above).
    crate::audit::print_audit_findings(result, quiet, explain, false);
}

/// The brief lines naming the files that split in place, or `None` when none
/// did.
///
/// Split out from the printer so the wording and the width are testable: every
/// line has to hold under 80 columns, and the brief has no other place where a
/// reader meets the phrase "branch point".
fn branching_human_lines(report: Option<&fallow_output::BranchingReport>) -> Option<Vec<String>> {
    let report = report.filter(|report| report.is_reportable())?;
    // States what was measured. Concluding "this was split" would overclaim:
    // the peak is a file-level maximum, so it can also fall because the largest
    // function left the file while other functions arrived.
    let mut lines =
        vec!["  branching: branching about level, more functions, smaller peak".to_string()];
    for split in report.split_in_place.iter().take(2) {
        lines.push(format!("         {}", elide_path(&split.path, 71)));
        lines.push(format!(
            "           {} to {} branch points, {} to {} functions, peak {} to {}",
            split.branch_points_before,
            split.branch_points_after,
            split.functions_before,
            split.functions_after,
            split.peak_before,
            split.peak_after,
        ));
    }
    let remaining = report.split_in_place.len().saturating_sub(2);
    if remaining > 0 {
        lines.push(format!(
            "         and {remaining} more file{}",
            crate::report::plural(remaining)
        ));
    }
    Some(lines)
}

/// Shorten a path from the left, keeping the file name, so a deep path cannot
/// push a brief line past the terminal width.
fn elide_path(path: &str, budget: usize) -> String {
    if path.chars().count() <= budget {
        return path.to_string();
    }
    let tail: String = path
        .chars()
        .rev()
        .take(budget.saturating_sub(4))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!(".../{tail}")
}

/// Print branching conservation on the human brief. Caller has already gated on
/// `!quiet`. Renders nothing unless a file demonstrably split in place: the set
/// totals are context, and every sibling section is silent when it has nothing
/// to say.
fn print_branching_human(report: Option<&fallow_output::BranchingReport>) {
    let Some(lines) = branching_human_lines(report) else {
        return;
    };
    for line in lines {
        eprintln!("{line}");
    }
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
    if partition.independent_slices.len() >= 2 {
        let slices: Vec<String> = partition
            .independent_slices
            .iter()
            .map(|slice| {
                let labeled: Vec<String> = slice.iter().map(|dir| unit_label(dir)).collect();
                format!("[{}]", labeled.join(", "))
            })
            .collect();
        eprintln!(
            "  independent slices: {} (no import edge between them) {}",
            partition.independent_slices.len(),
            slices.join(" ")
        );
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

/// The impact-closure lines: the magnitude, then the single heaviest directory
/// with its exact share and a pointer at the rest.
///
/// Split out from the printer so the wording and the width are testable, the
/// way `branching_human_lines` is: every line has to hold under 80 columns.
/// The heaviest directory carries a count because the names alone cannot say
/// whether the reach is concentrated or diffuse, which is the only question
/// this section exists to answer. One name plus its share beats two names
/// without: it costs half the width, and the count is what makes the line
/// readable at a glance ("88 of 326 in tests" is the answer; two bare names
/// are not).
///
/// The line names the rollup's own top row, so it never disagrees with
/// `affected_by_dir` in the JSON. That row is often a test directory, because
/// tests import broadly; the count is what tells a reader that, which is why
/// it is not omitted.
fn affected_lines(closure: &ImpactClosureFacts) -> Vec<String> {
    if closure.affected_count == 0 {
        return Vec::new();
    }
    let dirs = closure.affected_by_dir.len() + closure.affected_by_dir_omitted;
    let mut lines = vec![format!(
        "  impact closure: {} file{} affected beyond the diff{}",
        closure.affected_count,
        crate::report::plural(closure.affected_count),
        if dirs < 2 {
            String::new()
        } else {
            format!(" across {dirs} directories")
        },
    )];
    // One directory says nothing the count did not already say.
    let Some(heaviest) = closure.affected_by_dir.first().filter(|_| dirs > 1) else {
        return lines;
    };
    lines.push(format!(
        "         heaviest {} ({} file{})",
        elide_path(&unit_label(&heaviest.dir), 48),
        heaviest.count,
        crate::report::plural(heaviest.count),
    ));
    let remaining = dirs - 1;
    // The rollup itself is capped, so the JSON holds the whole breakdown only
    // when nothing was omitted from it. Promising a full list past that point
    // would send a reader on a round-trip that cannot answer them.
    let route = if closure.affected_by_dir_omitted == 0 {
        "--format json for full list".to_string()
    } else {
        format!(
            "{} of them in --format json",
            closure.affected_by_dir.len() - 1
        )
    };
    lines.push(format!(
        "         and {remaining} more director{} ({route})",
        if remaining == 1 { "y" } else { "ies" },
    ));
    lines
}

/// How many coordination gaps the human brief spells out before it collapses
/// the rest into a count. Each one costs two lines, as a branching split does,
/// but a gap is Stage 3's headline signal rather than a supporting metric, so it
/// gets one item more than `branching_human_lines` shows. The JSON carries every
/// gap; this is the reading order, not the record.
const MAX_HUMAN_COORDINATION_GAPS: usize = 3;

/// Join symbol names until they no longer fit `budget`, returning the rendered
/// text and how many names it left out. A gap on a barrel file can consume two
/// dozen symbols, and the reader needs to recognise the contract, not enumerate
/// it.
fn summarize_symbols(symbols: &[String], budget: usize) -> (String, usize) {
    let mut shown = 0usize;
    let mut width = 0usize;
    for symbol in symbols {
        let separator = usize::from(shown > 0) * 2;
        let remaining = symbols.len() - shown - 1;
        // Keep room for the suffix the omitted symbols will need.
        let suffix = if remaining == 0 {
            0
        } else {
            format!(" +{remaining} more").chars().count()
        };
        let next = width + separator + symbol.chars().count();
        if shown > 0 && next + suffix > budget {
            break;
        }
        width = next;
        shown += 1;
    }
    // The first symbol always renders, elided if it alone overruns the budget.
    let shown = shown.max(1).min(symbols.len());
    let joined = symbols[..shown].join(", ");
    let omitted = symbols.len() - shown;
    if omitted == 0 {
        return (elide_symbol(&joined, budget), 0);
    }
    let suffix = format!(" +{omitted} more");
    let head = elide_symbol(&joined, budget.saturating_sub(suffix.chars().count()));
    (format!("{head}{suffix}"), omitted)
}

/// Shorten a symbol list from the right, unlike `elide_path`, which keeps the
/// tail: a symbol's leading characters are what identifies it.
fn elide_symbol(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let head: String = text.chars().take(budget.saturating_sub(3)).collect();
    format!("{head}...")
}

/// The coordination-gap lines: how many consumers sit outside the diff, then a
/// capped walk through the widest of them, one consumer per pair of lines.
///
/// Split out from the printer so the wording and the width are testable, the
/// way `affected_lines` and `branching_human_lines` are: every line has to hold
/// under 80 columns. Two paths and a symbol list cannot share one line at that
/// width, so the consumer gets its own line and the contract it consumes gets
/// the continuation, matching how a branching split renders.
///
/// The walk is ordered by how many symbols the consumer takes, not by path.
/// Unlike `affected_by_dir`, the JSON gap list is path-sorted and carries no
/// ranking of its own, so an alphabetical prefix would spell out whichever
/// consumers sort first and collapse a barrel consumer taking two dozen symbols
/// behind the remainder. The header states the total either way.
///
/// The header claims only what `collect_coordination_gaps` establishes: the
/// consumer uses an export of a file in the diff. It never verifies that the
/// export itself changed, so the line must not say the contract changed.
fn coordination_gap_lines(gaps: &[CoordinationGapFact]) -> Vec<String> {
    if gaps.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "  coordination gap: {} consumer{} outside the diff use{} exports of changed files",
        gaps.len(),
        crate::report::plural(gaps.len()),
        if gaps.len() == 1 { "s" } else { "" },
    )];
    let mut widest: Vec<&CoordinationGapFact> = gaps.iter().collect();
    widest.sort_by(|a, b| {
        b.consumed_symbols
            .len()
            .cmp(&a.consumed_symbols.len())
            .then_with(|| a.consumer_file.cmp(&b.consumer_file))
    });

    let mut symbols_omitted = 0usize;
    for gap in widest.iter().take(MAX_HUMAN_COORDINATION_GAPS) {
        debug_assert!(
            !gap.consumed_symbols.is_empty(),
            "a gap exists because a symbol is consumed, so the list is never empty"
        );
        let (symbols, omitted) = summarize_symbols(&gap.consumed_symbols, 24);
        symbols_omitted += omitted;
        lines.push(format!("         {}", elide_path(&gap.consumer_file, 71)));
        lines.push(format!(
            "           consumes {symbols} from {}",
            elide_path(&gap.changed_file, 28),
        ));
    }

    let remaining = gaps.len().saturating_sub(MAX_HUMAN_COORDINATION_GAPS);
    if remaining > 0 {
        lines.push(format!(
            "         and {remaining} more consumer{} (--format json for full list)",
            crate::report::plural(remaining),
        ));
    } else if symbols_omitted > 0 {
        // A `+N more` with no route on screen leaves the reader nowhere to go.
        lines.push("         (--format json for every consumed symbol)".to_string());
    }
    lines
}

/// Print the Stage 3 impact-closure summary on the human brief: the blast
/// radius and its heaviest directory, then the coordination gaps (the precise
/// inter-module attention pointer). Caller has already gated on `!quiet`.
fn print_impact_closure_human(closure: &ImpactClosureFacts) {
    for line in affected_lines(closure) {
        eprintln!("{line}");
    }
    for line in coordination_gap_lines(&closure.coordination_gap) {
        eprintln!("{line}");
    }
}

/// Greedily wrap prose to `first` columns on the opening line and `rest`
/// thereafter, returning the unprefixed chunks so the caller owns the indents.
///
/// Elision is wrong for these strings: a decision question is the judgment the
/// brief exists to pose, and a focus reason is the evidence for a label, so
/// cutting either destroys the signal rather than shortening it. Only a word
/// that alone overruns its line is shortened, and `elide` decides from which
/// end: a path keeps its tail (`elide_path`), while an owner identity keeps its
/// head (`elide_symbol`), because an email or team name is identified by what it
/// starts with.
fn wrap_prose(
    text: &str,
    first: usize,
    rest: usize,
    elide: fn(&str, usize) -> String,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let budget = if lines.is_empty() { first } else { rest };
        if current.is_empty() {
            current = elide(word, budget);
            continue;
        }
        if current.chars().count() + 1 + word.chars().count() <= budget {
            current.push(' ');
            current.push_str(word);
            continue;
        }
        lines.push(std::mem::take(&mut current));
        current = elide(word, rest);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Render one focus unit: the label and its file, then the reason wrapped
/// underneath, then any confidence flags.
///
/// The reason used to share the file's line, which put an un-elided path and an
/// unbounded sentence on one row; on a real project that reached 103 columns.
fn focus_unit_lines(unit: &crate::audit_focus::FocusUnit) -> Vec<String> {
    let mut lines = vec![format!(
        "    [{}] {}",
        unit.label.token(),
        elide_path(&unit.file, 56),
    )];
    for (n, chunk) in wrap_prose(&unit.reason, 72, 70, elide_path)
        .into_iter()
        .enumerate()
    {
        // Continuations sit past the key column so `confidence` below stays the
        // only thing that starts a new fact.
        lines.push(if n == 0 {
            format!("      {chunk}")
        } else {
            format!("        {chunk}")
        });
    }
    for flag in &unit.confidence {
        lines.extend(
            wrap_prose(flag.message(), 60, 60, elide_path)
                .into_iter()
                .enumerate()
                .map(|(i, chunk)| {
                    if i == 0 {
                        format!("      confidence {chunk}")
                    } else {
                        format!("        {chunk}")
                    }
                }),
        );
    }
    lines
}

/// The Stage 4 weighted focus map lines: the ranked `review-here` units (with
/// reason and any low-confidence flag), then the de-prioritized count as a
/// collapsed escape hatch. `--show-deprioritized` re-expands the full
/// de-prioritized list ("show me what you de-prioritized").
///
/// Split out from the printer so the wording and the width are testable, the
/// way `affected_lines` and `branching_human_lines` are: every line has to hold
/// under 80 columns. Empty when no unit was scored.
fn focus_lines(focus: &crate::audit_focus::FocusMap, show_deprioritized: bool) -> Vec<String> {
    if focus.total_units() == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    if !focus.review_here.is_empty() {
        lines.push(format!(
            "  focus: {} unit{} to review here (of {} changed)",
            focus.review_here.len(),
            crate::report::plural(focus.review_here.len()),
            focus.total_units(),
        ));
        for unit in &focus.review_here {
            lines.extend(focus_unit_lines(unit));
        }
    }
    if focus.deprioritized.is_empty() {
        return lines;
    }
    if show_deprioritized {
        lines.push(format!("  de-prioritized ({}):", focus.deprioritized.len()));
        for unit in &focus.deprioritized {
            lines.extend(focus_unit_lines(unit));
        }
    } else {
        lines.push(format!(
            "  de-prioritized: {} unit{} (run with --show-deprioritized to list)",
            focus.deprioritized.len(),
            crate::report::plural(focus.deprioritized.len()),
        ));
    }
    lines
}

/// Print the Stage 4 weighted focus map on the human brief. Caller has already
/// gated on `!quiet`. Renders nothing when no unit was scored.
fn print_focus_human(focus: &crate::audit_focus::FocusMap, show_deprioritized: bool) {
    for line in focus_lines(focus, show_deprioritized) {
        eprintln!("{line}");
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
    if !deltas.dependency_added.is_empty() {
        eprintln!(
            "  new third-party dependenc{}: {}",
            if deltas.dependency_added.len() == 1 {
                "y"
            } else {
                "ies"
            },
            dependency_key_names(&deltas.dependency_added)
        );
    }
    if !deltas.dependency_major_bumped.is_empty() {
        eprintln!(
            "  major version bump{}: {}",
            crate::report::plural(deltas.dependency_major_bumped.len()),
            dependency_key_names(&deltas.dependency_major_bumped)
        );
    }
}

/// Render dependency delta keys (`<manifest>::<name>[@<from>-><to>]`) as the
/// names a human reads: `react ^18.0.0 to ^19.0.0 (package.json)`.
fn dependency_key_names(keys: &[String]) -> String {
    keys.iter()
        .map(|key| {
            let (manifest, rest) = key.split_once("::").unwrap_or(("", key));
            // A leading @ belongs to the scope; later @ signs may also occur
            // inside npm alias ranges, so only split at the first non-leading one.
            let (name, range) = rest
                .match_indices('@')
                .find(|(index, _)| *index > 0)
                .map_or((rest, None), |(index, _)| {
                    (&rest[..index], rest[index + 1..].split_once("->"))
                });
            let range_text = range
                .map(|(from, to)| format!(" {from} to {to}"))
                .unwrap_or_default();
            let manifest_text = if manifest.is_empty() || manifest == "package.json" {
                String::new()
            } else {
                format!(" ({manifest})")
            };
            format!("{name}{range_text}{manifest_text}")
        })
        .collect::<Vec<_>>()
        .join(", ")
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

/// Width of the owner column in the human ownership rollup.
const OWNER_COLUMN_WIDTH: usize = 24;

/// Width budget for the module directories of a slice on its human line.
const SLICE_DIRS_BUDGET: usize = 27;

/// The ownership lines: the owner-group count, the capped owner rollup, and
/// the slices that have one owner.
///
/// Split out from the printer so the wording and the width are testable: every
/// line has to hold under 80 columns. The slice numbers are 1-based positions
/// in `partition.independent_slices`, the same order the partition line
/// prints. Fallow reports the slices with one owner; the reviewer decides if
/// a split is useful.
fn ownership_lines(ownership: Option<&fallow_output::OwnershipFacts>) -> Vec<String> {
    let Some(ownership) = ownership else {
        return Vec::new();
    };
    if ownership.group_count == 0 {
        return Vec::new();
    }
    let mut notes: Vec<String> = Vec::new();
    if ownership.transitive_only_count > 0 {
        notes.push(format!(
            "{} transitive only",
            ownership.transitive_only_count
        ));
    }
    if ownership.unowned_direct_count > 0 {
        notes.push(format!(
            "{} unowned file{}",
            ownership.unowned_direct_count,
            crate::report::plural(ownership.unowned_direct_count),
        ));
    }
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join(", "))
    };
    let mut lines = vec![format!(
        "  ownership: {} owner group{}{notes}",
        ownership.group_count,
        crate::report::plural(ownership.group_count),
    )];
    for group in &ownership.groups {
        let owner = elide_symbol(&group.owner, OWNER_COLUMN_WIDTH);
        let affected = if group.affected_count > 0 {
            format!(", {} affected", group.affected_count)
        } else {
            String::new()
        };
        lines.push(format!(
            "    {owner:<OWNER_COLUMN_WIDTH$} {} changed{affected}",
            group.direct_count
        ));
    }
    if ownership.groups_omitted > 0 {
        lines.push(format!(
            "    +{} more group{}",
            ownership.groups_omitted,
            crate::report::plural(ownership.groups_omitted),
        ));
    }
    for (index, slice) in ownership.slices.iter().enumerate() {
        if !slice.separable {
            continue;
        }
        lines.push(format!(
            "  slice {} ({}) {}",
            index + 1,
            slice_dirs_label(&slice.module_dirs),
            slice_owner_phrase(&slice.owners),
        ));
    }
    lines
}

/// Label the module directories of a slice: the first directory, shortened
/// by whole leading segments so the distinct tail stays visible, plus the
/// count of the other directories. The partition line lists every directory in full.
fn slice_dirs_label(module_dirs: &[String]) -> String {
    let Some(first) = module_dirs.first() else {
        return String::new();
    };
    let more = module_dirs.len() - 1;
    let suffix = if more > 0 {
        format!(" +{more} more")
    } else {
        String::new()
    };
    let budget = SLICE_DIRS_BUDGET.saturating_sub(suffix.chars().count());
    format!(
        "{}{suffix}",
        elide_leading_segments(&unit_label(first), budget)
    )
}

/// Shorten a path by removing whole leading segments, so the label never
/// cuts inside a folder name. Falls back to a character cut only when the
/// last segment alone is wider than the budget.
fn elide_leading_segments(path: &str, budget: usize) -> String {
    if path.chars().count() <= budget {
        return path.to_string();
    }
    let mut rest = path;
    while let Some((_, tail)) = rest.split_once('/') {
        rest = tail;
        if rest.chars().count() + 4 <= budget {
            return format!(".../{rest}");
        }
    }
    elide_path(rest, budget)
}

/// The owner part of a separable slice line.
fn slice_owner_phrase(owners: &[String]) -> String {
    match owners {
        [owner] if owner == crate::codeowners::UNOWNED_LABEL => "is unowned".to_string(),
        _ => format!(
            "has one owner: {}",
            elide_symbol(&owners.join(", "), OWNER_COLUMN_WIDTH)
        ),
    }
}

/// Print the ownership section. Silent when the brief has no ownership
/// section.
fn print_ownership_human(ownership: Option<&fallow_output::OwnershipFacts>) {
    for line in ownership_lines(ownership) {
        eprintln!("{line}");
    }
}

/// The decision-surface lines (the apex, 6.G): the ranked, capped set of
/// consequential structural decisions, each as a framed judgment question with
/// its routed expert. Leads the brief.
///
/// Split out from the printer so the wording and the width are testable, the
/// way `affected_lines` and `branching_human_lines` are: every line has to hold
/// under 80 columns. A question naming a widened export list runs to several
/// hundred characters, so it wraps under a hanging indent rather than being
/// cut: the question IS the judgment the brief exists to pose, and truncating
/// it would drop the ask at the end of the sentence.
fn decision_surface_lines(surface: &crate::audit_decision_surface::DecisionSurface) -> Vec<String> {
    if surface.decisions.is_empty() {
        return vec![
            "Decisions: none (no consequential structural decision in this change)".to_string(),
            String::new(),
        ];
    }
    let mut lines = vec![format!("Decisions to make ({}):", surface.decisions.len())];
    for (i, decision) in surface.decisions.iter().enumerate() {
        // Taste ownership: the question first (never an answer), then the honest
        // graph fact, then the named trade-off. The human reads reversibility from
        // the count; the tool never labels the door or recommends a choice.
        let head = format!("  {}. [{}] ", i + 1, decision.category.tag());
        let head_width = head.chars().count();
        // Continuations sit past column 5 so that column stays the key column
        // `trade-off:` and `ask:` own, giving the block a 2 / 5 / 7 hierarchy.
        let question = wrap_prose(&decision.question, 80 - head_width, 73, elide_path);
        if question.is_empty() {
            lines.push(head.trim_end().to_string());
        }
        for (n, chunk) in question.into_iter().enumerate() {
            if n == 0 {
                lines.push(format!("{head}{chunk}"));
            } else {
                lines.push(format!("       {chunk}"));
            }
        }
        if !decision.tradeoff.is_empty() {
            for (n, chunk) in wrap_prose(&decision.tradeoff, 64, 71, elide_path)
                .into_iter()
                .enumerate()
            {
                if n == 0 {
                    lines.push(format!("     trade-off: {chunk}"));
                } else {
                    lines.push(format!("       {chunk}"));
                }
            }
        }
        if !decision.expert.is_empty() {
            let bus = if decision.bus_factor_one {
                " (bus-factor 1)"
            } else {
                ""
            };
            // The suffix lands on the LAST wrapped line, so its width has to come
            // out of every line's budget, not just the first.
            let reserved = bus.chars().count();
            // `     ask: ` is 10 columns and the continuation indent is 7, so the
            // budgets are 70 and 73 before the suffix, which lands on whichever
            // line ends up last and therefore comes out of every line.
            let experts = wrap_prose(
                &decision.expert.join(", "),
                70 - reserved,
                73 - reserved,
                elide_symbol,
            );
            for (n, chunk) in experts.iter().enumerate() {
                if n == 0 {
                    lines.push(format!("     ask: {chunk}"));
                } else {
                    lines.push(format!("       {chunk}"));
                }
            }
            if !bus.is_empty()
                && !experts.is_empty()
                && let Some(last) = lines.last_mut()
            {
                last.push_str(bus);
            }
        }
    }
    if let Some(note) = &surface.truncated {
        for (n, chunk) in wrap_prose(&note.reason, 72, 72, elide_path)
            .into_iter()
            .enumerate()
        {
            lines.push(if n == 0 {
                format!("  ... {chunk}")
            } else {
                format!("      {chunk}")
            });
        }
    }
    // The apex section closes with a blank line in BOTH states, or it runs
    // straight into the drill-down header it is supposed to lead.
    lines.push(String::new());
    lines
}

/// Print the decision surface. Caller has already gated on `!quiet`.
fn print_decision_surface_human(surface: &crate::audit_decision_surface::DecisionSurface) {
    for line in decision_surface_lines(surface) {
        eprintln!("{line}");
    }
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
    diff_index: Option<&fallow_output::DiffIndex>,
    quiet: bool,
    explain: bool,
    show_deprioritized: bool,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    use fallow_config::OutputFormat;

    match result.output {
        OutputFormat::Json => print_brief_json(result, diff_index, json_style),
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown => {
            print_brief_human(result, diff_index, quiet, explain, show_deprioritized);
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
pub fn print_decision_surface_result(
    result: &AuditResult,
    quiet: bool,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    use fallow_config::OutputFormat;

    let surface = result.decision_surface.clone().unwrap_or_default();
    match result.output {
        OutputFormat::Json => {
            let output = crate::audit_decision_surface::build_decision_surface_output(&surface);
            match fallow_output::serialize_decision_surface_json_output(
                output,
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => {
                    let _ = crate::report::emit_report_json(&value, "decision-surface", json_style);
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
pub fn print_walkthrough_guide_result(
    result: &AuditResult,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let guide = crate::audit_walkthrough::build_guide_from_result(result);
    if let Ok(value) = fallow_output::serialize_walkthrough_guide_json_output(
        guide,
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        let _ = crate::report::emit_report_json(&value, "review-walkthrough-guide", json_style);
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
/// direction: a missing or garbled agent file never accepts a judgment. The
/// read error itself is reported on stderr naming the failing path, so a typo'd
/// path is not misdiagnosed as a moved tree.
#[must_use]
pub fn print_walkthrough_file_result(
    result: &AuditResult,
    path: &std::path::Path,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let contents = std::fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!(
            "fallow: cannot read walkthrough file '{}': {error}",
            path.display()
        );
        String::new()
    });
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
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    ) {
        let _ =
            crate::report::emit_report_json(&value, "review-walkthrough-validation", json_style);
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
#[expect(
    clippy::too_many_arguments,
    reason = "walkthrough rendering needs its existing view state plus the JSON presentation style"
)]
pub fn print_walkthrough_human_result(
    result: &AuditResult,
    root: &std::path::Path,
    cache_dir: &std::path::Path,
    mark_viewed: &[std::path::PathBuf],
    show_cleared: bool,
    quiet: bool,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    use fallow_config::OutputFormat;

    // JSON reuses the single guide JSON path verbatim (no second serializer).
    if matches!(result.output, OutputFormat::Json) {
        return print_walkthrough_guide_result(result, json_style);
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

    #[test]
    fn dependency_names_preserve_scopes_and_manifest_labels() {
        let keys = [
            "package.json::@scope/one",
            "package.json::@scope/two",
            "package.json::other",
            "package.json::plain",
            "packages/app/package.json::@scope/workspace",
            "@scope/unqualified",
        ]
        .map(str::to_string);
        assert_eq!(
            dependency_key_names(&keys),
            "@scope/one, @scope/two, other, plain, @scope/workspace (packages/app/package.json), @scope/unqualified"
        );
    }

    #[test]
    fn dependency_names_preserve_scoped_bumps_and_alias_ranges() {
        let keys = [
            "package.json::@scope/bumped@^1.0.0->^2.0.0",
            "packages/app/package.json::plain@^1.0.0->^2.0.0",
            "package.json::@scope/alias@npm:@other/pkg@^1.0.0->npm:@other/pkg@^2.0.0",
        ]
        .map(str::to_string);
        assert_eq!(
            dependency_key_names(&keys),
            "@scope/bumped ^1.0.0 to ^2.0.0, plain ^1.0.0 to ^2.0.0 (packages/app/package.json), @scope/alias npm:@other/pkg@^1.0.0 to npm:@other/pkg@^2.0.0"
        );
    }

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
            dupe_demotion_diff_source: None,
            base_snapshot: None,
            comparison: None,
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
            ownership: None,
            decision_surface: None,
            graph_snapshot_hash: None,
            change_anchors: Vec::new(),
            diff_index: None,
        }
    }

    #[test]
    fn brief_mode_always_returns_success_even_when_verdict_is_fail() {
        // Human path.
        let human = audit_result(AuditVerdict::Fail, OutputFormat::Human);
        assert_eq!(
            print_brief_result(
                &human,
                None,
                true,
                false,
                false,
                crate::json_style::JsonStyle::Compact,
            ),
            ExitCode::SUCCESS
        );

        // JSON path.
        let json = audit_result(AuditVerdict::Fail, OutputFormat::Json);
        assert_eq!(
            print_brief_result(
                &json,
                None,
                true,
                false,
                false,
                crate::json_style::JsonStyle::Compact,
            ),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn brief_json_validates_against_audit_brief_schema_variant() {
        let result = audit_result(AuditVerdict::Fail, OutputFormat::Json);
        let value = fallow_output::serialize_review_brief_json_output(
            build_brief_json(&result, None).expect("brief json must build"),
            crate::output_runtime::telemetry_analysis_run_id().as_deref(),
        )
        .expect("brief json must serialize");

        assert_eq!(value["kind"], "audit-brief");
        assert_eq!(value["command"], "audit-brief");
        assert_eq!(value["schema_version"], REVIEW_BRIEF_SCHEMA_VERSION);
        assert_eq!(value["attribution"]["styling_introduced"], 0);
        assert_eq!(value["attribution"]["styling_inherited"], 0);
        assert_eq!(value["attribution"]["duplication_demoted"], 0);
    }

    #[test]
    fn brief_json_is_byte_identical_on_repeated_serialization() {
        // `elapsed: Duration::ZERO` and no telemetry: the brief JSON carries no
        // timestamps or randomness, so two builds serialize byte-identically.
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let first = build_brief_json(&result, None).expect("first build");
        let second = build_brief_json(&result, None).expect("second build");
        let first_str = serde_json::to_string_pretty(&first).expect("serialize first");
        let second_str = serde_json::to_string_pretty(&second).expect("serialize second");
        assert_eq!(first_str, second_str);
    }

    #[test]
    fn brief_json_serialization_honors_selected_style() {
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let value = build_brief_json(&result, None).expect("brief json must build");

        let compact = serialize_brief_json(&value, crate::json_style::JsonStyle::Compact)
            .expect("compact brief JSON must serialize");
        let pretty = serialize_brief_json(&value, crate::json_style::JsonStyle::Pretty)
            .expect("pretty brief JSON must serialize");

        assert_eq!(compact.lines().count(), 1);
        assert!(pretty.lines().count() > 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&compact).expect("compact JSON must parse"),
            serde_json::from_str::<serde_json::Value>(&pretty).expect("pretty JSON must parse")
        );
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
    fn triage_uses_supplied_diff_metrics_and_existing_risk_thresholds() {
        let mut result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        result.changed_files_count = 1;
        let mut diff = String::from(
            "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -0,0 +1,100 @@\n",
        );
        for _ in 0..RISK_MEDIUM_LINES {
            diff.push_str("+added\n");
        }
        let index = fallow_output::DiffIndex::from_unified_diff(&diff);
        result.diff_index = Some(index);

        let triage = build_brief_output(&result).triage;

        assert_eq!(triage.hunks, Some(1));
        assert_eq!(triage.net_lines, Some(RISK_MEDIUM_LINES));
        assert_eq!(triage.risk_class, RiskClass::Medium);
        assert_eq!(triage.review_effort, ReviewEffort::Review);
    }

    #[test]
    fn no_diff_brief_keeps_optional_triage_fields_absent() {
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let value = serde_json::to_value(build_brief_output_with_diff(&result, None))
            .expect("brief serializes");

        assert!(value["triage"].get("hunks").is_none());
        assert!(value["triage"].get("net_lines").is_none());
    }

    #[test]
    fn brief_json_includes_empty_impact_closure_when_no_graph_retained() {
        // check: None -> no closure; the impact_closure object must still be
        // present and empty so consumers can rely on its presence.
        let result = audit_result(AuditVerdict::Warn, OutputFormat::Json);
        let value = build_brief_json(&result, None).expect("brief json must build");
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
    fn graph_facts_carry_no_file_list() {
        // The blast radius is Stage 3's alone. Stage 1 used to clone it verbatim,
        // which put the same list on the wire twice and made half the envelope a
        // duplicate.
        let facts = derive_graph_facts(&AnalysisResults::default());
        let value = serde_json::to_value(&facts).expect("graph facts serialize");
        let keys: Vec<&str> = value
            .as_object()
            .expect("graph facts are an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["exports_added", "api_width_delta", "boundaries_touched"]
        );
    }

    fn spread_closure(affected: &[&str]) -> ImpactClosureFacts {
        let mut paths: Vec<String> = affected.iter().map(|p| (*p).to_string()).collect();
        paths.sort();
        ImpactClosureFacts::new(&paths, Vec::new())
    }

    #[test]
    fn human_impact_lines_report_the_full_count_not_the_sample() {
        // Distinct file and directory totals, so a line that printed one where
        // the other belongs cannot pass.
        let mut affected: Vec<String> = (0..40).map(|i| format!("src/zone{i:03}/a.ts")).collect();
        affected.extend((0..40).map(|i| format!("src/zone{i:03}/b.ts")));
        affected.sort();
        let closure = ImpactClosureFacts::new(&affected, Vec::new());
        assert!(
            closure.affected_not_shown.len() < closure.affected_count,
            "the fixture must exercise the sample cap"
        );
        assert!(
            closure.affected_by_dir.len() < 40,
            "the fixture must exercise the rollup cap too"
        );
        let lines = affected_lines(&closure);
        assert!(
            lines[0].contains("80 files affected") && lines[0].contains("across 40 directories"),
            "both totals survive capping, and neither stands in for the other: {lines:?}"
        );
        assert!(
            lines[2].contains("and 39 more directories"),
            "the remainder counts every directory, not just the kept rollup rows: {lines:?}"
        );
    }

    #[test]
    fn the_json_route_is_only_promised_when_the_json_holds_the_breakdown() {
        let complete: Vec<String> = (0..3).map(|i| format!("src/z{i}/file.ts")).collect();
        let lines = affected_lines(&ImpactClosureFacts::new(&complete, Vec::new()));
        assert!(
            lines[2].ends_with("(--format json for full list)"),
            "an uncapped rollup really is the full list: {lines:?}"
        );

        let mut capped: Vec<String> = (0..40).map(|i| format!("src/z{i:03}/file.ts")).collect();
        capped.sort();
        let closure = ImpactClosureFacts::new(&capped, Vec::new());
        assert!(closure.affected_by_dir_omitted > 0, "fixture must cap");
        let lines = affected_lines(&closure);
        assert!(
            lines[2].ends_with("(24 of them in --format json)"),
            "past the rollup cap the JSON has no full list either, so do not promise one: {lines:?}"
        );
    }

    #[test]
    fn the_repository_root_is_never_a_blank_token() {
        let closure = spread_closure(&["setup.ts", "playground.ts", "src/app.ts"]);
        let lines = affected_lines(&closure);
        assert!(
            lines[1].contains("heaviest <root> (2 files)"),
            "a root-level directory must render as `<root>`, not as nothing: {lines:?}"
        );
        assert!(
            lines[2].contains("and 1 more directory ("),
            "a single remaining directory is not plural: {lines:?}"
        );
    }

    #[test]
    fn impact_closure_lines_fit_eighty_columns() {
        let deep = "packages/platform/features/checkout/pricing/discounts/rules/seasonal";
        let mut affected: Vec<String> = (0..40).map(|i| format!("{deep}/rule{i:03}.ts")).collect();
        affected.extend((0..999).map(|i| format!("src/z{i:04}/file.ts")));
        affected.sort();
        for line in affected_lines(&ImpactClosureFacts::new(&affected, Vec::new())) {
            assert!(
                line.chars().count() <= 80,
                "brief lines hold under 80 columns: {} chars in {line:?}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn a_single_directory_reach_says_only_the_count() {
        let lines = affected_lines(&spread_closure(&["src/a.ts", "src/b.ts"]));
        assert_eq!(
            lines,
            vec!["  impact closure: 2 files affected beyond the diff".to_string()],
            "naming the one directory would repeat what the count said"
        );
    }

    fn gap(consumer: &str, changed: &str, symbols: &[&str]) -> CoordinationGapFact {
        CoordinationGapFact {
            changed_file: changed.to_string(),
            consumer_file: consumer.to_string(),
            consumed_symbols: symbols.iter().map(|s| (*s).to_string()).collect(),
            note: COORDINATION_GAP_NOTE.to_string(),
        }
    }

    #[test]
    fn coordination_gap_lines_fit_eighty_columns() {
        // The widest line in the section is the consumer path, so the fixture
        // has to overrun its budget or the assertion proves nothing.
        let symbols: Vec<String> = (0..26)
            .map(|i| format!("safeParseAsyncVariant{i:02}"))
            .collect();
        let symbol_refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
        let gaps: Vec<CoordinationGapFact> = (0..1234)
            .map(|i| {
                gap(
                    &format!(
                        "packages/platform/features/checkout/pricing/discounts/seasonal/regional/tiers/consumer{i:04}.ts"
                    ),
                    "packages/platform/features/checkout/pricing/discounts/parse.ts",
                    &symbol_refs,
                )
            })
            .collect();
        let lines = coordination_gap_lines(&gaps);
        assert!(
            lines
                .iter()
                .any(|line| line.contains(".../") && line.chars().count() == 80),
            "the fixture must drive the consumer line to the 80-column ceiling: {lines:?}"
        );
        assert!(
            lines[0].contains("1234 consumers") && lines.last().is_some_and(|l| l.contains("1231")),
            "four-digit counts must render on both the header and the remainder: {lines:?}"
        );
        for line in lines {
            assert!(
                line.chars().count() <= 80,
                "brief lines hold under 80 columns: {} chars in {line:?}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn the_widest_consumers_are_the_ones_spelled_out() {
        // The JSON gap list is path-sorted with no ranking, so an alphabetical
        // prefix would collapse the barrel consumer behind the remainder.
        let mut gaps: Vec<CoordinationGapFact> = (0..5)
            .map(|i| gap(&format!("src/a{i}.ts"), "src/core.ts", &["parse"]))
            .collect();
        gaps.push(gap(
            "src/zz-barrel.ts",
            "src/core.ts",
            &["parse", "decode", "encode"],
        ));
        let lines = coordination_gap_lines(&gaps);
        assert!(
            lines[1].contains("src/zz-barrel.ts"),
            "the consumer taking the most symbols leads: {lines:?}"
        );
    }

    #[test]
    fn coordination_gaps_beyond_the_cap_are_counted_not_dropped_silently() {
        let gaps: Vec<CoordinationGapFact> = (0..7)
            .map(|i| gap(&format!("src/c{i}.ts"), "src/core.ts", &["parse"]))
            .collect();
        let lines = coordination_gap_lines(&gaps);
        assert!(
            lines[0].starts_with(
                "  coordination gap: 7 consumers outside the diff use exports of changed files"
            ),
            "{lines:?}"
        );
        assert_eq!(
            lines.len(),
            1 + MAX_HUMAN_COORDINATION_GAPS * 2 + 1,
            "a header, two lines per shown gap, then the remainder: {lines:?}"
        );
        assert!(
            lines
                .last()
                .expect("a remainder line")
                .contains("and 4 more consumers (--format json for full list)"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_single_coordination_gap_reads_as_singular_and_needs_no_remainder() {
        let lines = coordination_gap_lines(&[gap("src/app.ts", "src/core.ts", &["parse"])]);
        assert_eq!(
            lines,
            vec![
                "  coordination gap: 1 consumer outside the diff uses exports of changed files"
                    .to_string(),
                "         src/app.ts".to_string(),
                "           consumes parse from src/core.ts".to_string(),
            ]
        );
    }

    #[test]
    fn an_over_budget_first_symbol_does_not_swallow_the_more_suffix() {
        let long = "aVeryLongExportedSymbolNameIndeed";
        let (text, omitted) = summarize_symbols(&[long.to_string(), "parse".to_string()], 24);
        assert_eq!(omitted, 1);
        assert!(text.ends_with(" +1 more"), "{text:?}");
        assert!(
            text.chars().count() <= 24,
            "{} chars in {text:?}",
            text.chars().count()
        );
    }

    #[test]
    fn an_elided_symbol_list_still_gets_a_route_without_a_remainder_line() {
        let symbols: Vec<String> = (0..30).map(|i| format!("symbolNumber{i:02}")).collect();
        let symbol_refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
        let lines = coordination_gap_lines(&[gap("src/app.ts", "src/core.ts", &symbol_refs)]);
        assert!(
            lines[2].contains('+'),
            "the fixture must elide symbols: {lines:?}"
        );
        assert_eq!(
            lines.last().map(String::as_str),
            Some("         (--format json for every consumed symbol)"),
            "a `+N more` with no remainder line still needs somewhere to go: {lines:?}"
        );
    }

    fn focus_unit(
        file: &str,
        reason: &str,
        label: crate::audit_focus::FocusLabel,
    ) -> crate::audit_focus::FocusUnit {
        use crate::audit_focus::{ConfidenceFlag, FocusScore, FocusUnit};
        FocusUnit {
            file: file.to_string(),
            score: FocusScore {
                fan_io: 1,
                security_taint: 0,
                risk_zone: 0,
                change_shape: 0,
                runtime: 0,
                total: 1,
            },
            label,
            reason: reason.to_string(),
            confidence: vec![ConfidenceFlag::ReExportIndirection],
        }
    }

    /// The widest real inputs: a deep monorepo path and an unbounded reason.
    fn wide_focus() -> crate::audit_focus::FocusMap {
        let deep = "packages/platform/features/checkout/pricing/discounts/seasonal/regional/tiers/rules.ts";
        let reason = "high fan-in (312 importers), fan-out 47, changes a contract consumed \
             outside the diff, sits in a risk zone, and carries a security-tainted \
             argument reachable from an untrusted source";
        use crate::audit_focus::FocusLabel;
        crate::audit_focus::FocusMap {
            review_here: vec![focus_unit(deep, reason, FocusLabel::ReviewHere)],
            // `[not-prioritized]` is the widest label, so it renders the widest
            // row; labelling this `ReviewHere` would leave that row untested.
            deprioritized: vec![focus_unit(deep, reason, FocusLabel::NotPrioritized)],
        }
    }

    #[test]
    fn focus_lines_fit_eighty_columns() {
        for show_deprioritized in [false, true] {
            let lines = focus_lines(&wide_focus(), show_deprioritized);
            assert!(
                lines.iter().any(|line| line.contains(".../")),
                "the fixture must exercise path elision: {lines:?}"
            );
            assert!(
                lines
                    .iter()
                    .filter(|l| l.starts_with("        ") && !l.contains("confidence"))
                    .count()
                    >= 1,
                "the fixture must wrap a reason onto a continuation line, and the \
                 `confidence` row must not stand in for one: {lines:?}"
            );
            if show_deprioritized {
                // `[not-prioritized]` is four columns wider than `[review-here]`,
                // so only the expanded branch renders the widest row this section
                // can produce. Pinning it means raising the path budget fails here.
                let widest = lines
                    .iter()
                    .find(|l| l.starts_with("    [not-prioritized] "))
                    .expect("the expanded branch renders the de-prioritized unit");
                assert_eq!(
                    widest.chars().count(),
                    78,
                    "the widest focus row sits at its budget: {widest:?}"
                );
            }
            for line in lines {
                assert!(
                    line.chars().count() <= 80,
                    "brief lines hold under 80 columns: {} chars in {line:?}",
                    line.chars().count()
                );
            }
        }
    }

    #[test]
    fn a_wrapped_focus_reason_keeps_every_word() {
        let reason =
            "high fan-in (2 importers), fan-out 4, changes a contract consumed outside the diff";
        let lines = focus_lines(
            &crate::audit_focus::FocusMap {
                review_here: vec![focus_unit(
                    "src/a.ts",
                    reason,
                    crate::audit_focus::FocusLabel::ReviewHere,
                )],
                deprioritized: Vec::new(),
            },
            false,
        );
        let rejoined: String = lines
            .iter()
            .filter(|l| l.starts_with("      ") && !l.contains("confidence"))
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            rejoined, reason,
            "wrapping re-flows the reason, it never drops a word"
        );
    }

    fn decision(
        question: &str,
        tradeoff: &str,
        experts: &[&str],
    ) -> crate::audit_decision_surface::Decision {
        use crate::audit_decision_surface::{Decision, DecisionCategory};
        Decision {
            signal_id: "sig".to_string(),
            category: DecisionCategory::PublicApiContract,
            question: question.to_string(),
            anchor_file: "src/core.ts".to_string(),
            anchor_line: 1,
            signal_key: "key".to_string(),
            previous_signal_id: None,
            blast: 1,
            consequence: 1,
            expert: experts.iter().map(|e| (*e).to_string()).collect(),
            bus_factor_one: true,
            internal_consumer_count: 1,
            tradeoff: tradeoff.to_string(),
        }
    }

    #[test]
    fn decision_surface_lines_fit_eighty_columns() {
        // The real shape that overran: a question naming every widened export.
        let exports = (0..26)
            .map(|i| format!("safeParseAsyncVariant{i:02}"))
            .collect::<Vec<_>>()
            .join(", ");
        let question = format!(
            "`packages/platform/features/checkout/pricing/parse.ts` changes exports \
             ({exports}) imported by 312 files outside this PR. Does this change break \
             or alter what those callers expect?"
        );
        let surface = crate::audit_decision_surface::DecisionSurface {
            decisions: vec![decision(
                &question,
                "312 modules outside the diff consume this contract; changing its shape \
                 requires coordinating them.",
                &[
                    "a-very-long-github-handle",
                    "another-long-handle",
                    "third-handle",
                ],
            )],
            truncated: Some(crate::audit_decision_surface::TruncationNote {
                collapsed: 9,
                reason: "9 more structural decisions collapsed below the cap of 4".to_string(),
            }),
            emitted_signal_ids: vec!["sig".to_string()],
        };
        let lines = decision_surface_lines(&surface);
        assert!(
            lines.iter().filter(|l| l.starts_with("     ")).count() > 3,
            "the fixture must exercise wrapping: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.chars().count() == 80),
            "the fixture must drive a line to the ceiling: {lines:?}"
        );
        let ask = lines
            .iter()
            .find(|l| l.starts_with("     ask: "))
            .expect("an ask line");
        assert!(
            ask.ends_with("(bus-factor 1)") || lines.iter().any(|l| l.ends_with("(bus-factor 1)")),
            "the bus-factor suffix still lands: {lines:?}"
        );
        assert!(
            !ask.contains("..."),
            "an owner identity that fits must not be shortened: {ask:?}"
        );
        for line in lines {
            assert!(
                line.chars().count() <= 80,
                "brief lines hold under 80 columns: {} chars in {line:?}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn a_decision_with_no_experts_omits_the_ask_line() {
        let lines = decision_surface_lines(&crate::audit_decision_surface::DecisionSurface {
            decisions: vec![decision("Widens the public surface. Intended?", "", &[])],
            truncated: None,
            emitted_signal_ids: vec!["sig".to_string()],
        });
        assert!(!lines.iter().any(|l| l.contains("ask:")), "{lines:?}");
        assert!(!lines.iter().any(|l| l.contains("trade-off:")), "{lines:?}");
    }

    #[test]
    fn an_empty_decision_surface_still_says_so() {
        let lines =
            decision_surface_lines(&crate::audit_decision_surface::DecisionSurface::default());
        assert_eq!(
            lines.first().map(String::as_str),
            Some("Decisions: none (no consequential structural decision in this change)")
        );
    }

    #[test]
    fn an_unscored_focus_map_renders_nothing() {
        assert!(focus_lines(&crate::audit_focus::FocusMap::default(), false).is_empty());
        assert!(focus_lines(&crate::audit_focus::FocusMap::default(), true).is_empty());
    }

    #[test]
    fn a_word_wider_than_its_line_is_shortened_not_overflowed() {
        let deep =
            "packages/platform/features/checkout/pricing/discounts/seasonal/regional/rules.ts";
        let lines = wrap_prose(&format!("touches {deep} directly"), 40, 40, elide_path);
        for line in &lines {
            assert!(line.chars().count() <= 40, "{line:?}");
        }
        assert!(
            lines.iter().any(|l| l.contains(".../")),
            "a path keeps its tail: {lines:?}"
        );
    }

    #[test]
    fn an_owner_identity_is_shortened_from_the_head_not_the_tail() {
        // The routing line exists to name who to ask, and an email or team name
        // is identified by what it starts with, not by its domain.
        let owner = "very.long.firstname.lastname@engineering.example.com";
        let lines = wrap_prose(owner, 30, 30, elide_symbol);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("very.long.first") && lines[0].ends_with("..."),
            "{lines:?}"
        );
    }

    fn ownership_fixture(
        groups: usize,
        slices: Vec<fallow_output::OwnershipSliceFact>,
    ) -> fallow_output::OwnershipFacts {
        let kept = groups.min(fallow_output::OWNER_GROUP_CAP);
        fallow_output::OwnershipFacts {
            group_count: groups,
            transitive_only_count: 12,
            unowned_direct_count: 123,
            groups: (0..kept)
                .map(|i| fallow_output::OwnerGroupFact {
                    owner: format!("@a-very-long-organization-name/team-{i:02}"),
                    direct_count: 100 + i,
                    affected_count: 10_000 + i,
                })
                .collect(),
            groups_omitted: groups - kept,
            slices,
        }
    }

    fn slice(dirs: &[&str], owners: &[&str]) -> fallow_output::OwnershipSliceFact {
        fallow_output::OwnershipSliceFact {
            module_dirs: dirs.iter().map(|d| (*d).to_string()).collect(),
            owners: owners.iter().map(|o| (*o).to_string()).collect(),
            separable: owners.len() == 1,
        }
    }

    #[test]
    fn ownership_lines_fit_eighty_columns() {
        let slices = (0..12)
            .map(|i| {
                slice(
                    &[
                        "packages/some-deeply-nested/module-directory",
                        "packages/other",
                    ],
                    &[if i % 2 == 0 {
                        "@a-very-long-organization-name/design-system-team"
                    } else {
                        "@x"
                    }],
                )
            })
            .collect();
        let lines = ownership_lines(Some(&ownership_fixture(40, slices)));
        assert!(lines.len() > 10, "{lines:?}");
        for line in &lines {
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line}",
                line.chars().count()
            );
        }
    }

    #[test]
    fn ownership_lines_count_the_groups_beyond_the_cap() {
        let lines = ownership_lines(Some(&ownership_fixture(
            fallow_output::OWNER_GROUP_CAP + 3,
            Vec::new(),
        )));
        assert_eq!(lines.last().map(String::as_str), Some("    +3 more groups"));
        assert_eq!(lines.len(), 1 + fallow_output::OWNER_GROUP_CAP + 1);
    }

    #[test]
    fn ownership_lines_name_only_the_slices_with_one_owner() {
        let lines = ownership_lines(Some(&ownership_fixture(
            2,
            vec![
                slice(&["src/app", "src/core"], &["@team/app", "@team/core"]),
                slice(&["src/tools"], &["@team/tools"]),
            ],
        )));
        assert!(!lines.iter().any(|l| l.contains("slice 1 ")), "{lines:?}");
        assert!(
            lines.contains(&"  slice 2 (src/tools) has one owner: @team/tools".to_string()),
            "{lines:?}"
        );
    }

    #[test]
    fn ownership_slice_lines_keep_the_distinct_tail_of_long_directories() {
        let lines = ownership_lines(Some(&ownership_fixture(
            2,
            vec![
                slice(
                    &["packages/hoppscotch-selfhost-web/src/api/queries"],
                    &["@a"],
                ),
                slice(
                    &[
                        "packages/hoppscotch-selfhost-web/src/platform/infra",
                        "packages/other",
                    ],
                    &[crate::codeowners::UNOWNED_LABEL],
                ),
            ],
        )));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("  slice 1 (") && l.contains("api/queries)")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("platform/infra +1 more) is unowned")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_slice_label_removes_whole_leading_segments() {
        assert_eq!(
            elide_leading_segments("packages/hoppscotch-backend/src/infra-config", 27),
            ".../src/infra-config"
        );
        assert_eq!(elide_leading_segments("src/short", 27), "src/short");
        let label = elide_leading_segments("a/an-extremely-long-single-folder-name-here", 20);
        assert_eq!(label.chars().count(), 20, "{label}");
    }

    #[test]
    fn ownership_lines_are_silent_without_codeowners_or_slices() {
        assert!(ownership_lines(None).is_empty());
        let lines = ownership_lines(Some(&ownership_fixture(1, Vec::new())));
        assert!(!lines.iter().any(|l| l.contains("slice")), "{lines:?}");
    }

    #[test]
    fn no_gaps_prints_nothing() {
        assert!(coordination_gap_lines(&[]).is_empty());
    }

    #[test]
    fn a_symbol_list_that_fits_is_printed_whole() {
        assert_eq!(
            summarize_symbols(&["a".into(), "b".into()], 24),
            ("a, b".to_string(), 0)
        );
    }

    #[test]
    fn a_single_symbol_wider_than_the_budget_is_elided_not_dropped() {
        let (one, omitted) = summarize_symbols(&["aRidiculouslyLongExportedSymbolName".into()], 24);
        assert_eq!(omitted, 0);
        assert_eq!(one.chars().count(), 24, "{one:?}");
        assert!(
            one.starts_with("aRidiculously") && one.ends_with("..."),
            "{one:?}"
        );
    }

    #[test]
    fn an_empty_closure_prints_nothing() {
        assert!(affected_lines(&ImpactClosureFacts::new(&[], Vec::new())).is_empty());
        assert!(affected_lines(&ImpactClosureFacts::default()).is_empty());
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

    fn branching_fixture(
        previous: (u32, u32, u16),
        current: (u32, u32, u16),
    ) -> fallow_output::BranchingReport {
        let unit = |(branch_points, functions, peak): (u32, u32, u16)| {
            std::iter::once((
                "src/checkout/pricing.ts".to_string(),
                fallow_types::extract::FileBranching {
                    branch_points,
                    functions,
                    peak_cyclomatic: peak,
                    cognitive: branch_points,
                    cognitive_nesting_weight: 0,
                    has_module_unit: false,
                    has_synthetic_units: false,
                },
            ))
            .collect::<fallow_output::BranchingSnapshot>()
        };
        fallow_output::BranchingReport::compare(
            &unit(previous),
            &unit(current),
            fallow_output::DEFAULT_BRANCHING_TOLERANCE,
            &|_| false,
        )
    }

    #[test]
    fn branching_lines_are_silent_without_a_split() {
        assert!(branching_human_lines(None).is_none());
        let flat = branching_fixture((12, 3, 5), (12, 3, 5));
        assert!(
            branching_human_lines(Some(&flat)).is_none(),
            "set totals alone are context, matching every sibling section"
        );
    }

    #[test]
    fn branching_lines_name_the_file_that_split() {
        let report = branching_fixture((39, 1, 40), (39, 8, 6));
        let lines = branching_human_lines(Some(&report)).expect("a split renders");

        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("branching about level, more functions, smaller peak"));
        assert!(
            lines[1].ends_with("src/checkout/pricing.ts"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].contains("39 to 39 branch points, 1 to 8 functions, peak 40 to 6"),
            "{}",
            lines[2]
        );
    }

    #[test]
    fn branching_lines_fit_eighty_columns() {
        // A long path with four-digit counts, which is the widest realistic
        // shape.
        let long = "src/features/checkout/pricing/discounts/calculate-line-totals.ts";
        let unit = |branch_points: u32, functions: u32, peak: u16| {
            std::iter::once((
                long.to_string(),
                fallow_types::extract::FileBranching {
                    branch_points,
                    functions,
                    peak_cyclomatic: peak,
                    cognitive: branch_points,
                    cognitive_nesting_weight: 0,
                    has_module_unit: false,
                    has_synthetic_units: false,
                },
            ))
            .collect::<fallow_output::BranchingSnapshot>()
        };
        let report = fallow_output::BranchingReport::compare(
            &unit(9999, 1000, 9999),
            &unit(9999, 4000, 12),
            fallow_output::DEFAULT_BRANCHING_TOLERANCE,
            &|_| false,
        );

        for line in branching_human_lines(Some(&report)).expect("a split renders") {
            assert!(
                line.chars().count() <= 80,
                "{} columns: {line}",
                line.chars().count()
            );
        }
    }
}
