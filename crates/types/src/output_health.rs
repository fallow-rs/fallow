//! Per-action types attached to each health finding by the JSON output
//! layer.
//!
//! These types are the typed wire shape for the `actions[]` array on health
//! findings, hotspots, refactoring targets, and coverage-gap entries. The
//! JSON emission path constructs them through typed wrappers (for example
//! `UntestedFileFinding` in `crates/cli/src/health_types/coverage.rs`) and
//! serializes them via serde; the schemars derive renders the matching
//! per-action shape in `docs/output-schema.json`.
//!
//! Whenever a new action variant or optional field is added, update the
//! matching type here so the drift gate flags the divergence before review.

use serde::Serialize;

/// Suggested action attached to a [`ComplexityViolation`].
///
/// Each complexity finding carries an array of these on the JSON wire
/// (`findings[].actions[]`). The action selector in
/// `crates/cli/src/report/json.rs::build_health_finding_actions` picks the
/// primary action based on which thresholds triggered the finding and the
/// bucketed coverage tier. See [`HealthFindingActionType`] for the full
/// discriminant list.
///
/// `note`, `comment`, and `placement` are populated per-variant: refactor
/// actions carry a `note`, suppress-line / suppress-file actions carry
/// `comment` plus `placement`, and the coverage-leaning actions
/// (`add-tests`, `increase-coverage`) carry only `note`.
///
/// [`ComplexityViolation`]: ../../fallow-cli/src/health_types/scores.rs
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthFindingAction {
    /// Action type identifier. A single finding's `actions` array can carry
    /// MULTIPLE entries of different types: e.g., a finding that exceeded
    /// both cyclomatic and CRAP at `coverage_tier`: partial will get BOTH
    /// `increase-coverage` AND `refactor-function`, plus `suppress-line`.
    /// Consumers that select a single action should treat the FIRST
    /// non-`suppress-{line,file}` action as primary. `add-tests` is emitted
    /// when CRAP triggered the finding, the function has no test coverage
    /// (`coverage_tier`: none), and full coverage can bring CRAP below
    /// `max_crap_threshold` (cyclomatic < threshold, since CRAP bottoms out
    /// at CC at 100% coverage). `increase-coverage` is emitted when CRAP
    /// triggered the finding, some coverage exists (`coverage_tier`: partial
    /// or high), and full coverage can bring CRAP below `max_crap_threshold`;
    /// the description steers toward targeted branch coverage rather than
    /// scaffolding new tests. `refactor-function` is emitted when
    /// cyclomatic/cognitive triggered the finding, when full coverage still
    /// cannot bring CRAP below `max_crap_threshold` (cyclomatic >=
    /// threshold), or as a secondary action when cyclomatic is within 5 of
    /// the cyclomatic threshold AND cognitive is at or above
    /// `max_cognitive_threshold / 2` (the cognitive floor suppresses false
    /// positives on flat type-tag dispatchers and JSX render maps where
    /// high cyclomatic comes from a single switch with near-zero cognitive
    /// load). `suppress-file` is emitted instead of `suppress-line` for
    /// synthetic Angular `<template>` findings on `.html` files, because
    /// line-suppression comments cannot be expressed in HTML; the `comment`
    /// field carries `<!-- fallow-ignore-file complexity -->` and
    /// `placement` is `top-of-template`.
    #[serde(rename = "type")]
    pub kind: HealthFindingActionType,
    /// Whether `fallow fix` can auto-apply this action. Today every health
    /// finding action is manual, but the field is non-singleton so a future
    /// auto-applier (e.g., an LLM-driven `refactor-function` worker) does
    /// not need a schema change.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context (e.g., the canonical CRAP formula, or a hint
    /// about which branch type to extract). Present on most action types;
    /// dropped only when the description carries the full ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The inline comment to insert (e.g.,
    /// `// fallow-ignore-next-line complexity` or
    /// `<!-- fallow-ignore-file complexity -->`). Present on
    /// `suppress-line` and `suppress-file` action variants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Where to insert the suppress comment
    /// (e.g., `above-function-declaration`, `above-angular-decorator`,
    /// `above-component-worst-method`, or `top-of-template`). Present on
    /// `suppress-line` and `suppress-file` action variants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    /// Project-relative path the action should target when the finding's
    /// remediation lives in a different file from where the finding is
    /// anchored. Currently populated on the `increase-coverage` action for
    /// synthetic Angular `<template>` findings whose CRAP is inherited from
    /// the owning `.component.ts`: the action points at the component file
    /// (where the user actually adds tests) rather than the `.html` template
    /// (where the finding is anchored but which is not directly testable).
    /// Absent when the action's target is the finding's own file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
}

