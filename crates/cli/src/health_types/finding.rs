//! Health-finding wrappers, action context, and typed action builders.
//!
//! Three typed envelope wrappers live in this module:
//!
//! - [`HealthFinding`] flattens [`ComplexityViolation`] for the
//!   `findings[]` array and carries the typed `actions` list plus the
//!   optional audit-mode `introduced` flag.
//! - [`HotspotFinding`] flattens [`HotspotEntry`] for the
//!   `hotspots[]` array and carries a typed [`HotspotAction`] list.
//! - [`RefactoringTargetFinding`] flattens [`RefactoringTarget`] for the
//!   `targets[]` array and carries a typed [`RefactoringTargetAction`]
//!   list.
//!
//! Wire compatibility: `#[serde(flatten)]` on the inner type means every
//! array item continues to expose the inner fields at the top level
//! alongside `actions` (and `introduced` for [`HealthFinding`] only).
//! Consumers that hand-parse the JSON see no shape change.
//!
//! Audit-mode asymmetry: [`HealthFinding`] carries `introduced` because
//! `fallow audit` attributes complexity findings to the introduced /
//! inherited diff. [`HotspotFinding`] and [`RefactoringTargetFinding`] do
//! NOT carry `introduced` because hotspot ranking and refactoring targets
//! are not produced by the audit base-snapshot classifier; the pre-wrapper
//! `finding_augmentation` set `include_introduced: false` for both and the
//! typed wrappers preserve that.
//!
//! [`ComplexityViolation`]: crate::health_types::scores::ComplexityViolation

use fallow_types::output_health::{
    HealthFindingAction, HealthFindingActionType, HotspotAction, HotspotActionHeuristic,
    HotspotActionType, RefactoringTargetAction, RefactoringTargetActionType,
};
use std::ops::Deref;
use std::path::Path;

use crate::health_types::scores::{ComplexityViolation, CoverageTier, HotspotEntry};
use crate::health_types::targets::{RecommendationCategory, RefactoringTarget};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

// ────────────────────────────────────────────────────────────────────
// HotspotFinding
// ────────────────────────────────────────────────────────────────────

/// Wire envelope for a single hotspot entry.
///
/// Flattens [`HotspotEntry`] for wire continuity and adds the typed
/// `actions` list. The `#[serde(flatten)]` keeps each `hotspots[]` item
/// byte-identical to the pre-wrapper shape: inner fields (`path`,
/// `score`, `commits`, `weighted_commits`, ...) sit at the top level
/// alongside `actions`. Optional inner fields (`ownership`,
/// `is_test_path`) keep their original `skip_serializing_if` behaviour
/// because serde applies the flatten before the parent serializer runs.
///
/// Construct via [`HotspotFinding::with_actions`] in the typical health
/// pipeline (the typed action builder operates on the inner
/// [`HotspotEntry`]) or via [`HotspotFinding::from`] for fixture and
/// test code.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotFinding {
    /// Inner hotspot payload. Flattened on the wire.
    #[serde(flatten)]
    pub entry: HotspotEntry,
    /// Machine-actionable refactor and review hints. Always populated;
    /// the list never empties because the action selector unconditionally
    /// emits `refactor-file` plus `add-tests`. Ownership-derived variants
    /// (`low-bus-factor`, `unowned-hotspot`, `ownership-drift`) are
    /// appended when `--ownership` is active and the corresponding signal
    /// fires.
    pub actions: Vec<HotspotAction>,
}

