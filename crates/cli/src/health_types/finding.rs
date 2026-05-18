//! Health-finding wrapper, action context, and typed action builder.
//!
//! The [`HealthFinding`] envelope flattens a [`ComplexityViolation`] payload
//! and adds the typed `actions` list and the audit-mode `introduced` flag
//! natively, so the JSON output layer no longer needs the
//! `inject_health_actions` post-pass to patch actions into the serialized
//! tree.
//!
//! Wire compatibility: `#[serde(flatten)]` on the inner violation means
//! `findings[]` items continue to expose the inner fields at the top level
//! alongside `actions` + `introduced`. Consumers that hand-parse the JSON
//! see no shape change.
//!
//! [`ComplexityViolation`]: crate::health_types::scores::ComplexityViolation

use fallow_types::output_health::{HealthFindingAction, HealthFindingActionType};
use std::ops::Deref;
use std::path::Path;

use crate::health_types::scores::{ComplexityViolation, CoverageTier};

/// Cyclomatic distance from `max_cyclomatic_threshold` at which a
/// CRAP-only finding still warrants a secondary `refactor-function` action.
///
/// Reasoning: a function whose cyclomatic count is within this band of the
/// configured threshold is "almost too complex" already, so refactoring is a
/// useful complement to the primary coverage action. Keeping the boundary
/// expressed as a band (threshold minus N) rather than a ratio links it
/// to the existing `health.maxCyclomatic` knob: tightening the threshold
/// automatically widens the population that gets the secondary suggestion.
const SECONDARY_REFACTOR_BAND: u16 = 5;

/// Options controlling how the action builder populates a health finding's
/// `actions` array.
///
/// `omit_suppress_line` skips the `suppress-line` action across every
/// health finding. Set when:
/// - A baseline is active (`opts.baseline.is_some()` or
///   `opts.save_baseline.is_some()`): the baseline file already suppresses
///   findings, and adding `// fallow-ignore-next-line` comments on top
///   creates dead annotations once the baseline regenerates.
/// - The team has opted out via `health.suggestInlineSuppression: false`.
///
/// When omitted, a top-level `actions_meta` object on the report records
/// the omission and the reason so consumers can audit "where did
/// health finding suppress-line go?" without having to grep the config
/// or CLI history. Wire shape is documented by
/// [`crate::health_types::HealthActionsMeta`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HealthActionOptions {
    /// Skip emission of `suppress-line` action entries.
    pub omit_suppress_line: bool,
    /// Human-readable reason surfaced in the `actions_meta` breadcrumb when
    /// `omit_suppress_line` is true. Stable codes:
    /// - `"baseline-active"`: `--baseline` or `--save-baseline` was passed
    /// - `"config-disabled"`: `health.suggestInlineSuppression: false`
    pub omit_reason: Option<&'static str>,
}

/// Construction-time context for [`HealthFinding::with_actions`].
///
/// Bundles the action-emission options and the complexity thresholds the
/// action selector needs. Computed once per `HealthReport` build (or once
/// per group when `--group-by` partitions the run) and reused across every
/// finding so the action list is byte-for-byte equivalent to the prior
/// `inject_health_actions` post-pass output.
#[derive(Debug, Clone, Copy)]
pub struct HealthActionContext {
    /// Action-emission options (suppress-line gating + audit reason).
    pub opts: HealthActionOptions,
    /// Cyclomatic-complexity ceiling beyond which a function is flagged.
    /// Sourced from `summary.max_cyclomatic_threshold`.
    pub max_cyclomatic_threshold: u16,
    /// Cognitive-complexity ceiling. Sourced from
    /// `summary.max_cognitive_threshold`.
    pub max_cognitive_threshold: u16,
    /// CRAP ceiling. Sourced from `summary.max_crap_threshold`.
    pub max_crap_threshold: f64,
}

