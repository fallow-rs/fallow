//! Health threshold override resolution and reporting state.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy)]
pub(super) struct GlobalHealthThresholds {
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) crap: f64,
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
    effective: fallow_output::HealthEffectiveThresholds,
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
            matches.push(ThresholdOverrideMatch { entry, effective });
        }

        (
            AppliedHealthThresholds {
                effective,
                override_index,
            },
            matches,
        )
    }

    fn entries(&self) -> &[CompiledThresholdOverride] {
        &self.entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThresholdOverrideDimension {
    Complexity,
    Crap,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThresholdOverrideStateKey {
    status: &'static str,
    override_index: usize,
    path: Option<PathBuf>,
    function: Option<String>,
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

impl ThresholdOverrideStateTracker {
    pub(super) fn record_complexity(
        &mut self,
        function: ComplexityFunctionContext<'_>,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
    ) {
        let ComplexityFunctionContext {
            path,
            function,
            cyclomatic,
            cognitive,
        } = function;
        for matched in matches {
            self.matched_indexes.insert(matched.entry.index);
            let configured = matched.entry.configured;
            let has_complexity_threshold =
                configured.max_cyclomatic.is_some() || configured.max_cognitive.is_some();
            if !has_complexity_threshold {
                continue;
            }
            let global_exceeded = configured
                .max_cyclomatic
                .is_some_and(|_| cyclomatic > global.cyclomatic)
                || configured
                    .max_cognitive
                    .is_some_and(|_| cognitive > global.cognitive);
            let local_exceeded = configured
                .max_cyclomatic
                .is_some_and(|threshold| cyclomatic > threshold)
                || configured
                    .max_cognitive
                    .is_some_and(|threshold| cognitive > threshold);
            let status = if global_exceeded && !local_exceeded {
                fallow_output::ThresholdOverrideStatus::Active
            } else if !global_exceeded {
                fallow_output::ThresholdOverrideStatus::Stale
            } else {
                continue;
            };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                configured_thresholds: configured,
                effective_thresholds: matched.effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic,
                    cognitive,
                    crap: None,
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Complexity,
            });
        }
    }

    pub(super) fn record_crap(
        &mut self,
        path: &Path,
        function: &str,
        metrics: MeasuredThresholdMetrics,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
    ) {
        for matched in matches {
            self.matched_indexes.insert(matched.entry.index);
            let Some(max_crap) = matched.entry.configured.max_crap else {
                continue;
            };
            let status = if metrics.crap >= global.crap && metrics.crap < max_crap {
                fallow_output::ThresholdOverrideStatus::Active
            } else if metrics.crap < global.crap {
                fallow_output::ThresholdOverrideStatus::Stale
            } else {
                continue;
            };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                configured_thresholds: matched.entry.configured,
                effective_thresholds: matched.effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic: metrics.cyclomatic,
                    cognitive: metrics.cognitive,
                    crap: Some(metrics.crap),
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
            self.push_state(ThresholdOverrideStateInput {
                status: fallow_output::ThresholdOverrideStatus::NoMatch,
                override_index: entry.index,
                path: None,
                function: None,
                configured_thresholds: entry.configured,
                effective_thresholds: fallow_output::HealthEffectiveThresholds {
                    max_cyclomatic: entry
                        .configured
                        .max_cyclomatic
                        .unwrap_or(resolver.global.cyclomatic),
                    max_cognitive: entry
                        .configured
                        .max_cognitive
                        .unwrap_or(resolver.global.cognitive),
                    max_crap: entry.configured.max_crap.unwrap_or(resolver.global.crap),
                },
                metrics: None,
                reason: entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Complexity,
            });
        }
    }

    pub(super) fn into_states(mut self) -> Vec<fallow_output::ThresholdOverrideState> {
        self.states.sort_by(|a, b| {
            a.override_index
                .cmp(&b.override_index)
                .then(a.path.cmp(&b.path))
                .then(a.function.cmp(&b.function))
        });
        self.states
    }

    fn push_state(&mut self, input: ThresholdOverrideStateInput) {
        let status_key = match input.status {
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let key = ThresholdOverrideStateKey {
            status: status_key,
            override_index: input.override_index,
            path: input.path.clone(),
            function: input.function.clone(),
            dimension: input.dimension,
        };
        if !self.seen.insert(key) {
            return;
        }
        self.states.push(fallow_output::ThresholdOverrideState {
            status: input.status,
            override_index: input.override_index,
            path: input.path,
            function: input.function,
            configured_thresholds: input.configured_thresholds,
            effective_thresholds: input.effective_thresholds,
            metrics: input.metrics,
            reason: input.reason,
        });
    }
}

/// One function's identity (path + name) and measured complexity metrics,
/// bundled so `record_complexity` takes the function descriptor as a single
/// parameter instead of four.
#[derive(Clone, Copy)]
pub(super) struct ComplexityFunctionContext<'a> {
    pub(super) path: &'a Path,
    pub(super) function: &'a str,
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
}

struct ThresholdOverrideStateInput {
    status: fallow_output::ThresholdOverrideStatus,
    override_index: usize,
    path: Option<PathBuf>,
    function: Option<String>,
    configured_thresholds: fallow_output::HealthConfiguredThresholds,
    effective_thresholds: fallow_output::HealthEffectiveThresholds,
    metrics: Option<fallow_output::ThresholdOverrideMetrics>,
    reason: Option<String>,
    dimension: ThresholdOverrideDimension,
}