impl Deref for HotspotFinding {
    type Target = HotspotEntry;

    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

impl From<HotspotEntry> for HotspotFinding {
    /// Convenience conversion: wrap a hotspot entry with an empty
    /// `actions` list. Used by tests and fixture builders. Production
    /// code should call [`HotspotFinding::with_actions`] so the wire
    /// shape carries the typed actions.
    fn from(entry: HotspotEntry) -> Self {
        Self {
            entry,
            actions: Vec::new(),
        }
    }
}

impl HotspotFinding {
    /// Construct a wrapper with the `actions` list computed from the
    /// hotspot's measured signals plus its ownership block (when
    /// present).
    ///
    /// `root` is the project root used to strip the absolute
    /// [`HotspotEntry::path`] when composing action descriptions like
    /// `"Refactor `{path}`, ..."`.
    /// The JSON post-pass that this wrapper retires ran AFTER
    /// `strip_root_prefix`, so the typed builder must apply the same
    /// stripping here for byte-identical wire output.
    #[must_use]
    pub fn with_actions(entry: HotspotEntry, root: &Path) -> Self {
        let actions = build_hotspot_actions(&entry, root);
        Self { entry, actions }
    }
}

/// Compute the typed `actions` list for a hotspot entry.
///
/// The list always begins with `refactor-file` plus `add-tests`. The
/// ownership-derived variants (`low-bus-factor`, `unowned-hotspot`,
/// `ownership-drift`) are appended when [`HotspotEntry::ownership`] is
/// present and the corresponding signal fires.
fn build_hotspot_actions(entry: &HotspotEntry, root: &Path) -> Vec<HotspotAction> {
    let relative = entry.path.strip_prefix(root).unwrap_or(&entry.path);
    // Normalise Windows backslashes to forward slashes. The retired JSON
    // post-pass read the path AFTER `strip_root_prefix` (which calls
    // `normalize_uri` to flip `\\` to `/`), so action descriptions on
    // Windows used forward slashes; the typed builder runs before
    // serialisation and must apply the same normalisation for cross-
    // platform wire parity.
    let path = relative.to_string_lossy().replace('\\', "/");

    let mut actions = vec![
        HotspotAction {
            kind: HotspotActionType::RefactorFile,
            auto_fixable: false,
            description: format!(
                "Refactor `{path}`, high complexity combined with frequent changes makes this a maintenance risk"
            ),
            note: Some(
                "Prioritize extracting complex functions, adding tests, or splitting the module"
                    .to_string(),
            ),
            suggested_pattern: None,
            heuristic: None,
        },
        HotspotAction {
            kind: HotspotActionType::AddTests,
            auto_fixable: false,
            description: format!("Add test coverage for `{path}` to reduce change risk"),
            note: Some(
                "Frequently changed complex files benefit most from comprehensive test coverage"
                    .to_string(),
            ),
            suggested_pattern: None,
            heuristic: None,
        },
    ];

    let Some(ownership) = entry.ownership.as_ref() else {
        return actions;
    };

    // Bus factor of 1 is the canonical "single point of failure" signal.
    if ownership.bus_factor == 1 {
        let top = &ownership.top_contributor;
        let owner = top.identifier.as_str();
        let commits = top.commits;
        // File-specific note: name the candidate reviewers from the
        // `suggested_reviewers` array when any exist, fall back to
        // softened framing for low-commit files, and otherwise omit
        // the note entirely (the description already carries the
        // actionable ask; adding generic boilerplate wastes tokens).
        let suggested: Vec<&str> = ownership
            .suggested_reviewers
            .iter()
            .map(|r| r.identifier.as_str())
            .collect();
        let note = if suggested.is_empty() {
            if commits < 5 {
                Some(
                    "Single recent contributor on a low-commit file. Consider a pair review for major changes."
                        .to_string(),
                )
            } else {
                // else: omit `note` entirely. The description already carries the ask.
                None
            }
        } else {
            let list = suggested
                .iter()
                .map(|s| format!("@{s}"))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("Candidate reviewers: {list}"))
        };
        actions.push(HotspotAction {
            kind: HotspotActionType::LowBusFactor,
            auto_fixable: false,
            description: format!(
                "{owner} is the sole recent contributor to `{path}`; adding a second reviewer reduces knowledge-loss risk"
            ),
            note,
            suggested_pattern: None,
            heuristic: None,
        });
    }

    // Unowned-hotspot: file matches no CODEOWNERS rule. Skip when None
    // (no CODEOWNERS file discovered) or Some(false) (a rule matches).
    if ownership.unowned == Some(true) {
        actions.push(HotspotAction {
            kind: HotspotActionType::UnownedHotspot,
            auto_fixable: false,
            description: format!("Add a CODEOWNERS entry for `{path}`"),
            note: Some(
                "Frequently-changed files without declared owners create review bottlenecks"
                    .to_string(),
            ),
            suggested_pattern: Some(suggest_codeowners_pattern(&path)),
            heuristic: Some(HotspotActionHeuristic::DirectoryDeepest),
        });
    }

