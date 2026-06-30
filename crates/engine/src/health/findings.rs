//! Complexity finding collection and CRAP merge helpers.

use fallow_output::{
    ComplexityViolation, DEFAULT_COGNITIVE_CRITICAL, DEFAULT_COGNITIVE_HIGH,
    DEFAULT_CYCLOMATIC_CRITICAL, DEFAULT_CYCLOMATIC_HIGH, ExceededThreshold,
    compute_finding_severity,
};

#[cfg(test)]
use super::threshold_overrides::GlobalHealthThresholds;
use super::threshold_overrides::{
    AppliedHealthThresholds, ComplexityFunctionContext, MeasuredThresholdMetrics,
    ThresholdOverrideResolver, ThresholdOverrideStateTracker,
};
use super::{react_hooks, scoring};

/// Collect health findings from parsed modules, applying ignore, changed-since,
/// and workspace filters. The returned `files_analyzed` / `total_functions`
/// counters reflect only modules that pass every filter so the rendered
/// summary matches the produced findings.
#[expect(
    clippy::too_many_arguments,
    reason = "filter pipeline mirrors compute_filtered_file_scores"
)]
#[cfg(test)]
pub(super) fn collect_findings(
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    config_root: &std::path::Path,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    complexity_breakdown: bool,
) -> (Vec<ComplexityViolation>, usize, usize) {
    let global = GlobalHealthThresholds {
        cyclomatic: max_cyclomatic,
        cognitive: max_cognitive,
        crap: 30.0,
    };
    let resolver = ThresholdOverrideResolver::new(&[], global);
    let mut tracker = ThresholdOverrideStateTracker::default();
    let mut input = CollectFindingsInput {
        modules,
        file_paths,
        config_root,
        ignore_set,
        changed_files,
        ws_roots,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
        complexity_breakdown,
    };
    collect_findings_with_resolver(&mut input)
}

pub(super) struct CollectFindingsInput<'a> {
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) config_root: &'a std::path::Path,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) threshold_resolver: &'a ThresholdOverrideResolver,
    pub(super) threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
    pub(super) complexity_breakdown: bool,
}

pub(super) fn collect_findings_with_resolver(
    input: &mut CollectFindingsInput<'_>,
) -> (Vec<ComplexityViolation>, usize, usize) {
    let mut files_analyzed = 0usize;
    let mut total_functions = 0usize;
    let mut findings: Vec<ComplexityViolation> = Vec::new();

    for module in input.modules {
        let Some((path, relative)) = collect_findings_module_path(input, module) else {
            continue;
        };

        files_analyzed += 1;
        // Precompute the per-function React hook profile ONCE per module from the
        // cached `hook_uses` IR (the sole reader of `module.hook_uses`). Aligned
        // by index to `module.complexity`; all-`None` at zero cost for non-React
        // files (empty `hook_uses`).
        let hook_profiles = react_hooks::build_module_hook_profiles(module);
        for (fc_idx, fc) in module.complexity.iter().enumerate() {
            total_functions += 1;
            if crate::suppress::is_suppressed(
                &module.suppressions,
                fc.line,
                crate::suppress::IssueKind::Complexity,
            ) {
                continue;
            }
            let react_hook_profile = hook_profiles.get(fc_idx).cloned().flatten();
            if let Some(finding) =
                collect_complexity_finding(input, path, relative, fc, react_hook_profile)
            {
                findings.push(finding);
            }
        }
    }

    (findings, files_analyzed, total_functions)
}

fn collect_findings_module_path<'a>(
    input: &CollectFindingsInput<'a>,
    module: &crate::source::ModuleInfo,
) -> Option<(&'a std::path::PathBuf, &'a std::path::Path)> {
    let &path = input.file_paths.get(&module.file_id)?;
    let relative = path.strip_prefix(input.config_root).unwrap_or(path);
    if input.ignore_set.is_match(relative) {
        return None;
    }
    if let Some(changed) = input.changed_files
        && !changed.contains(path)
    {
        return None;
    }
    if let Some(ws) = input.ws_roots
        && !ws.iter().any(|root| path.starts_with(root))
    {
        return None;
    }
    Some((path, relative))
}

