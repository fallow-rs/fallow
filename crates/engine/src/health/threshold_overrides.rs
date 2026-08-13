//! Health threshold override resolution and reporting state.

use std::path::{Path, PathBuf};

use fallow_output::ThresholdOverrideDimension;
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy)]
pub(super) struct GlobalHealthThresholds {
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) crap: f64,
    pub(super) unit_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AppliedHealthThresholds {
    pub(super) effective: fallow_output::HealthEffectiveThresholds,
    pub(super) override_index: Option<usize>,
}

pub(super) struct CompiledThresholdOverride {
    index: usize,
    matchers: globset::GlobSet,
    functions: Vec<String>,
    configured: fallow_output::HealthConfiguredThresholds,
    reason: Option<String>,
}

pub(super) struct ThresholdOverrideMatch<'a> {
    entry: &'a CompiledThresholdOverride,
}

pub(super) struct ThresholdOverrideResolver {
    entries: Vec<CompiledThresholdOverride>,
    pub(super) global: GlobalHealthThresholds,
}

impl ThresholdOverrideResolver {
    #[must_use]
    pub(super) fn new(
        overrides: &[fallow_config::HealthThresholdOverride],
        global: GlobalHealthThresholds,
    ) -> Self {
        let entries = overrides
            .iter()
            .enumerate()
            .map(|(index, override_entry)| {
                let mut builder = globset::GlobSetBuilder::new();
                for pattern in &override_entry.files {
                    if let Ok(glob) = globset::Glob::new(pattern) {
                        builder.add(glob);
                    }
                }
                CompiledThresholdOverride {
                    index,
                    matchers: builder
                        .build()
                        .unwrap_or_else(|_| globset::GlobSet::empty()),
                    functions: override_entry.functions.clone(),
                    configured: fallow_output::HealthConfiguredThresholds {
                        max_cyclomatic: override_entry.max_cyclomatic,
                        max_cognitive: override_entry.max_cognitive,
                        max_crap: override_entry.max_crap,
                        max_unit_size: override_entry.max_unit_size,
                    },
                    reason: override_entry.reason.clone(),
                }
            })
            .collect();
        Self { entries, global }
    }

    #[must_use]
    pub(super) fn resolve(
        &self,
        relative: &Path,
        function: &str,
    ) -> (AppliedHealthThresholds, Vec<ThresholdOverrideMatch<'_>>) {
        let mut effective = fallow_output::HealthEffectiveThresholds {
            max_cyclomatic: self.global.cyclomatic,
            max_cognitive: self.global.cognitive,
            max_crap: self.global.crap,
            max_unit_size: self.global.unit_size,
        };
        let mut override_index = None;
        let mut matches = Vec::new();

        for entry in &self.entries {
            if !entry.matchers.is_match(relative) {
                continue;
            }
            if !entry.functions.is_empty() && !entry.functions.iter().any(|f| f == function) {
                continue;
            }
            if let Some(max_cyclomatic) = entry.configured.max_cyclomatic {
                effective.max_cyclomatic = max_cyclomatic;
                override_index = Some(entry.index);
            }
            if let Some(max_cognitive) = entry.configured.max_cognitive {
                effective.max_cognitive = max_cognitive;
                override_index = Some(entry.index);
            }
            if let Some(max_crap) = entry.configured.max_crap {
                effective.max_crap = max_crap;
                override_index = Some(entry.index);
            }
            // The unit-size dimension gates the large-function list, not the
            // complexity findings, so it fills in the effective value without
            // flipping `override_index` (which controls whether a *complexity*
            // finding attaches an effective-thresholds block).
            if let Some(max_unit_size) = entry.configured.max_unit_size {
                effective.max_unit_size = max_unit_size;
            }
            matches.push(ThresholdOverrideMatch { entry });
        }

        (
            AppliedHealthThresholds {
                effective,
                override_index,
            },
            matches,
        )
    }

