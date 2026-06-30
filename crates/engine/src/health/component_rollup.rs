//! Angular component complexity rollup findings.

use std::path::PathBuf;

use fallow_output::{
    ComplexityViolation, ComponentRollup, DEFAULT_COGNITIVE_CRITICAL, DEFAULT_COGNITIVE_HIGH,
    DEFAULT_CYCLOMATIC_CRITICAL, DEFAULT_CYCLOMATIC_HIGH, ExceededThreshold,
    compute_finding_severity,
};

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
pub(super) fn append_component_rollup_findings(
    findings: &mut Vec<ComplexityViolation>,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<PathBuf, PathBuf>>,
    max_cyclomatic: u16,
    max_cognitive: u16,
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
            build_component_rollup(owner, worst, template, max_cyclomatic, max_cognitive)
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
    finding.name != "<component>"
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
) -> ComplexityViolation {
    let component = owner.file_stem().map_or_else(
        || "<unknown-component>".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    ComplexityViolation {
        path: owner,
        name: "<component>".to_string(),
        line: worst.line,
        col: worst.col,
        cyclomatic: totals.rollup_cyc,
        cognitive: totals.rollup_cog,
        line_count: worst.line_count.saturating_add(template.line_count),
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
        effective_thresholds: None,
        threshold_source: None,
    }
}

fn build_component_rollup(
    owner: PathBuf,
    worst: &ComplexityViolation,
    template: &ComplexityViolation,
    max_cyclomatic: u16,
    max_cognitive: u16,
) -> Option<ComplexityViolation> {
    let rollup_cyc = worst.cyclomatic.saturating_add(template.cyclomatic);
    let rollup_cog = worst.cognitive.saturating_add(template.cognitive);
    let exceeds_cyclomatic = rollup_cyc > max_cyclomatic;
    let exceeds_cognitive = rollup_cog > max_cognitive;
    if !exceeds_cyclomatic && !exceeds_cognitive {
        return None;
    }

    let totals = ComponentRollupTotals {
        rollup_cyc,
        rollup_cog,
        exceeds_cyclomatic,
        exceeds_cognitive,
    };
    Some(make_component_rollup_violation(
        owner, worst, template, &totals,
    ))
}