fn collect_complexity_finding(
    input: &mut CollectFindingsInput<'_>,
    path: &std::path::Path,
    relative: &std::path::Path,
    fc: &fallow_types::extract::FunctionComplexity,
    react_hook_profile: Option<fallow_output::ReactHookProfile>,
) -> Option<ComplexityViolation> {
    let (applied_thresholds, matched_overrides) =
        input.threshold_resolver.resolve(relative, &fc.name);
    input.threshold_state_tracker.record_complexity(
        ComplexityFunctionContext {
            path,
            function: &fc.name,
            cyclomatic: fc.cyclomatic,
            cognitive: fc.cognitive,
        },
        &matched_overrides,
        input.threshold_resolver.global,
    );
    let exceeds_cyclomatic = fc.cyclomatic > applied_thresholds.effective.max_cyclomatic;
    let exceeds_cognitive = fc.cognitive > applied_thresholds.effective.max_cognitive;
    if !exceeds_cyclomatic && !exceeds_cognitive {
        return None;
    }

    Some(ComplexityViolation {
        path: path.to_path_buf(),
        name: fc.name.clone(),
        line: fc.line,
        col: fc.col,
        cyclomatic: fc.cyclomatic,
        cognitive: fc.cognitive,
        line_count: fc.line_count,
        param_count: fc.param_count,
        react_hook_count: fc.react_hook_count,
        react_jsx_max_depth: fc.react_jsx_max_depth,
        react_prop_count: fc.react_prop_count,
        react_hook_profile,
        exceeded: ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, false),
        severity: compute_finding_severity(
            fc.cognitive,
            fc.cyclomatic,
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
        component_rollup: None,
        contributions: contributions_for(input.complexity_breakdown, fc),
        effective_thresholds: applied_thresholds
            .override_index
            .map(|_| applied_thresholds.effective),
        threshold_source: applied_thresholds
            .override_index
            .map(|_| fallow_output::ThresholdSource::Override),
    })
}

/// Clone the per-decision-point breakdown onto a finding only when the caller
/// opted in via `health --complexity-breakdown`; otherwise leave it empty so it
/// is omitted from JSON.
fn contributions_for(
    complexity_breakdown: bool,
    fc: &fallow_types::extract::FunctionComplexity,
) -> Vec<fallow_types::extract::ComplexityContribution> {
    if complexity_breakdown {
        fc.contributions.clone()
    } else {
        Vec::new()
    }
}

/// Merge per-function CRAP data into an existing complexity findings vector.
///
/// Functions that only exceed `--max-crap` (without exceeding cyclomatic or
/// cognitive) become new findings. Functions that already produced a finding
/// for cyclomatic/cognitive get their `crap` and `coverage_pct` fields
/// populated, and the `exceeded` discriminant plus `severity` are recomputed
/// to reflect CRAP's contribution.
pub(super) struct CrapFindingMergeInput<'a> {
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) config_root: &'a std::path::Path,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) per_function_crap:
        &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<scoring::PerFunctionCrap>>,
    pub(super) template_inherit_provenance:
        &'a rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
    pub(super) complexity_breakdown: bool,
    pub(super) threshold_resolver: &'a ThresholdOverrideResolver,
    pub(super) threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
}

type ComplexityByPosition<'a> = rustc_hash::FxHashMap<
    &'a std::path::Path,
    rustc_hash::FxHashMap<(u32, u32), &'a fallow_types::extract::FunctionComplexity>,
>;

/// The precomputed position-keyed lookup maps shared across the CRAP merge pass:
/// existing-finding index, per-function complexity, React hook profiles, and
/// per-path suppressions.
struct CrapMergeMaps<'a> {
    finding_index: rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize>,
    complexity_by_pos: ComplexityByPosition<'a>,
    hook_profiles_by_pos: rustc_hash::FxHashMap<
        &'a std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
    >,
    suppressions_by_path:
        rustc_hash::FxHashMap<&'a std::path::Path, &'a Vec<crate::suppress::Suppression>>,
}