    /// Resolve the effective unit-size (large-function line-count) ceiling for a
    /// single function, applying the last matching override on top of the global
    /// `health.maxUnitSize` default. Cheaper than [`resolve`](Self::resolve): it
    /// allocates nothing, so it is safe to call in the large-function hot loop.
    #[must_use]
    pub(super) fn effective_max_unit_size(&self, relative: &Path, function: &str) -> u32 {
        let mut effective = self.global.unit_size;
        for entry in &self.entries {
            if !entry.matchers.is_match(relative) {
                continue;
            }
            if !entry.functions.is_empty() && !entry.functions.iter().any(|f| f == function) {
                continue;
            }
            if let Some(max_unit_size) = entry.configured.max_unit_size {
                effective = max_unit_size;
            }
        }
        effective
    }

    /// Resolve the effective CRAP ceiling for a single function, applying the
    /// last matching override on top of the global `maxCrap` / `--max-crap`
    /// value. Cheaper than [`resolve`](Self::resolve): it allocates nothing, so
    /// it is safe to call in the per-file scoring hot loop.
    #[must_use]
    pub(super) fn effective_max_crap(&self, relative: &Path, function: &str) -> f64 {
        let mut effective = self.global.crap;
        for entry in &self.entries {
            if !entry.matchers.is_match(relative) {
                continue;
            }
            if !entry.functions.is_empty() && !entry.functions.iter().any(|f| f == function) {
                continue;
            }
            if let Some(max_crap) = entry.configured.max_crap {
                effective = max_crap;
            }
        }
        effective
    }

    fn entries(&self) -> &[CompiledThresholdOverride] {
        &self.entries
    }

