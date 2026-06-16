//! Health-finding wrappers, action context, and typed action builders.
//!
//! This module keeps the wire envelopes typed while preserving the existing
//! flattened JSON shape.

use fallow_types::output_health::{
    HealthFindingAction, HealthFindingActionType, HotspotAction, HotspotActionHeuristic,
    HotspotActionType, RefactoringTargetAction, RefactoringTargetActionType,
};
use std::ops::Deref;
use std::path::Path;

use crate::health_types::scores::{
    ComplexityViolation, CoverageTier, HotspotEntry, OwnershipState,
};
use crate::health_types::targets::{RecommendationCategory, RefactoringTarget};

/// Options controlling how the action builder populates `actions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HealthActionOptions {
    /// Skip `suppress-line` action entries.
    pub omit_suppress_line: bool,
    /// Reason surfaced in `actions_meta` when `omit_suppress_line` is true.
    pub omit_reason: Option<&'static str>,
}

/// Construction-time context for [`HealthFinding::with_actions`].
#[derive(Debug, Clone, Copy)]
pub struct HealthActionContext {
    /// Action-emission options.
    pub opts: HealthActionOptions,
    /// Cyclomatic-complexity ceiling.
    pub max_cyclomatic_threshold: u16,
    /// Cognitive-complexity ceiling.
    pub max_cognitive_threshold: u16,
    /// CRAP ceiling.
    pub max_crap_threshold: f64,
    /// Band below `max_cyclomatic_threshold` where a CRAP-only finding also
    /// gets a secondary `refactor-function` action.
    pub crap_refactor_band: u16,
}

/// Wire envelope for a single complexity finding.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthFinding {
    /// Inner complexity-violation payload.
    #[serde(flatten)]
    pub violation: ComplexityViolation,
    /// Machine-actionable fix and suppress hints.
    pub actions: Vec<HealthFindingAction>,
    /// Audit-mode flag indicating whether the finding is new versus the base
    /// snapshot.
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
    /// Wrap a violation with empty actions and no `introduced` flag.
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

    /// Construct a wrapper with `actions` computed from the finding and
    /// report-wide context.
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
    let max_cyclomatic_threshold = violation
        .effective_thresholds
        .map_or(ctx.max_cyclomatic_threshold, |thresholds| {
            thresholds.max_cyclomatic
        });
    let max_cognitive_threshold = violation
        .effective_thresholds
        .map_or(ctx.max_cognitive_threshold, |thresholds| {
            thresholds.max_cognitive
        });
    let max_crap_threshold = violation
        .effective_thresholds
        .map_or(ctx.max_crap_threshold, |thresholds| thresholds.max_crap);
    let full_coverage_can_clear_crap = !includes_crap || f64::from(cyclomatic) < max_crap_threshold;

    let mut actions: Vec<HealthFindingAction> = Vec::new();

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

    let is_template = name == "<template>";
    let is_component = name == "<component>";
    if should_add_refactor_action(
        crap_only,
        full_coverage_can_clear_crap,
        cyclomatic,
        cognitive,
        max_cyclomatic_threshold,
        max_cognitive_threshold,
        ctx,
    ) {
        actions.push(build_refactor_action(
            violation,
            name,
            is_template,
            is_component,
        ));
    }

    if !ctx.opts.omit_suppress_line {
        actions.push(build_suppress_action(violation, is_template, is_component));
    }

    actions
}

fn should_add_refactor_action(
    crap_only: bool,
    full_coverage_can_clear_crap: bool,
    cyclomatic: u16,
    cognitive: u16,
    max_cyclomatic_threshold: u16,
    max_cognitive_threshold: u16,
    ctx: &HealthActionContext,
) -> bool {
    let crap_only_needs_complexity_reduction = crap_only && !full_coverage_can_clear_crap;
    let cognitive_floor = max_cognitive_threshold / 2;
    let near_cyclomatic_threshold = crap_only
        && cyclomatic > 0
        && cyclomatic >= max_cyclomatic_threshold.saturating_sub(ctx.crap_refactor_band)
        && cognitive >= cognitive_floor;
    !crap_only || crap_only_needs_complexity_reduction || near_cyclomatic_threshold
}

