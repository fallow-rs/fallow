//! Angular component complexity rollup findings.

use std::path::{Path, PathBuf};

use fallow_output::{
    ComplexityViolation, ComponentRollup, DEFAULT_COGNITIVE_CRITICAL, DEFAULT_COGNITIVE_HIGH,
    DEFAULT_CYCLOMATIC_CRITICAL, DEFAULT_CYCLOMATIC_HIGH, ExceededThreshold,
    compute_finding_severity,
};

use super::threshold_overrides::{
    AppliedHealthThresholds, ComplexityFunctionContext, ThresholdOverrideResolver,
    ThresholdOverrideStateTracker,
};

/// Name of the synthetic rollup finding, the function name the
/// `thresholdOverrides` resolver is queried with for it, and the name the
/// override row is recorded under. An entry that scopes itself with `functions`
/// therefore has to name `<component>` to reach the rollup, matching how
/// `<template>` entries are already addressed.
///
/// All three uses must stay the same string: `annotate_outstanding_dimensions`
/// joins an override row to its finding on `(path, line, col, name)`, so a
/// rollup recorded under a different name would silently never receive its
/// `outstanding` dimensions.
const COMPONENT_ROLLUP_NAME: &str = "<component>";

/// Synthesise per-Angular-component rollup findings.
///
/// For each Angular component that has both at least one class-function
/// finding above threshold and a synthetic `<template>` finding, emit a new
/// `<component>` `ComplexityViolation` whose `cyclomatic` / `cognitive` totals
/// are `max(class) + template`. The rollup is anchored at the worst class
/// function's `(path, line, col)` so an existing
/// `// fallow-ignore-next-line complexity` placed above that function, or the
/// `@Component` decorator on inline-template components, continues to hide both
/// the per-function finding and the rollup. Per-function and per-`<template>`
/// findings are not removed, the rollup is strictly additive.
///
/// The rollup is measured against the owner file's resolved
/// `health.thresholdOverrides` ceilings, not the run globals, and publishes the
/// ceilings it was measured with like every other finding. It also records an
/// override row through `state_tracker`, so an entry that reaches the rollup is
/// disclosed as `active` / `stale` / `insufficient` rather than being reported
/// `no_match` while it silently changes the finding set (issue #2163).
pub(super) fn append_component_rollup_findings(
    findings: &mut Vec<ComplexityViolation>,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<PathBuf, PathBuf>>,
    resolver: &ThresholdOverrideResolver,
    config_root: &Path,
    state_tracker: &mut ThresholdOverrideStateTracker,
) {
    let mut by_owner: rustc_hash::FxHashMap<PathBuf, (Vec<usize>, Vec<usize>)> =
        rustc_hash::FxHashMap::default();
    for (idx, finding) in findings.iter().enumerate() {
        if finding.name == "<template>" {
            if let Some(owner) = component_template_owner(finding, template_owner_lookup) {
                by_owner.entry(owner).or_default().1.push(idx);
            }
        } else if is_component_class_finding(finding) {
            by_owner
                .entry(finding.path.clone())
                .or_default()
                .0
                .push(idx);
        }
    }

    let mut to_push: Vec<ComplexityViolation> = Vec::new();
    for (owner, (class_idxs, template_idxs)) in by_owner {
        if class_idxs.is_empty() || template_idxs.is_empty() || template_idxs.len() > 1 {
            continue;
        }
        let template = &findings[template_idxs[0]];
        let Some(worst_idx) = class_idxs
            .iter()
            .copied()
            .max_by_key(|&index| findings[index].cyclomatic)
        else {
            continue;
        };
        let worst = &findings[worst_idx];
        if let Some(rollup) =
            build_component_rollup(owner, worst, template, resolver, config_root, state_tracker)
        {
            to_push.push(rollup);
        }
    }
    findings.extend(to_push);
}

fn component_template_owner(
    finding: &ComplexityViolation,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<PathBuf, PathBuf>>,
) -> Option<PathBuf> {
    let ext = finding
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("html") => template_owner_lookup
            .and_then(|lookup| lookup.get(&finding.path))
            .cloned(),
        Some("ts" | "tsx" | "mts" | "cts") => Some(finding.path.clone()),
        _ => None,
    }
}

fn is_component_class_finding(finding: &ComplexityViolation) -> bool {
    finding.name != COMPONENT_ROLLUP_NAME
        && finding
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "ts" | "tsx" | "mts" | "cts"
                )
            })
}