/// Wire envelope for a single complexity finding.
///
/// Flattens [`ComplexityViolation`] for wire continuity and adds the typed
/// `actions` list plus the audit-mode `introduced` flag. The
/// `#[serde(flatten)]` keeps each `findings[]` item byte-identical to the
/// pre-wrapper shape: inner fields (`path`, `name`, `line`, `cyclomatic`,
/// ...) sit at the top level alongside `actions` and optional `introduced`.
///
/// Construct via [`HealthFinding::with_actions`] in the typical health
/// pipeline (the wrapper computes its own `actions` from a
/// [`HealthActionContext`]) or via [`HealthFinding::new`] when the caller
/// already has the action list (e.g., tests, audit cross-attribution).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthFinding {
    /// Inner complexity-violation payload. Flattened on the wire.
    #[serde(flatten)]
    pub violation: ComplexityViolation,
    /// Machine-actionable fix and suppress hints. Always populated; never
    /// empty in the typical pipeline (the action selector emits at least
    /// `suppress-line` or `suppress-file` unless suppressed by the
    /// context).
    pub actions: Vec<HealthFindingAction>,
    /// Audit-mode flag indicating whether the finding is new versus the
    /// audit base snapshot. `Some(true)` when introduced in the diff,
    /// `Some(false)` when present in both snapshots, `None` outside audit
    /// mode (the field is skipped from the wire).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introduced: Option<bool>,
}

impl Deref for HealthFinding {
    type Target = ComplexityViolation;

    fn deref(&self) -> &Self::Target {
        &self.violation
    }
}

impl From<ComplexityViolation> for HealthFinding {
    /// Convenience conversion: wrap a violation with an empty `actions`
    /// list and no `introduced` flag. Used by tests and fixture builders
    /// that don't exercise the action-selection path. Production code
    /// should call [`HealthFinding::with_actions`] (or
    /// [`HealthFinding::new`] when the action list is already computed)
    /// so the wire shape carries the typed actions.
    fn from(violation: ComplexityViolation) -> Self {
        Self {
            violation,
            actions: Vec::new(),
            introduced: None,
        }
    }
}

impl HealthFinding {
    /// Construct a wrapper around a pre-computed action list.
    ///
    /// Used by audit cross-attribution paths and tests where the caller
    /// already has the actions in hand. Prefer [`Self::with_actions`] in
    /// the typical pipeline.
    #[must_use]
    #[allow(
        dead_code,
        reason = "intentional public constructor for audit / test paths that supply their own actions; with_actions is the production constructor"
    )]
    pub fn new(
        violation: ComplexityViolation,
        actions: Vec<HealthFindingAction>,
        introduced: Option<bool>,
    ) -> Self {
        Self {
            violation,
            actions,
            introduced,
        }
    }

    /// Construct a wrapper with the `actions` list computed from the
    /// finding's measured signals plus the report-wide context.
    ///
    /// The `introduced` field is left at `None`; audit-mode callers set it
    /// after construction once base-snapshot attribution runs.
    #[must_use]
    pub fn with_actions(violation: ComplexityViolation, ctx: &HealthActionContext) -> Self {
        let actions = build_health_finding_actions(&violation, ctx);
        Self {
            violation,
            actions,
            introduced: None,
        }
    }
}