    /// True when no `thresholdOverrides` entries are configured. Lets callers
    /// on cold paths (suppressed functions) skip `resolve` entirely, so the
    /// common no-override run does zero extra work per suppressed unit.
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThresholdOverrideStateKey {
    status: &'static str,
    override_index: usize,
    path: Option<PathBuf>,
    function: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    dimension: ThresholdOverrideDimension,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MeasuredThresholdMetrics {
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) crap: f64,
}

#[derive(Default)]
pub(super) struct ThresholdOverrideStateTracker {
    matched_indexes: FxHashSet<usize>,
    seen: FxHashSet<ThresholdOverrideStateKey>,
    states: Vec<fallow_output::ThresholdOverrideState>,
}

/// Dimensions a configured entry participates in, in serialization order.
///
/// `complexity` covers the structural ceilings (cyclomatic, cognitive, unit
/// size) and `crap` covers CRAP alone, matching the enum's own contract. Used
/// only by the `no_match` path, where there is no measured function to derive a
/// dimension from. `HealthConfig::validate` rejects an entry that configures no
/// ceiling at all, so this never yields an empty list for a real entry.
fn configured_dimensions(
    configured: fallow_output::HealthConfiguredThresholds,
) -> Vec<ThresholdOverrideDimension> {
    let mut dimensions = Vec::new();
    if configured.max_cyclomatic.is_some()
        || configured.max_cognitive.is_some()
        || configured.max_unit_size.is_some()
    {
        dimensions.push(ThresholdOverrideDimension::Complexity);
    }
    if configured.max_crap.is_some() {
        dimensions.push(ThresholdOverrideDimension::Crap);
    }
    dimensions
}

impl ThresholdOverrideStateTracker {
    /// `effective` is the ceiling set resolved across every matching entry, not
    /// the entry's own configured value. A later entry can supersede an earlier
    /// one, and the finding is emitted against the resolved value, so measuring
    /// the row against anything else lets `insufficient` claim a finding that
    /// never fires (issue #2163).
    pub(super) fn record_complexity(
        &mut self,
        function: ComplexityFunctionContext<'_>,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
        effective: fallow_output::HealthEffectiveThresholds,
    ) {
        let ComplexityFunctionContext {
            path,
            function,
            line,
            col,
            cyclomatic,
            cognitive,
            line_count,
            suppressed,
        } = function;
        for matched in matches {
            let configured = matched.entry.configured;
            // `maxUnitSize` belongs to the complexity dimension, per
            // `configured_dimensions` and the `ThresholdOverrideDimension`
            // contract. Leaving it out made a unit-size-only entry emit no row
            // at all when it matched, while the same entry pointed at a typo
            // glob still emitted `no_match`: silent when in force, loud when
            // inert (issue #2163).
            let touches_unit_size = configured.max_unit_size.is_some();
            let has_complexity_threshold = configured.max_cyclomatic.is_some()
                || configured.max_cognitive.is_some()
                || touches_unit_size;
            if !has_complexity_threshold {
                continue;
            }
            // Marked only once this recorder is committed to emitting a row.
            // Marking on entry made an index count as matched while no recorder
            // produced anything for it, which excluded it from
            // `record_no_match_entries` too: a `maxCrap`-only entry scoped to
            // `<component>` went silent on both paths (issue #2163).
            self.matched_indexes.insert(matched.entry.index);
            // Both sides run the SAME dimension predicate, once against the
            // global ceilings and once against the resolved ones, so `active`,
            // `stale` and `insufficient` all describe the complexity dimension
            // as a whole and agree with the `outstanding` list. Scoring each
            // ceiling in isolation let one row claim the dimension was satisfied
            // while `outstanding` said it still fires (issue #2163): an entry
            // raising only `maxCyclomatic` on a unit that breaches cognitive
            // read `active`, and a `maxUnitSize`-only entry on a unit breaching
            // cyclomatic read `stale`.
            //
            // Unit size is the one narrowed term. It emits no complexity
            // finding, so folding an unrelated long function into the predicate
            // would flip a cyclomatic-only override to `insufficient` with
            // nothing outstanding to point at.
            //
            // Suppression neutralizes only the finding-driven terms: an inline
            // `fallow-ignore` comment hides the complexity finding, so a raised
            // cyclomatic/cognitive ceiling buys nothing there and the row reads
            // `stale`, mirroring `record_crap`. The unit-size term keeps real
            // scoring because suppression covers the finding only, never the
            // large-function list, so a raised `maxUnitSize` still has effect
            // on a suppressed unit (issue #2163 follow-up).
            let breaches = |max_cyclomatic: u16, max_cognitive: u16, max_unit_size: u32| {
                (!suppressed && (cyclomatic > max_cyclomatic || cognitive > max_cognitive))
                    || (touches_unit_size && line_count.is_some_and(|lc| lc > max_unit_size))
            };
            let global_exceeded = breaches(global.cyclomatic, global.cognitive, global.unit_size);
            let local_exceeded = breaches(
                effective.max_cyclomatic,
                effective.max_cognitive,
                effective.max_unit_size,
            );
            let status = if global_exceeded && !local_exceeded {
                fallow_output::ThresholdOverrideStatus::Active
            } else if !global_exceeded {
                fallow_output::ThresholdOverrideStatus::Stale
            } else {
                fallow_output::ThresholdOverrideStatus::Insufficient
            };
            // A surviving unit-size breach has to be seeded here, because
            // `annotate_outstanding_dimensions` derives `outstanding` from the
            // findings list and unit size emits no finding. Without it a
            // unit-size-only entry that raised the ceiling without clearing it
            // read `insufficient` with an empty `outstanding`, the one
            // contradiction a CI gate cannot detect (issue #2163).
            let outstanding =
                if touches_unit_size && line_count.is_some_and(|lc| lc > effective.max_unit_size) {
                    vec![ThresholdOverrideDimension::Complexity]
                } else {
                    Vec::new()
                };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                outstanding,
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                line: Some(line),
                col: Some(col),
                configured_thresholds: configured,
                effective_thresholds: effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic,
                    cognitive,
                    crap: None,
                    line_count,
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Complexity,
            });
        }
    }

    /// `effective` is the resolved CRAP ceiling across every matching entry, for
    /// the same reason as [`record_complexity`](Self::record_complexity): the
    /// finding is emitted against the resolved value, so the row has to be
    /// scored against it too (issue #2163).
    pub(super) fn record_crap(
        &mut self,
        function: CrapFunctionContext<'_>,
        metrics: MeasuredThresholdMetrics,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
        effective: fallow_output::HealthEffectiveThresholds,
    ) {
        let CrapFunctionContext {
            path,
            function,
            line,
            col,
            suppressed,
        } = function;
        for matched in matches {
            if matched.entry.configured.max_crap.is_none() {
                continue;
            }
            self.matched_indexes.insert(matched.entry.index);
            let status = if suppressed || metrics.crap < global.crap {
                fallow_output::ThresholdOverrideStatus::Stale
            } else if metrics.crap < effective.max_crap {
                fallow_output::ThresholdOverrideStatus::Active
            } else {
                fallow_output::ThresholdOverrideStatus::Insufficient
            };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                outstanding: Vec::new(),
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                line: Some(line),
                col: Some(col),
                configured_thresholds: matched.entry.configured,
                effective_thresholds: effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic: metrics.cyclomatic,
                    cognitive: metrics.cognitive,
                    crap: Some(metrics.crap),
                    // The CRAP dimension never scores unit size, so an absent
                    // field keeps the row honest next to a CRAP-breach claim.
                    line_count: None,
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Crap,
            });
        }
    }

    /// Record a matched CRAP-dimension row for a synthetic template-family
    /// unit. Template units are not scored on the CRAP dimension, so the
    /// per-function CRAP loop never reaches them and `record_crap` never
    /// runs; without this entry point a `maxCrap`-only override scoped to
    /// `<template>` (the exact config shape v3.15.0 recommended) regressed
    /// to a first-sorted `no_match` row (issue #2235).
    ///
    /// The row always reads `stale` with an empty `outstanding` list and
    /// `metrics.crap` absent: absent, not `0.0`, because `0.0` would assert
    /// a measurement that was never taken. The renderers pair this shape
    /// with copy saying the unit is not scored on CRAP and the entry can be
    /// removed.
    pub(super) fn record_crap_not_applicable(
        &mut self,
        function: CrapFunctionContext<'_>,
        measured: (u16, u16),
        matches: &[ThresholdOverrideMatch<'_>],
        effective: fallow_output::HealthEffectiveThresholds,
    ) {
        let CrapFunctionContext {
            path,
            function,
            line,
            col,
            suppressed: _,
        } = function;
        let (cyclomatic, cognitive) = measured;
        for matched in matches {
            if matched.entry.configured.max_crap.is_none() {
                continue;
            }
            self.matched_indexes.insert(matched.entry.index);
            self.push_state(ThresholdOverrideStateInput {
                status: fallow_output::ThresholdOverrideStatus::Stale,
                override_index: matched.entry.index,
                outstanding: Vec::new(),
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                line: Some(line),
                col: Some(col),
                configured_thresholds: matched.entry.configured,
                effective_thresholds: effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic,
                    cognitive,
                    crap: None,
                    line_count: None,
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Crap,
            });
        }
    }

    pub(super) fn record_no_match_entries(
        &mut self,
        resolver: &ThresholdOverrideResolver,
        should_emit: bool,
    ) {
        if !should_emit {
            return;
        }
        for entry in resolver.entries() {
            if self.matched_indexes.contains(&entry.index) {
                continue;
            }
            let effective = fallow_output::HealthEffectiveThresholds {
                max_cyclomatic: entry
                    .configured
                    .max_cyclomatic
                    .unwrap_or(resolver.global.cyclomatic),
                max_cognitive: entry
                    .configured
                    .max_cognitive
                    .unwrap_or(resolver.global.cognitive),
                max_crap: entry.configured.max_crap.unwrap_or(resolver.global.crap),
                max_unit_size: entry
                    .configured
                    .max_unit_size
                    .unwrap_or(resolver.global.unit_size),
            };
            for dimension in configured_dimensions(entry.configured) {
                self.push_state(ThresholdOverrideStateInput {
                    status: fallow_output::ThresholdOverrideStatus::NoMatch,
                    override_index: entry.index,
                    outstanding: Vec::new(),
                    path: None,
                    function: None,
                    line: None,
                    col: None,
                    configured_thresholds: entry.configured,
                    effective_thresholds: effective,
                    metrics: None,
                    reason: entry.reason.clone(),
                    dimension,
                });
            }
        }
    }

    /// Mutable access to the accumulated rows so the findings pipeline can
    /// annotate `outstanding` once every finding has been collected and merged.
    /// The tracker itself records during collection, before the CRAP merge, so
    /// it cannot know which dimension ultimately kept a finding alive.
    pub(super) fn states_mut(&mut self) -> &mut [fallow_output::ThresholdOverrideState] {
        &mut self.states
    }

    pub(super) fn into_states(mut self) -> Vec<fallow_output::ThresholdOverrideState> {
        self.states.sort_by(|a, b| {
            a.override_index
                .cmp(&b.override_index)
                .then(a.path.cmp(&b.path))
                .then(a.function.cmp(&b.function))
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.dimension.cmp(&b.dimension))
        });
        self.states
    }

    fn push_state(&mut self, input: ThresholdOverrideStateInput) {
        let status_key = match input.status {
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::Insufficient => "insufficient",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let key = ThresholdOverrideStateKey {
            status: status_key,
            override_index: input.override_index,
            path: input.path.clone(),
            function: input.function.clone(),
            line: input.line,
            col: input.col,
            dimension: input.dimension,
        };
        if !self.seen.insert(key) {
            return;
        }
        self.states.push(fallow_output::ThresholdOverrideState {
            status: input.status,
            override_index: input.override_index,
            dimension: input.dimension,
            outstanding: input.outstanding,
            path: input.path,
            function: input.function,
            line: input.line,
            col: input.col,
            configured_thresholds: input.configured_thresholds,
            effective_thresholds: input.effective_thresholds,
            metrics: input.metrics,
            reason: input.reason,
        });
    }
}