/// The rolled-up cyclomatic / cognitive totals for a component (worst frame plus
/// its template) and whether each total exceeds its threshold.
struct ComponentRollupTotals {
    rollup_cyc: u16,
    rollup_cog: u16,
    rollup_lines: u32,
    exceeds_cyclomatic: bool,
    exceeds_cognitive: bool,
}

/// Assemble the synthetic `<component>` rollup finding from the precomputed
/// totals, the worst class frame, and its template frame.
fn make_component_rollup_violation(
    owner: PathBuf,
    worst: &ComplexityViolation,
    template: &ComplexityViolation,
    totals: &ComponentRollupTotals,
    applied: AppliedHealthThresholds,
) -> ComplexityViolation {
    let component = owner.file_stem().map_or_else(
        || "<unknown-component>".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    ComplexityViolation {
        path: owner,
        name: COMPONENT_ROLLUP_NAME.to_string(),
        line: worst.line,
        col: worst.col,
        cyclomatic: totals.rollup_cyc,
        cognitive: totals.rollup_cog,
        line_count: totals.rollup_lines,
        param_count: 0,
        exceeded: ExceededThreshold::from_bools(
            totals.exceeds_cyclomatic,
            totals.exceeds_cognitive,
            false,
        ),
        severity: compute_finding_severity(
            totals.rollup_cog,
            totals.rollup_cyc,
            None,
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        ),
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        component_rollup: Some(ComponentRollup {
            component,
            class_worst_function: worst.name.clone(),
            class_cyclomatic: worst.cyclomatic,
            class_cognitive: worst.cognitive,
            template_path: template.path.clone(),
            template_cyclomatic: template.cyclomatic,
            template_cognitive: template.cognitive,
        }),
        contributions: Vec::new(),
        effective_thresholds: applied.override_index.map(|_| applied.effective),
        threshold_source: applied
            .override_index
            .map(|_| fallow_output::ThresholdSource::Override),
    }
}

fn build_component_rollup(
    owner: PathBuf,
    worst: &ComplexityViolation,
    template: &ComplexityViolation,
    resolver: &ThresholdOverrideResolver,
    config_root: &Path,
    state_tracker: &mut ThresholdOverrideStateTracker,
) -> Option<ComplexityViolation> {
    let rollup_cyc = worst.cyclomatic.saturating_add(template.cyclomatic);
    let rollup_cog = worst.cognitive.saturating_add(template.cognitive);
    let rollup_lines = worst.line_count.saturating_add(template.line_count);
    // The rollup is a finding in its own right, so it has to be measured against
    // the owner file's resolved ceilings. Quoting the run globals here left a
    // `health.thresholdOverrides` entry visibly ignored on the `<component>`
    // surface, which is issue #2163's original complaint.
    let relative = owner.strip_prefix(config_root).unwrap_or(owner.as_path());
    let (applied, matches) = resolver.resolve(relative, COMPONENT_ROLLUP_NAME);
    // Recorded before the ceiling check, not behind it: an entry that lifts the
    // rollup back under its ceilings is exactly the one whose row must read
    // `active`. Skipping it here is what let a `functions: ["<component>"]`
    // entry silence the rollup while the report still called it `no_match`, the
    // user's only signal that a glob matched nothing (issue #2163).
    state_tracker.record_complexity(
        ComplexityFunctionContext {
            path: owner.as_path(),
            function: COMPONENT_ROLLUP_NAME,
            line: worst.line,
            col: worst.col,
            cyclomatic: rollup_cyc,
            cognitive: rollup_cog,
            // Zero, not the rollup's own span: the rollup is never measured
            // against `maxUnitSize` and never enters the large-function list, so
            // scoring a unit-size term here would let a row read `insufficient`
            // over a breach no other part of the report can show.
            line_count: 0,
        },
        &matches,
        resolver.global,
        applied.effective,
    );
    let exceeds_cyclomatic = rollup_cyc > applied.effective.max_cyclomatic;
    let exceeds_cognitive = rollup_cog > applied.effective.max_cognitive;
    if !exceeds_cyclomatic && !exceeds_cognitive {
        return None;
    }

    let totals = ComponentRollupTotals {
        rollup_cyc,
        rollup_cog,
        rollup_lines,
        exceeds_cyclomatic,
        exceeds_cognitive,
    };
    Some(make_component_rollup_violation(
        owner, worst, template, &totals, applied,
    ))
}