/// Compute the typed `actions` list for a complexity finding.
///
/// Selection rules:
///
/// - Exceeded cyclomatic/cognitive only (no CRAP): `refactor-function`.
/// - Exceeded CRAP, tier `none` or absent: `add-tests` (no test path
///   reaches this function; start from scratch).
/// - Exceeded CRAP, tier `partial`/`high`: `increase-coverage` (file
///   already has some test path; add targeted assertions for uncovered
///   branches).
/// - Exceeded CRAP, full coverage cannot clear CRAP: `refactor-function`
///   because reducing cyclomatic complexity is the remaining lever.
/// - Exceeded both CRAP and cyclomatic/cognitive: emit BOTH the
///   tier-appropriate coverage action AND `refactor-function`.
/// - CRAP-only with cyclomatic within `SECONDARY_REFACTOR_BAND` of the
///   threshold AND cognitive past the cognitive floor: also append
///   `refactor-function` as a secondary action; the function is
///   "almost too complex" already.
///
/// A trailing `suppress-line` (or `suppress-file` for Angular `.html`
/// templates) is appended unless `ctx.opts.omit_suppress_line` is true.
#[must_use]
pub fn build_health_finding_actions(
    violation: &ComplexityViolation,
    ctx: &HealthActionContext,
) -> Vec<HealthFindingAction> {
    let name = violation.name.as_str();
    let exceeded = violation.exceeded;
    let includes_crap = exceeded.includes_crap();
    let crap_only = matches!(exceeded, crate::health_types::ExceededThreshold::Crap);
    let cyclomatic = violation.cyclomatic;
    let cognitive = violation.cognitive;
    let full_coverage_can_clear_crap =
        !includes_crap || f64::from(cyclomatic) < ctx.max_crap_threshold;

    let mut actions: Vec<HealthFindingAction> = Vec::new();

    // Coverage-leaning action: only emitted when CRAP contributed. For
    // synthetic <template> findings whose CRAP was inherited from the
    // owning .component.ts via the inverse templateUrl edge, the action
    // description must point AI agents at the component file rather than
    // the .html template, otherwise agents will hallucinate Angular
    // template test harnesses or try to scaffold a spec for the .html
    // path directly (which is structurally impossible). The inherited_from
    // string is the project-relative .ts path emitted alongside the
    // coverage_source discriminator.
    let inherited_from = violation.inherited_from.as_deref();
    if includes_crap
        && let Some(action) = build_crap_coverage_action(
            name,
            violation.coverage_tier,
            full_coverage_can_clear_crap,
            inherited_from,
        )
    {
        actions.push(action);
    }

    // Refactor action conditions:
    //   1. Exceeded cyclomatic/cognitive (with or without CRAP), or
    //   2. CRAP-only where even full coverage cannot bring CRAP below the
    //      configured threshold, so reducing complexity is the remaining
    //      lever, or
    //   3. CRAP-only with cyclomatic within SECONDARY_REFACTOR_BAND of the
    //      threshold AND cognitive complexity past the cognitive floor (the
    //      function is almost too complex anyway and the cognitive signal
    //      confirms that refactoring would actually help). Without the
    //      cognitive floor, flat type-tag dispatchers and JSX render maps
    //      (high CC, near-zero cog) get a misleading refactor suggestion.
    //
    // `build_crap_coverage_action` returns `None` for case 2 instead of
    // pushing `refactor-function` itself, so this branch unconditionally
    // pushes the refactor entry without needing to dedupe.
    let crap_only_needs_complexity_reduction = crap_only && !full_coverage_can_clear_crap;
    let cognitive_floor = ctx.max_cognitive_threshold / 2;
    let near_cyclomatic_threshold = crap_only
        && cyclomatic > 0
        && cyclomatic
            >= ctx
                .max_cyclomatic_threshold
                .saturating_sub(SECONDARY_REFACTOR_BAND)
        && cognitive >= cognitive_floor;
    let is_template = name == "<template>";
    let is_component = name == "<component>";
    if !crap_only || crap_only_needs_complexity_reduction || near_cyclomatic_threshold {
        let (description, note): (String, &str) = if is_component {
            // Component rollup: name is the literal "<component>"; the
            // breakdown lives in `component_rollup`. Direct AI agents at the
            // component as the unit so they consider splitting the template
            // OR refactoring the worst class method, not just one of them.
            let rollup = violation.component_rollup.as_ref();
            let class_name = rollup.map_or("the component", |r| r.component.as_str());
            let worst_method = rollup.map_or("the worst class method", |r| {
                r.class_worst_function.as_str()
            });
            let class_cyc = rollup.map_or(0_u16, |r| r.class_cyclomatic);
            let template_cyc = rollup.map_or(0_u16, |r| r.template_cyclomatic);
            (
                format!(
                    "Refactor `{class_name}` to reduce component complexity (rolled-up cyclomatic {cyclomatic} = {class_cyc} on `{worst_method}` + {template_cyc} on the template)"
                ),
                "Consider splitting the template into smaller components OR extracting helpers from the worst class method; the rollup reflects the component as one complexity unit",
            )
        } else if is_template {
            (
                format!(
                    "Refactor `{name}` to reduce template complexity (simplify control flow and bindings)"
                ),
                "Consider splitting complex template branches into smaller components or simpler bindings",
            )
        } else {
            (
                format!(
                    "Refactor `{name}` to reduce complexity (extract helper functions, simplify branching)"
                ),
                "Consider splitting into smaller functions with single responsibilities",
            )
        };
        actions.push(HealthFindingAction {
            kind: HealthFindingActionType::RefactorFunction,
            auto_fixable: false,
            description,
            note: Some(note.to_string()),
            comment: None,
            placement: None,
            target_path: None,
        });
    }

    if !ctx.opts.omit_suppress_line {
        if is_template
            && violation
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        {
            actions.push(HealthFindingAction {
                kind: HealthFindingActionType::SuppressFile,
                auto_fixable: false,
                description: "Suppress with an HTML comment at the top of the template".to_string(),
                note: None,
                comment: Some("<!-- fallow-ignore-file complexity -->".to_string()),
                placement: Some("top-of-template".to_string()),
                target_path: None,
            });
        } else if is_template {
            actions.push(HealthFindingAction {
                kind: HealthFindingActionType::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the Angular decorator"
                    .to_string(),
                note: None,
                comment: Some("// fallow-ignore-next-line complexity".to_string()),
                placement: Some("above-angular-decorator".to_string()),
                target_path: None,
            });
        } else if is_component {
            // Rollup anchors at the worst class function's line; the same
            // suppression that hides the worst function also hides the
            // rollup, but the description tells the user which line it
            // lands on so they don't expect the comment above the
            // @Component decorator (which would NOT match the rollup's line).
            actions.push(HealthFindingAction {
                kind: HealthFindingActionType::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the worst class method (the rollup is anchored at that method's line, so a comment above it hides both the function finding and the rollup)".to_string(),
                note: None,
                comment: Some("// fallow-ignore-next-line complexity".to_string()),
                placement: Some("above-component-worst-method".to_string()),
                target_path: None,
            });
        } else {
            actions.push(HealthFindingAction {
                kind: HealthFindingActionType::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the function declaration"
                    .to_string(),
                note: None,
                comment: Some("// fallow-ignore-next-line complexity".to_string()),
                placement: Some("above-function-declaration".to_string()),
                target_path: None,
            });
        }
    }

    actions
}

/// Build the coverage-leaning action for a CRAP-contributing finding.
///
/// Returns `None` when even 100% coverage could not bring the function
/// below the configured CRAP threshold. In that case the primary action
/// becomes `refactor-function`, which the caller emits separately.
fn build_crap_coverage_action(
    name: &str,
    tier: Option<CoverageTier>,
    full_coverage_can_clear_crap: bool,
    inherited_from: Option<&Path>,
) -> Option<HealthFindingAction> {
    if !full_coverage_can_clear_crap {
        return None;
    }

    // Inherited-coverage path: when the CRAP score on a `<template>`
    // finding was derived from the owning Angular component .ts file, the
    // test surface to act on is the component, not the .html. Override
    // the description so agents do not try to scaffold tests against the
    // template path directly.
    if let Some(owner) = inherited_from {
        let owner_str = owner.to_string_lossy().into_owned();
        return Some(HealthFindingAction {
            kind: HealthFindingActionType::IncreaseCoverage,
            auto_fixable: false,
            description: format!(
                "Increase test coverage on `{owner_str}` (the CRAP score on `{name}` is inherited from this Angular component; add component tests there rather than against the template)"
            ),
            note: Some(
                "CRAP = CC^2 * (1 - cov/100)^3 + CC; .html templates are exercised through their @Component class, so the test target is the .ts file referenced by `inherited_from`".to_string(),
            ),
            comment: None,
            placement: None,
            target_path: Some(owner_str),
        });
    }

    match tier {
        // Partial / high coverage: the file already has some test path.
        // Pivot the action description from "add tests" to "increase
        // coverage" so agents add targeted assertions for uncovered
        // branches instead of scaffolding new tests from scratch.
        Some(CoverageTier::Partial | CoverageTier::High) => Some(HealthFindingAction {
            kind: HealthFindingActionType::IncreaseCoverage,
            auto_fixable: false,
            description: format!(
                "Increase test coverage for `{name}` (file is reachable from existing tests; add targeted assertions for uncovered branches)"
            ),
            note: Some(
                "CRAP = CC^2 * (1 - cov/100)^3 + CC; targeted branch coverage is more efficient than scaffolding new test files when the file already has coverage".to_string(),
            ),
            comment: None,
            placement: None,
            target_path: None,
        }),
        // None / unknown tier: keep the original "add-tests" message.
        _ => Some(HealthFindingAction {
            kind: HealthFindingActionType::AddTests,
            auto_fixable: false,
            description: format!(
                "Add test coverage for `{name}` to lower its CRAP score (coverage reduces risk even without refactoring)"
            ),
            note: Some(
                "CRAP = CC^2 * (1 - cov/100)^3 + CC; higher coverage is the fastest way to bring CRAP under threshold".to_string(),
            ),
            comment: None,
            placement: None,
            target_path: None,
        }),
    }
}