/// Process one path's per-function CRAP entries: record threshold state, skip
/// below-threshold / suppressed frames, then merge into an existing finding or
/// append a new one to `new_findings`.
fn process_crap_findings_for_path(
    path: &std::path::Path,
    per_fn: &[scoring::PerFunctionCrap],
    maps: &CrapMergeMaps<'_>,
    findings: &mut [ComplexityViolation],
    new_findings: &mut Vec<ComplexityViolation>,
    input: &mut CrapFindingMergeInput<'_>,
) {
    for pf in per_fn {
        let Some(fc) = maps
            .complexity_by_pos
            .get(path)
            .and_then(|m| m.get(&(pf.line, pf.col)).copied())
        else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        let (applied_thresholds, matched_overrides) =
            input.threshold_resolver.resolve(relative, &fc.name);
        input.threshold_state_tracker.record_crap(
            path,
            &fc.name,
            MeasuredThresholdMetrics {
                cyclomatic: fc.cyclomatic,
                cognitive: fc.cognitive,
                crap: pf.crap,
            },
            &matched_overrides,
            input.threshold_resolver.global,
        );
        if pf.crap < applied_thresholds.effective.max_crap
            || crap_is_suppressed(path, pf, &maps.suppressions_by_path)
        {
            continue;
        }

        if let Some(&idx) = maps
            .finding_index
            .get(&(path.to_path_buf(), pf.line, pf.col))
        {
            merge_existing_crap_finding(&mut findings[idx], path, pf, input, applied_thresholds);
        } else {
            let hook_profile = maps
                .hook_profiles_by_pos
                .get(path)
                .and_then(|m| m.get(&(pf.line, pf.col)).cloned());
            new_findings.push(new_crap_finding(
                path,
                pf,
                fc,
                hook_profile,
                input,
                applied_thresholds,
            ));
        }
    }
}

pub(super) fn merge_crap_findings(
    findings: &mut Vec<ComplexityViolation>,
    input: &mut CrapFindingMergeInput<'_>,
) {
    // Copy the `'a` references out so the lookup maps and the per-function map
    // borrow the underlying analysis data, not `input`, leaving `input` free to
    // be passed mutably into the per-path processor below.
    let modules = input.modules;
    let file_paths = input.file_paths;
    let per_function_crap = input.per_function_crap;
    let maps = CrapMergeMaps {
        finding_index: build_complexity_finding_index(findings),
        complexity_by_pos: build_complexity_by_position(modules, file_paths),
        hook_profiles_by_pos: build_hook_profiles_by_position(modules, file_paths),
        suppressions_by_path: build_complexity_suppressions_by_path(modules, file_paths),
    };

    let mut new_findings: Vec<ComplexityViolation> = Vec::new();
    for (path, per_fn) in per_function_crap {
        if !crap_path_in_scope(path, input) {
            continue;
        }
        process_crap_findings_for_path(path, per_fn, &maps, findings, &mut new_findings, input);
    }
    findings.extend(new_findings);
}

fn build_complexity_finding_index(
    findings: &[ComplexityViolation],
) -> rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize> {
    findings
        .iter()
        .enumerate()
        .map(|(idx, f)| ((f.path.clone(), f.line, f.col), idx))
        .collect()
}

fn build_complexity_by_position<'a>(
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> ComplexityByPosition<'a> {
    let mut complexity_by_pos: ComplexityByPosition<'a> = rustc_hash::FxHashMap::default();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let entry = complexity_by_pos.entry(path.as_path()).or_default();
        for fc in &module.complexity {
            entry.insert((fc.line, fc.col), fc);
        }
    }
    complexity_by_pos
}

/// Build a `path -> (line, col) -> ReactHookProfile` map by precomputing each
/// module's per-function hook profile ONCE (the CRAP path keys findings by
/// `(line, col)`, so the profile must be addressable the same way). Frames with
/// no attributed component-scope hook are omitted; non-React modules contribute
/// nothing.
fn build_hook_profiles_by_position<'a>(
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> rustc_hash::FxHashMap<
    &'a std::path::Path,
    rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
> {
    let mut by_pos: rustc_hash::FxHashMap<
        &'a std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
    > = rustc_hash::FxHashMap::default();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let profiles = react_hooks::build_module_hook_profiles(module);
        let mut frame_profiles = rustc_hash::FxHashMap::default();
        for (fc, profile) in module.complexity.iter().zip(profiles) {
            if let Some(profile) = profile {
                frame_profiles.insert((fc.line, fc.col), profile);
            }
        }
        if !frame_profiles.is_empty() {
            by_pos.insert(path.as_path(), frame_profiles);
        }
    }
    by_pos
}

fn build_complexity_suppressions_by_path<'a>(
    modules: &'a [crate::source::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> rustc_hash::FxHashMap<&'a std::path::Path, &'a Vec<crate::suppress::Suppression>> {
    modules
        .iter()
        .filter_map(|module| {
            file_paths
                .get(&module.file_id)
                .map(|path| (path.as_path(), &module.suppressions))
        })
        .collect()
}