/// Discriminant for [`HealthFindingAction::kind`]. Mirrors the action types
/// emitted by `build_health_finding_actions`. A single finding's `actions`
/// array may carry multiple entries of different types: a finding that
/// exceeded both cyclomatic and CRAP at `coverage_tier: partial` will get
/// BOTH `increase-coverage` AND `refactor-function`, plus the trailing
/// `suppress-line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HealthFindingActionType {
    /// Refactor the function to reduce complexity. Emitted when
    /// cyclomatic/cognitive triggered the finding, when full coverage
    /// still cannot bring CRAP below `max_crap_threshold`, or as a
    /// secondary action when cyclomatic is within 5 of the cyclomatic
    /// threshold AND cognitive is at or above the cognitive floor.
    RefactorFunction,
    /// Add tests for a CRAP-triggered finding whose coverage tier is
    /// `none` (no test path reaches the function).
    AddTests,
    /// Increase test coverage for a CRAP-triggered finding whose coverage
    /// tier is `partial` or `high` (some test path exists; add targeted
    /// assertions for uncovered branches).
    IncreaseCoverage,
    /// Suppress with an HTML comment at the top of the template. Used for
    /// synthetic Angular `<template>` findings on `.html` files where a
    /// line suppression cannot be expressed.
    SuppressFile,
    /// Suppress with an inline `// fallow-ignore-next-line complexity`
    /// comment above the function or Angular decorator.
    SuppressLine,
}

/// Suggested action attached to a [`HotspotEntry`].
///
/// The action list always begins with `refactor-file` plus `add-tests`.
/// Ownership-derived variants (`low-bus-factor`, `unowned-hotspot`,
/// `ownership-drift`) are appended only when `--ownership` is active AND
/// the corresponding signal fires for the hotspot.
///
/// [`HotspotEntry`]: ../../fallow-cli/src/health_types/scores.rs
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HotspotAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: HotspotActionType,
    /// Whether `fallow fix` can auto-apply this action. Today every
    /// hotspot action is manual.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context for the action. Absent on `low-bus-factor` when
    /// the finding's description already carries the full ask (no
    /// suggested reviewers and not a low-commit file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Suggested CODEOWNERS pattern. Present only on `unowned-hotspot`
    /// actions. Derived per the [`heuristic`](Self::heuristic) field;
    /// consumers should branch on [`heuristic`](Self::heuristic) rather
    /// than assume a stable algorithm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_pattern: Option<String>,
    /// Strategy used to derive [`suggested_pattern`](Self::suggested_pattern).
    /// Reserved for future evolution (`codeowners-cluster`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heuristic: Option<HotspotActionHeuristic>,
}

/// Discriminant for [`HotspotAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HotspotActionType {
    /// Refactor the hotspot file (high complexity plus frequent change).
    RefactorFile,
    /// Add test coverage to reduce change risk on the hotspot file.
    AddTests,
    /// Bus factor of 1: a single recent contributor owns the file.
    /// Emitted only with `--ownership`.
    LowBusFactor,
    /// Hotspot matches no CODEOWNERS rule (a rules file exists but no
    /// pattern matches). Emitted only with `--ownership`.
    UnownedHotspot,
    /// Ownership has drifted from the original author to a new top
    /// contributor. Emitted only with `--ownership`.
    OwnershipDrift,
}

/// Strategy discriminant for the suggested CODEOWNERS pattern attached to
/// an `unowned-hotspot` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HotspotActionHeuristic {
    /// Suggest the deepest directory containing the file (e.g.,
    /// `/src/api/users/`). Keeps the suggestion reviewable while staying
    /// a directory pattern rather than a per-file rule.
    DirectoryDeepest,
}