fn build_refactor_action(
    violation: &ComplexityViolation,
    name: &str,
    is_template: bool,
    is_component: bool,
) -> HealthFindingAction {
    let (description, note): (String, &str) = if is_component {
        component_refactor_copy(violation)
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
    HealthFindingAction {
        kind: HealthFindingActionType::RefactorFunction,
        auto_fixable: false,
        description,
        note: Some(note.to_string()),
        comment: None,
        placement: None,
        target_path: None,
    }
}

fn component_refactor_copy(violation: &ComplexityViolation) -> (String, &'static str) {
    let rollup = violation.component_rollup.as_ref();
    let class_name = rollup.map_or("the component", |r| r.component.as_str());
    let worst_method = rollup.map_or("the worst class method", |r| {
        r.class_worst_function.as_str()
    });
    let class_cyc = rollup.map_or(0_u16, |r| r.class_cyclomatic);
    let template_cyc = rollup.map_or(0_u16, |r| r.template_cyclomatic);
    (
        format!(
            "Refactor `{class_name}` to reduce component complexity (rolled-up cyclomatic {} = {class_cyc} on `{worst_method}` + {template_cyc} on the template)",
            violation.cyclomatic
        ),
        "Consider splitting the template into smaller components OR extracting helpers from the worst class method; the rollup reflects the component as one complexity unit",
    )
}

fn build_suppress_action(
    violation: &ComplexityViolation,
    is_template: bool,
    is_component: bool,
) -> HealthFindingAction {
    if is_template
        && violation
            .path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
    {
        return suppress_file_action(
            "Suppress with an HTML comment at the top of the template",
            "<!-- fallow-ignore-file complexity -->",
            "top-of-template",
        );
    }
    if is_template {
        return suppress_line_action(
            "Suppress with an inline comment above the Angular decorator",
            "above-angular-decorator",
        );
    }
    if is_component {
        return suppress_line_action(
            "Suppress with an inline comment above the worst class method (the rollup is anchored at that method's line, so a comment above it hides both the function finding and the rollup)",
            "above-component-worst-method",
        );
    }
    suppress_line_action(
        "Suppress with an inline comment above the function declaration",
        "above-function-declaration",
    )
}

fn suppress_file_action(description: &str, comment: &str, placement: &str) -> HealthFindingAction {
    HealthFindingAction {
        kind: HealthFindingActionType::SuppressFile,
        auto_fixable: false,
        description: description.to_string(),
        note: None,
        comment: Some(comment.to_string()),
        placement: Some(placement.to_string()),
        target_path: None,
    }
}

fn suppress_line_action(description: &str, placement: &str) -> HealthFindingAction {
    HealthFindingAction {
        kind: HealthFindingActionType::SuppressLine,
        auto_fixable: false,
        description: description.to_string(),
        note: None,
        comment: Some("// fallow-ignore-next-line complexity".to_string()),
        placement: Some(placement.to_string()),
        target_path: None,
    }
}

/// Build the coverage-leaning action for a CRAP-contributing finding.
fn build_crap_coverage_action(
    name: &str,
    tier: Option<CoverageTier>,
    full_coverage_can_clear_crap: bool,
    inherited_from: Option<&Path>,
) -> Option<HealthFindingAction> {
    if !full_coverage_can_clear_crap {
        return None;
    }

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
    let path = relative.to_string_lossy().replace('\\', "/");
    let mut actions = base_hotspot_actions(&path);
    if let Some(ownership) = entry.ownership.as_ref() {
        append_ownership_hotspot_actions(&mut actions, ownership, &path);
    }
    actions
}

fn base_hotspot_actions(path: &str) -> Vec<HotspotAction> {
    vec![
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
    ]
}

fn append_ownership_hotspot_actions(
    actions: &mut Vec<HotspotAction>,
    ownership: &crate::health_types::OwnershipMetrics,
    path: &str,
) {
    if ownership.bus_factor == 1 {
        let top = &ownership.top_contributor;
        let owner = top.identifier.as_str();
        let commits = top.commits;
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

    if ownership.unowned == Some(true) {
        actions.push(HotspotAction {
            kind: HotspotActionType::UnownedHotspot,
            auto_fixable: false,
            description: format!("Add a CODEOWNERS entry for `{path}`"),
            note: Some(
                "Frequently-changed files without declared owners create review bottlenecks"
                    .to_string(),
            ),
            suggested_pattern: Some(suggest_codeowners_pattern(path)),
            heuristic: Some(HotspotActionHeuristic::DirectoryDeepest),
        });
    }

    if ownership.ownership_state == OwnershipState::Drifting && ownership.drift {
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
        ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics, OwnershipState,
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
        assert!(obj.contains_key("score"));
        assert!(obj.contains_key("commits"));
        assert!(obj.contains_key("weighted_commits"));
        assert!(obj.contains_key("actions"));
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
            ownership_state: OwnershipState::Active,
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
            ownership_state: OwnershipState::Active,
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
            ownership_state: OwnershipState::Active,
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
            ownership_state: OwnershipState::Unowned,
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
        let mut entry = sample_entry("src\\api\\users.ts");
        entry.ownership = Some(OwnershipMetrics {
            bus_factor: 2,
            contributor_count: 3,
            top_contributor: contributor("alice", 10),
            recent_contributors: Vec::new(),
            suggested_reviewers: Vec::new(),
            declared_owner: None,
            unowned: Some(true),
            ownership_state: OwnershipState::Unowned,
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
            ownership_state: OwnershipState::Drifting,
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
        assert!(obj.contains_key("priority"));
        assert!(obj.contains_key("efficiency"));
        assert!(obj.contains_key("recommendation"));
        assert!(obj.contains_key("category"));
        assert!(obj.contains_key("actions"));
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
            ..Default::default()
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