    // Drift: original author no longer maintains; add a notice action so
    // agents can route the next change to the new top contributor.
    if ownership.drift {
        let reason = ownership
            .drift_reason
            .as_deref()
            .unwrap_or("ownership has shifted from the original author");
        actions.push(HotspotAction {
            kind: HotspotActionType::OwnershipDrift,
            auto_fixable: false,
            description: format!("Update CODEOWNERS for `{path}`: {reason}"),
            note: Some(
                "Drift suggests the declared or original owner is no longer the right reviewer"
                    .to_string(),
            ),
            suggested_pattern: None,
            heuristic: None,
        });
    }

    actions
}

/// Suggest a CODEOWNERS pattern for an unowned hotspot.
///
/// Picks the deepest directory containing the file
/// (e.g. `src/api/users/handlers.ts` -> `/src/api/users/`) so agents can
/// paste a tightly-scoped default. Earlier versions used the first two
/// directory levels but that catches too many siblings in monorepos
/// (`/src/api/` could span 200 files across 8 sub-domains). The deepest
/// directory keeps the suggestion reviewable while still being a directory
/// pattern rather than a per-file rule.
///
/// The action emits this alongside
/// [`HotspotActionHeuristic::DirectoryDeepest`] so consumers can branch
/// on the strategy if it evolves.
fn suggest_codeowners_pattern(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let mut components: Vec<&str> = trimmed.split('/').collect();
    components.pop(); // drop the file itself
    if components.is_empty() {
        return format!("/{trimmed}");
    }
    format!("/{}/", components.join("/"))
}

// ────────────────────────────────────────────────────────────────────
// RefactoringTargetFinding
// ────────────────────────────────────────────────────────────────────

/// Wire envelope for a single refactoring target.
///
/// Flattens [`RefactoringTarget`] for wire continuity and adds the typed
/// `actions` list. The `#[serde(flatten)]` keeps each `targets[]` item
/// byte-identical to the pre-wrapper shape: inner fields (`path`,
/// `priority`, `efficiency`, `recommendation`, `category`, ...) sit at
/// the top level alongside `actions`. Optional inner fields (`factors`,
/// `evidence`) keep their original `skip_serializing_if` behaviour.
///
/// Construct via [`RefactoringTargetFinding::with_actions`] in the
/// typical health pipeline or via [`RefactoringTargetFinding::from`] for
/// fixture and test code.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RefactoringTargetFinding {
    /// Inner refactoring target payload. Flattened on the wire.
    #[serde(flatten)]
    pub target: RefactoringTarget,
    /// Machine-actionable refactoring and suppression hints. Always
    /// populated; the list never empties because the action selector
    /// unconditionally emits `apply-refactoring`. A trailing
    /// `suppress-line` is appended only when the target carries
    /// [`RefactoringTarget::evidence`] linking to specific functions.
    pub actions: Vec<RefactoringTargetAction>,
}

impl Deref for RefactoringTargetFinding {
    type Target = RefactoringTarget;

    fn deref(&self) -> &Self::Target {
        &self.target
    }
}

impl From<RefactoringTarget> for RefactoringTargetFinding {
    /// Convenience conversion: wrap a refactoring target with an empty
    /// `actions` list. Used by tests and fixture builders. Production
    /// code should call [`RefactoringTargetFinding::with_actions`] so
    /// the wire shape carries the typed actions.
    fn from(target: RefactoringTarget) -> Self {
        Self {
            target,
            actions: Vec::new(),
        }
    }
}

impl RefactoringTargetFinding {
    /// Construct a wrapper with the `actions` list computed from the
    /// target's `recommendation`, `category`, and optional `evidence`.
    ///
    /// Asymmetry with [`HotspotFinding::with_actions`]: this constructor
    /// does NOT take a `root: &Path` because refactoring-target action
    /// descriptions never interpolate the file path; they pass
    /// [`RefactoringTarget::recommendation`] verbatim into the
    /// `apply-refactoring` action. The [`RefactoringTarget::category`]
    /// field flows into the action's `category` field as the serde
    /// snake-case form.
    #[must_use]
    pub fn with_actions(target: RefactoringTarget) -> Self {
        let actions = build_refactoring_target_actions(&target);
        Self { target, actions }
    }
}