/// One function's identity (path, name and position) and measured complexity
/// metrics, bundled so `record_complexity` takes the function descriptor as a
/// single parameter instead of seven.
///
/// `line_count` is here because `maxUnitSize` is part of the complexity
/// dimension: without it a unit-size-only entry has nothing to score its row
/// against. `None` means the caller has no unit-size-scorable span: the
/// `<component>` rollup is never measured against `maxUnitSize`, so a `Some`
/// there would let a row read `insufficient` over a breach no other part of
/// the report can show.
///
/// `suppressed` mirrors [`CrapFunctionContext`]: an inline `fallow-ignore`
/// comment hides the complexity finding, so the row must read `stale` instead
/// of falling through to `no_match` (issue #2163 follow-up).
#[derive(Clone, Copy)]
pub(super) struct ComplexityFunctionContext<'a> {
    pub(super) path: &'a Path,
    pub(super) function: &'a str,
    pub(super) line: u32,
    pub(super) col: u32,
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) line_count: Option<u32>,
    pub(super) suppressed: bool,
}

/// One function's identity and suppression state for the CRAP recorder.
///
/// Position matters: several units in one file can share a name, and the row
/// must stay distinct per unit. `suppressed` reports whether a
/// `fallow-ignore-*` comment already hides the CRAP finding; a suppressed unit
/// emits no finding, so the raised ceiling buys nothing and the row reads
/// `stale`. Without it the row claimed `insufficient` while `outstanding`
/// stayed empty, the one contradiction a CI gate cannot detect (issue #2163).
#[derive(Clone, Copy)]
pub(super) struct CrapFunctionContext<'a> {
    pub(super) path: &'a Path,
    pub(super) function: &'a str,
    pub(super) line: u32,
    pub(super) col: u32,
    pub(super) suppressed: bool,
}

struct ThresholdOverrideStateInput {
    status: fallow_output::ThresholdOverrideStatus,
    override_index: usize,
    /// Dimensions the recorder already knows still breach. Only the unit-size
    /// term lands here; everything else is filled in later from the findings
    /// list by `annotate_outstanding_dimensions`.
    outstanding: Vec<ThresholdOverrideDimension>,
    path: Option<PathBuf>,
    function: Option<String>,
    line: Option<u32>,
    col: Option<u32>,
    configured_thresholds: fallow_output::HealthConfiguredThresholds,
    effective_thresholds: fallow_output::HealthEffectiveThresholds,
    metrics: Option<fallow_output::ThresholdOverrideMetrics>,
    reason: Option<String>,
    dimension: ThresholdOverrideDimension,
}