/// Suggested action attached to a [`RefactoringTarget`].
///
/// The list always begins with `apply-refactoring`. A trailing
/// `suppress-line` is appended only when the target carries `evidence`
/// linking to specific functions (e.g., `extract_complex_functions`,
/// `add_test_coverage`).
///
/// Unlike [`HealthFindingAction`], the `suppress-line` variant emitted
/// here does NOT carry a `placement` field: the parent
/// [`RefactoringTarget`] points at a file (not a specific function
/// declaration site), so a per-line placement hint would have no
/// referent. Consumers that want the placement metadata should follow
/// the target's `evidence.complex_functions` back to the matching
/// `ComplexityViolation` and read placement from THAT action instead.
///
/// [`RefactoringTarget`]: ../../fallow-cli/src/health_types/targets.rs
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RefactoringTargetAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: RefactoringTargetActionType,
    /// Whether `fallow fix` can auto-apply this action. Today both
    /// variants are manual.
    pub auto_fixable: bool,
    /// Human-readable description of the action. For `apply-refactoring`
    /// this is the target's own `recommendation` string; for
    /// `suppress-line` it is the suppression prompt.
    pub description: String,
    /// Recommendation category for `apply-refactoring` actions. Mirrors
    /// the parent target's
    /// [`category`](../../fallow-cli/src/health_types/targets.rs.html)
    /// field so consumers can route on the action alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The inline comment to insert. Present on `suppress-line` actions
    /// when evidence exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`RefactoringTargetAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RefactoringTargetActionType {
    /// Apply the recommended refactoring (extract, split, decouple, etc.).
    ApplyRefactoring,
    /// Suppress the underlying complexity finding with an inline comment.
    SuppressLine,
}

/// Suggested action attached to an [`UntestedFile`] coverage-gap finding.
///
/// `build_untested_file_actions` emits a two-entry array on every
/// untested-file item: an `add-tests` primary action (scaffold tests for
/// the runtime file) and a `suppress-file` action
/// (`// fallow-ignore-file coverage-gaps`). Both variants share the same
/// struct shape; the field that is populated (`note` for `add-tests`,
/// `comment` for `suppress-file`) depends on the `kind`.
///
/// [`UntestedFile`]: ../../fallow-cli/src/health_types/coverage.rs
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedFileAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: UntestedFileActionType,
    /// Whether `fallow fix` can auto-apply this action. Today both
    /// variants are manual.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context for the `add-tests` variant (explains why no
    /// test path reaches this file). Absent on `suppress-file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The file-level comment to insert. Present on `suppress-file`
    /// (`// fallow-ignore-file coverage-gaps`). Absent on `add-tests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`UntestedFileAction::kind`]. Mirrors the action types
/// emitted by `build_untested_file_actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum UntestedFileActionType {
    /// Scaffold tests that exercise the runtime file.
    AddTests,
    /// Suppress coverage-gap reporting for this file with a file-level
    /// comment.
    SuppressFile,
}

/// Suggested action attached to an [`UntestedExport`] coverage-gap
/// finding.
///
/// `build_untested_export_actions` emits a two-entry array on every
/// untested-export item: an `add-test-import` primary action (import the
/// export from a test-reachable module) and a `suppress-file` action
/// (`// fallow-ignore-file coverage-gaps`). The export-specific variant
/// `add-test-import` reflects that a test-reachable reference chain, not
/// just any test coverage, is what closes the gap.
///
/// [`UntestedExport`]: ../../fallow-cli/src/health_types/coverage.rs
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedExportAction {
    /// Action type identifier.
    #[serde(rename = "type")]
    pub kind: UntestedExportActionType,
    /// Whether `fallow fix` can auto-apply this action. Today both
    /// variants are manual.
    pub auto_fixable: bool,
    /// Human-readable description of the action.
    pub description: String,
    /// Additional context for the `add-test-import` variant (explains the
    /// runtime-reachable / test-unreachable asymmetry). Absent on
    /// `suppress-file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The file-level comment to insert. Present on `suppress-file`
    /// (`// fallow-ignore-file coverage-gaps`). Absent on
    /// `add-test-import`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Discriminant for [`UntestedExportAction::kind`]. Mirrors the action
/// types emitted by `build_untested_export_actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum UntestedExportActionType {
    /// Import and exercise the export from a test-reachable module.
    AddTestImport,
    /// Suppress coverage-gap reporting for the export's file with a
    /// file-level comment.
    SuppressFile,
}