/// Compute the typed `actions` list for a refactoring target.
///
/// The list always begins with `apply-refactoring`. A trailing
/// `suppress-line` is appended only when the target carries
/// [`RefactoringTarget::evidence`] linking to specific functions.
fn build_refactoring_target_actions(target: &RefactoringTarget) -> Vec<RefactoringTargetAction> {
    let mut actions = vec![RefactoringTargetAction {
        kind: RefactoringTargetActionType::ApplyRefactoring,
        auto_fixable: false,
        description: target.recommendation.clone(),
        category: Some(category_snake_case(&target.category).to_string()),
        comment: None,
    }];

    if target.evidence.is_some() {
        actions.push(RefactoringTargetAction {
            kind: RefactoringTargetActionType::SuppressLine,
            auto_fixable: false,
            description: "Suppress the underlying complexity finding".to_string(),
            category: None,
            comment: Some("// fallow-ignore-next-line complexity".to_string()),
        });
    }

    actions
}

/// Serde-rename_all-snake_case form of a [`RecommendationCategory`]
/// variant.
///
/// `RefactoringTargetAction.category` is `Option<String>` carrying the
/// serde-encoded form of [`RecommendationCategory`]. The JSON post-pass
/// retired by issue #408 read this string from the serialized JSON
/// value; the typed action builder needs the same form without paying
/// for a serde round-trip per target. The
/// `recommendation_category_snake_case_round_trips` test in this module
/// asserts every variant matches `serde_json::to_value` byte-for-byte,
/// so silent drift between this function and the
/// `#[serde(rename_all = "snake_case")]` attribute is caught at test
/// time.
const fn category_snake_case(cat: &RecommendationCategory) -> &'static str {
    match cat {
        RecommendationCategory::UrgentChurnComplexity => "urgent_churn_complexity",
        RecommendationCategory::BreakCircularDependency => "break_circular_dependency",
        RecommendationCategory::SplitHighImpact => "split_high_impact",
        RecommendationCategory::RemoveDeadCode => "remove_dead_code",
        RecommendationCategory::ExtractComplexFunctions => "extract_complex_functions",
        RecommendationCategory::ExtractDependencies => "extract_dependencies",
        RecommendationCategory::AddTestCoverage => "add_test_coverage",
    }
}

#[cfg(test)]
mod hotspot_target_tests {
    use super::*;
    use crate::health_types::scores::{
        ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics,
    };
    use fallow_core::churn::ChurnTrend;
    use std::path::PathBuf;

    fn sample_entry(path: &str) -> HotspotEntry {
        HotspotEntry {
            path: PathBuf::from(path),
            score: 80.0,
            commits: 12,
            weighted_commits: 8.0,
            lines_added: 100,
            lines_deleted: 40,
            complexity_density: 1.5,
            fan_in: 3,
            trend: ChurnTrend::Stable,
            ownership: None,
            is_test_path: false,
        }
    }

    fn contributor(identifier: &str, commits: u32) -> ContributorEntry {
        ContributorEntry {
            identifier: identifier.to_string(),
            format: ContributorIdentifierFormat::Handle,
            share: 1.0,
            stale_days: 1,
            commits,
        }
    }

    fn sample_target() -> RefactoringTarget {
        RefactoringTarget {
            path: PathBuf::from("/root/src/foo.ts"),
            priority: 75.0,
            efficiency: 75.0,
            recommendation: "Extract `handleRequest` into helpers".to_string(),
            category: RecommendationCategory::ExtractComplexFunctions,
            effort: crate::health_types::EffortEstimate::Low,
            confidence: crate::health_types::Confidence::High,
            factors: Vec::new(),
            evidence: None,
        }
    }

    #[test]
    fn hotspot_finding_flattens_inner_fields_at_top_level() {
        let entry = sample_entry("/root/src/api.ts");
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let json = serde_json::to_value(&finding).unwrap();
        let obj = json.as_object().unwrap();
        // Inner fields at the top level via flatten.
        assert!(obj.contains_key("score"));
        assert!(obj.contains_key("commits"));
        assert!(obj.contains_key("weighted_commits"));
        // Wrapper-only field.
        assert!(obj.contains_key("actions"));
        // Optional inner fields with skip_serializing_if respect their attrs.
        assert!(!obj.contains_key("ownership"));
        assert!(!obj.contains_key("is_test_path"));
    }