fn crap_path_in_scope(path: &std::path::Path, input: &CrapFindingMergeInput<'_>) -> bool {
    let relative = path.strip_prefix(input.config_root).unwrap_or(path);
    if input.ignore_set.is_match(relative) {
        return false;
    }
    if let Some(changed) = input.changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = input.ws_roots
        && !ws.iter().any(|r| path.starts_with(r))
    {
        return false;
    }
    true
}

fn crap_is_suppressed(
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    suppressions_by_path: &rustc_hash::FxHashMap<
        &std::path::Path,
        &Vec<crate::suppress::Suppression>,
    >,
) -> bool {
    suppressions_by_path.get(path).is_some_and(|sups| {
        crate::suppress::is_suppressed(sups, pf.line, crate::suppress::IssueKind::Complexity)
    })
}

fn merge_existing_crap_finding(
    finding: &mut ComplexityViolation,
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    input: &CrapFindingMergeInput<'_>,
    applied_thresholds: AppliedHealthThresholds,
) {
    finding.crap = Some(pf.crap);
    finding.coverage_pct = pf.coverage_pct;
    finding.coverage_tier = Some(pf.coverage_tier);
    finding.coverage_source = Some(pf.coverage_source);
    finding.inherited_from =
        inherited_from_for(pf.coverage_source, path, input.template_inherit_provenance);
    let exceeds_cyclomatic = finding.exceeded.includes_cyclomatic();
    let exceeds_cognitive = finding.exceeded.includes_cognitive();
    finding.exceeded = ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, true);
    if applied_thresholds.override_index.is_some() {
        finding.effective_thresholds = Some(applied_thresholds.effective);
        finding.threshold_source = Some(fallow_output::ThresholdSource::Override);
    }
    finding.severity = compute_finding_severity(
        finding.cognitive,
        finding.cyclomatic,
        Some(pf.crap),
        DEFAULT_COGNITIVE_HIGH,
        DEFAULT_COGNITIVE_CRITICAL,
        DEFAULT_CYCLOMATIC_HIGH,
        DEFAULT_CYCLOMATIC_CRITICAL,
    );
}

fn new_crap_finding(
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    fc: &fallow_types::extract::FunctionComplexity,
    hook_profile: Option<fallow_output::ReactHookProfile>,
    input: &CrapFindingMergeInput<'_>,
    applied_thresholds: AppliedHealthThresholds,
) -> ComplexityViolation {
    let exceeds_cyclomatic = fc.cyclomatic > applied_thresholds.effective.max_cyclomatic;
    let exceeds_cognitive = fc.cognitive > applied_thresholds.effective.max_cognitive;
    ComplexityViolation {
        path: path.to_path_buf(),
        name: fc.name.clone(),
        line: fc.line,
        col: fc.col,
        cyclomatic: fc.cyclomatic,
        cognitive: fc.cognitive,
        line_count: fc.line_count,
        param_count: fc.param_count,
        react_hook_count: fc.react_hook_count,
        react_jsx_max_depth: fc.react_jsx_max_depth,
        react_prop_count: fc.react_prop_count,
        react_hook_profile: hook_profile,
        exceeded: ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, true),
        severity: compute_finding_severity(
            fc.cognitive,
            fc.cyclomatic,
            Some(pf.crap),
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        ),
        crap: Some(pf.crap),
        coverage_pct: pf.coverage_pct,
        coverage_tier: Some(pf.coverage_tier),
        coverage_source: Some(pf.coverage_source),
        inherited_from: inherited_from_for(
            pf.coverage_source,
            path,
            input.template_inherit_provenance,
        ),
        component_rollup: None,
        contributions: contributions_for(input.complexity_breakdown, fc),
        effective_thresholds: applied_thresholds
            .override_index
            .map(|_| applied_thresholds.effective),
        threshold_source: applied_thresholds
            .override_index
            .map(|_| fallow_output::ThresholdSource::Override),
    }
}

/// Resolve the `inherited_from` provenance path for a CRAP finding.
///
/// Returns `Some(owner_path)` only for the
/// `CoverageSource::EstimatedComponentInherited` variant, so the field stays
/// absent on every Istanbul / regular-estimated row. Pairs with the
/// `coverage_source` discriminator: any finding carrying
/// `estimated_component_inherited` also carries `inherited_from`, and vice
/// versa.
fn inherited_from_for(
    source: fallow_output::CoverageSource,
    template_path: &std::path::Path,
    template_inherit_provenance: &rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if matches!(
        source,
        fallow_output::CoverageSource::EstimatedComponentInherited
    ) {
        template_inherit_provenance.get(template_path).cloned()
    } else {
        None
    }
}