    #[test]
    fn hotspot_actions_default_pair_when_ownership_absent() {
        let entry = sample_entry("/root/src/api.ts");
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(finding.actions[0].kind, HotspotActionType::RefactorFile);
        assert_eq!(finding.actions[1].kind, HotspotActionType::AddTests);
        assert!(finding.actions[0].description.contains("src/api.ts"));
    }

    #[test]
    fn hotspot_low_bus_factor_with_suggested_reviewers_lists_them() {
        let mut entry = sample_entry("/root/src/api.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 1,
            contributor_count: 1,
            top_contributor: contributor("alice", 30),
            recent_contributors: Vec::new(),
            suggested_reviewers: vec![contributor("bob", 4), contributor("carol", 2)],
            declared_owner: None,
            unowned: None,
            drift: false,
            drift_reason: None,
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let low_bus = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::LowBusFactor)
            .expect("low-bus-factor action present");
        assert_eq!(
            low_bus.note.as_deref(),
            Some("Candidate reviewers: @bob, @carol"),
        );
    }

    #[test]
    fn hotspot_low_bus_factor_softens_for_low_commit_files() {
        let mut entry = sample_entry("/root/src/api.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 1,
            contributor_count: 1,
            top_contributor: contributor("alice", 3),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: None,
            drift: false,
            drift_reason: None,
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let low_bus = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::LowBusFactor)
            .expect("low-bus-factor action present");
        assert_eq!(
            low_bus.note.as_deref(),
            Some(
                "Single recent contributor on a low-commit file. Consider a pair review for major changes.",
            ),
        );
    }

    #[test]
    fn hotspot_low_bus_factor_omits_note_for_high_commit_no_reviewers() {
        let mut entry = sample_entry("/root/src/api.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 1,
            contributor_count: 1,
            top_contributor: contributor("alice", 50),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: None,
            drift: false,
            drift_reason: None,
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let low_bus = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::LowBusFactor)
            .expect("low-bus-factor action present");
        assert!(low_bus.note.is_none());
    }

    #[test]
    fn hotspot_unowned_action_carries_deepest_directory_pattern() {
        let mut entry = sample_entry("/root/src/api/users/handlers.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 2,
            contributor_count: 3,
            top_contributor: contributor("alice", 10),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: Some(true),
            drift: false,
            drift_reason: None,
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let unowned = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::UnownedHotspot)
            .expect("unowned-hotspot action present");
        assert_eq!(
            unowned.suggested_pattern.as_deref(),
            Some("/src/api/users/")
        );
        assert_eq!(
            unowned.heuristic,
            Some(HotspotActionHeuristic::DirectoryDeepest)
        );
    }

    #[test]
    fn hotspot_action_descriptions_normalise_windows_separators() {
        // Cross-platform parity: the retired JSON post-pass read the path
        // AFTER `strip_root_prefix` (which normalises backslashes via
        // `normalize_uri`). The typed builder runs before serialisation
        // and must apply the same normalisation, otherwise action
        // descriptions on Windows would contain `src\api.ts` while macOS
        // / Linux see `src/api.ts`. Simulating the Windows shape by
        // constructing a path with embedded backslashes is cross-platform
        // safe because `Path` on Unix treats the entire literal as one
        // component (no `strip_prefix` match) and the builder falls
        // back to the input path, which the `replace('\\', "/")` then
        // normalises to forward slashes for description embedding.
        let mut entry = sample_entry("src\\api\\users.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 2,
            contributor_count: 3,
            top_contributor: contributor("alice", 10),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: Some(true),
            drift: false,
            drift_reason: None,
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let refactor = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::RefactorFile)
            .expect("refactor-file action present");
        assert!(refactor.description.contains("src/api/users.ts"));
        assert!(!refactor.description.contains('\\'));
        let unowned = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::UnownedHotspot)
            .expect("unowned-hotspot action present");
        assert_eq!(unowned.suggested_pattern.as_deref(), Some("/src/api/"));
    }

    #[test]
    fn hotspot_drift_action_uses_provided_reason() {
        let mut entry = sample_entry("/root/src/api.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 2,
            contributor_count: 4,
            top_contributor: contributor("alice", 10),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: Some(false),
            drift: true,
            drift_reason: Some("top contributor changed in last 6 months".to_string()),
        });
        let finding = HotspotFinding::with_actions(entry, Path::new("/root"));
        let drift = finding
            .actions
            .iter()
            .find(|a| a.kind == HotspotActionType::OwnershipDrift)
            .expect("ownership-drift action present");
        assert!(
            drift
                .description
                .contains("top contributor changed in last 6 months"),
        );
    }

    #[test]
    fn refactoring_target_finding_flattens_inner_fields_at_top_level() {
        let target = sample_target();
        let finding = RefactoringTargetFinding::with_actions(target);
        let json = serde_json::to_value(&finding).unwrap();
        let obj = json.as_object().unwrap();
        // Inner fields at the top level via flatten.
        assert!(obj.contains_key("priority"));
        assert!(obj.contains_key("efficiency"));
        assert!(obj.contains_key("recommendation"));
        assert!(obj.contains_key("category"));
        // Wrapper-only field.
        assert!(obj.contains_key("actions"));
        // Optional inner fields skipped when empty / None.
        assert!(!obj.contains_key("factors"));
        assert!(!obj.contains_key("evidence"));
    }

    #[test]
    fn refactoring_target_actions_default_to_apply_only_without_evidence() {
        let target = sample_target();
        let finding = RefactoringTargetFinding::with_actions(target);
        assert_eq!(finding.actions.len(), 1);
        assert_eq!(
            finding.actions[0].kind,
            RefactoringTargetActionType::ApplyRefactoring,
        );
        assert_eq!(
            finding.actions[0].category.as_deref(),
            Some("extract_complex_functions"),
        );
        assert_eq!(
            finding.actions[0].description,
            "Extract `handleRequest` into helpers",
        );
    }

    #[test]
    fn refactoring_target_actions_append_suppress_when_evidence_present() {
        let mut target = sample_target();
        target.evidence = Some(crate::health_types::TargetEvidence {
            unused_exports: Vec::new(),
            complex_functions: vec![crate::health_types::EvidenceFunction {
                name: "handleRequest".to_string(),
                line: 12,
                cognitive: 30,
            }],
            cycle_path: Vec::new(),
        });
        let finding = RefactoringTargetFinding::with_actions(target);
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(
            finding.actions[1].kind,
            RefactoringTargetActionType::SuppressLine,
        );
        assert_eq!(
            finding.actions[1].comment.as_deref(),
            Some("// fallow-ignore-next-line complexity"),
        );
    }

    #[test]
    fn codeowners_pattern_uses_deepest_directory() {
        // Deepest dir keeps the suggestion tightly-scoped; the prior
        // "first two levels" heuristic over-generalized in monorepos.
        assert_eq!(
            suggest_codeowners_pattern("src/api/users/handlers.ts"),
            "/src/api/users/",
        );
    }

    #[test]
    fn codeowners_pattern_for_root_file() {
        assert_eq!(suggest_codeowners_pattern("README.md"), "/README.md");
    }

    #[test]
    fn codeowners_pattern_normalizes_backslashes() {
        assert_eq!(
            suggest_codeowners_pattern("src\\api\\users.ts"),
            "/src/api/",
        );
    }

    #[test]
    fn codeowners_pattern_two_level_path() {
        assert_eq!(suggest_codeowners_pattern("src/foo.ts"), "/src/");
    }

    #[test]
    fn recommendation_category_snake_case_round_trips_through_serde() {
        // Hard gate against drift between `category_snake_case` and the
        // `#[serde(rename_all = "snake_case")]` attribute on
        // `RecommendationCategory`. If a future contributor adds a new
        // variant and forgets to extend the match, this test will fail
        // because `serde_json::to_value` will emit one form and the
        // hand-rolled mapper will emit another.
        let variants = [
            RecommendationCategory::UrgentChurnComplexity,
            RecommendationCategory::BreakCircularDependency,
            RecommendationCategory::SplitHighImpact,
            RecommendationCategory::RemoveDeadCode,
            RecommendationCategory::ExtractComplexFunctions,
            RecommendationCategory::ExtractDependencies,
            RecommendationCategory::AddTestCoverage,
        ];
        for cat in &variants {
            let via_serde = serde_json::to_value(cat).unwrap();
            let serde_str = via_serde.as_str().unwrap();
            assert_eq!(
                serde_str,
                category_snake_case(cat),
                "category_snake_case for {cat:?} drifted from serde rename_all",
            );
        }
    }
}
