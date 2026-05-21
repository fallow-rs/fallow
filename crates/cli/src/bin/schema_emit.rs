#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "schema-emit binary prints the regenerated schema to stdout and errors to stderr"
)]

//! Regenerate `docs/output-schema.json` from the Rust source of truth.
//!
//! Built only when the `schema-emit` cargo feature is active. Pulls
//! `schemars::JsonSchema` derives off the result and duplication types and
//! prints a draft-07 JSON Schema document to stdout.
//!
//! Usage:
//! ```bash
//! cargo run -p fallow-cli --features schema-emit --bin fallow-schema-emit \
//!     > docs/output-schema.json
//! ```
//!
//! Today this emits only the `definitions` block that can be derived from the
//! in-scope structs (`AnalysisResults`, all per-finding types in
//! `crates/types/src/results.rs`, `DuplicationReport` and friends in
//! `crates/core/src/duplicates/types.rs`). Hand-written sections of
//! `docs/output-schema.json` (the top-level `oneOf`, envelopes such as
//! `CheckOutput` / `DupesOutput` / `HealthOutput`, audit/explain/coverage/
//! codeclimate/review envelopes, and the health subtree) are merged in from
//! the committed file so the emitted document stays a drop-in replacement
//! while subsequent migration phases tackle them.

#[cfg(not(test))]
use std::path::PathBuf;
use std::process::ExitCode;

use schemars::generate::SchemaSettings;
use serde_json::{Map, Value};

use fallow_cli::health_types::{
    ComplexityViolation, ContributorEntry, ContributorIdentifierFormat, CoverageGapSummary,
    CoverageGaps, CoverageModel, CoverageTier, ExceededThreshold, FileHealthScore, FindingSeverity,
    HealthActionsMeta, HealthScore, HealthScorePenalties, HealthSummary, HealthTrend, HotspotEntry,
    HotspotFinding, HotspotSummary, LargeFunctionEntry, OwnershipMetrics, RecommendationCategory,
    RefactoringTarget, RefactoringTargetFinding, RiskProfile, RuntimeCoverageReport,
    TargetThresholds, TrendCount, UntestedExport, UntestedExportFinding, UntestedFile,
    UntestedFileFinding, VitalSigns, VitalSignsCounts,
};
use fallow_cli::output_dupes::{
    AttributedCloneGroupFinding, CloneFamilyAction, CloneFamilyActionType, CloneFamilyFinding,
    CloneGroupAction, CloneGroupActionType, CloneGroupFinding, DupesReportPayload,
};
use fallow_cli::output_envelope::{
    AuditCommand, AuditOutput, BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone,
    BoundariesListing, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CodeClimateIssue,
    CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation, CodeClimateOutput,
    CodeClimateSeverity, CombinedOutput, CoverageAnalyzeOutput, CoverageAnalyzeSchemaVersion,
    CoverageSetupFileToEdit, CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput,
    CoverageSetupPackageManager, CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion,
    CoverageSetupSnippet, DupesOutput, ExplainOutput, FallowOutput, GitHubReviewComment,
    GitHubReviewSide, GitLabReviewComment, GitLabReviewPosition, GitLabReviewPositionType,
    GroupByMode, HealthOutput, ListBoundariesOutput, ReviewCheckConclusion, ReviewComment,
    ReviewEnvelopeEvent, ReviewEnvelopeMeta, ReviewEnvelopeOutput, ReviewEnvelopeSchema,
    ReviewEnvelopeSummary, ReviewProvider, ReviewReconcileOutput, ReviewReconcileSchema,
};
use fallow_cli::report::dupes_grouping::{
    AttributedCloneGroup, AttributedInstance, DuplicationGroup,
};
use fallow_config::{AuthoredRule, LogicalGroup, LogicalGroupStatus};
use fallow_core::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats, MirroredDirectory,
    RefactoringKind, RefactoringSuggestion,
};
use fallow_types::envelope::{
    AuditIntroduced, BaselineCategoryDelta, BaselineDeltas, BaselineMatch, CheckSummary, ElapsedMs,
    EntryPoints, Meta, MetaMetric, MetaRule, RegressionResult, RegressionStatus,
    RegressionToleranceKind, SchemaVersion, ToolVersion,
};
use fallow_types::extract::MemberKind;
use fallow_types::output::{
    AddToConfigAction, AddToConfigKind, AddToConfigValue, FixAction, FixActionType,
    IgnoreExportsRule, IssueAction, SuppressFileAction, SuppressFileKind, SuppressLineAction,
    SuppressLineKind, SuppressLineScope,
};
use fallow_types::output_dead_code::{
    BoundaryViolationFinding, CircularDependencyFinding, PrivateTypeLeakFinding,
    TestOnlyDependencyFinding, TypeOnlyDependencyFinding, UnlistedDependencyFinding,
    UnresolvedImportFinding, UnusedClassMemberFinding, UnusedDependencyFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedOptionalDependencyFinding, UnusedTypeFinding,
};
use fallow_types::output_health::{
    HealthFindingAction, HealthFindingActionType, HotspotAction, HotspotActionHeuristic,
    HotspotActionType, RefactoringTargetAction, RefactoringTargetActionType, UntestedExportAction,
    UntestedExportActionType, UntestedFileAction, UntestedFileActionType,
};
use fallow_types::results::{
    AnalysisResults, BoundaryViolation, CircularDependency, DependencyLocation,
    DependencyOverrideMisconfigReason, DependencyOverrideSource, DuplicateExport,
    DuplicateLocation, EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag,
    FlagConfidence, FlagKind, ImportSite, MisconfiguredDependencyOverride, PrivateTypeLeak,
    ReferenceLocation, StaleSuppression, SuppressionOrigin, TestOnlyDependency, TypeOnlyDependency,
    UnlistedDependency, UnresolvedCatalogReference, UnresolvedImport, UnusedCatalogEntry,
    UnusedDependency, UnusedDependencyOverride, UnusedExport, UnusedFile, UnusedMember,
};

/// Workspace-relative path to the committed schema. Read at runtime against
/// the workspace root so the published `fallow-cli` crate does not need to
/// bundle `docs/output-schema.json` (which lives outside the cli crate's
/// own directory). Only used by the production code path; tests use the
/// embedded copy below.
#[cfg(not(test))]
const COMMITTED_SCHEMA_REL_PATH: &str = "docs/output-schema.json";

/// Embedded copy used by `#[cfg(test)] mod drift_tests`. Tests run with
/// `CARGO_MANIFEST_DIR = crates/cli`, so the runtime resolver below would
/// have to walk the workspace; the embedded copy is simpler and only ships
/// in test builds.
#[cfg(test)]
const COMMITTED_SCHEMA: &str = include_str!("../../../../docs/output-schema.json");

/// Locate `docs/output-schema.json` by walking up from `CARGO_MANIFEST_DIR`
/// (or the current working directory) until a parent contains the file.
/// Returns the full file contents.
#[cfg(not(test))]
fn read_committed_schema() -> Result<String, String> {
    let start = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "unable to determine starting directory".to_string())?;
    for dir in start.ancestors() {
        let candidate = dir.join(COMMITTED_SCHEMA_REL_PATH);
        if candidate.is_file() {
            return std::fs::read_to_string(&candidate)
                .map_err(|err| format!("failed to read {}: {err}", candidate.display()));
        }
    }
    Err(format!(
        "could not find {COMMITTED_SCHEMA_REL_PATH} by walking up from {}; run the binary from the workspace root",
        start.display()
    ))
}

/// Test-only helper that uses the embedded schema rather than the
/// filesystem, keeping the drift tests fast and independent of working
/// directory. The `Result` wrap mirrors the non-test signature so callers
/// stay agnostic of which path is active.
#[cfg(test)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "signature must match the non-test variant's `Result<String, String>` return"
)]
fn committed_schema_source() -> Result<String, String> {
    Ok(COMMITTED_SCHEMA.to_string())
}

#[cfg(not(test))]
fn committed_schema_source() -> Result<String, String> {
    read_committed_schema()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fallow-schema-emit: {err}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let derived = derived_definitions();
    let merged = merge_with_committed(&derived)?;
    let pretty = serde_json::to_string_pretty(&merged)
        .map_err(|err| format!("failed to serialize merged schema: {err}"))?;
    println!("{pretty}");
    Ok(())
}

/// Names of the definitions that this binary owns (regenerated from Rust).
/// Anything not in this set is copied verbatim from the committed schema.
///
/// As migration phases land (health subtree, envelopes), entries move from
/// the committed-only set into this list, until eventually `merge_with_committed`
/// can be replaced by a pure derive-and-emit flow.
pub(crate) fn derived_definition_names() -> &'static [&'static str] {
    // The list below is intentionally narrower than the full set of types with
    // `JsonSchema` derives. It contains only types that have a SEPARATE,
    // matching definition in `docs/output-schema.json#/definitions/` today.
    //
    // Types whose Rust definition is inlined into a parent's schema (enums
    // like `DependencyLocation`, `MemberKind`, `RefactoringKind`,
    // `SuppressionOrigin`, ...) are intentionally excluded because there is
    // nothing to drift-check against. A follow-up that extracts inline enums
    // into separate `definitions/` entries can grow this list.
    //
    // Types that are LSP-internal (`ExportUsage`, `ReferenceLocation`) or
    // shipped via a separate output (feature flags) are also excluded; they
    // are not part of the public JSON output contract today.
    &[
        // crates/types/src/results.rs - per-finding structs
        "BoundaryViolation",
        "CircularDependency",
        "DuplicateExport",
        "DuplicateLocation",
        "EmptyCatalogGroup",
        "ImportSite",
        "MisconfiguredDependencyOverride",
        "PrivateTypeLeak",
        "StaleSuppression",
        "TestOnlyDependency",
        "TypeOnlyDependency",
        "UnlistedDependency",
        "UnresolvedCatalogReference",
        "UnresolvedImport",
        "UnusedCatalogEntry",
        "UnusedDependency",
        "UnusedDependencyOverride",
        "UnusedExport",
        "UnusedFile",
        "UnusedMember",
        // crates/core/src/duplicates/types.rs - per-finding clone structs
        "CloneFamily",
        "CloneGroup",
        "CloneInstance",
        "MirroredDirectory",
        // crates/types/src/output.rs - JSON-layer augmentations
        "AddToConfigAction",
        "FixAction",
        "IssueAction",
        "SuppressFileAction",
        "SuppressLineAction",
        // crates/cli/src/health_types/ - health output subtree.
        // `HealthFinding` is the typed wrapper introduced in #384 B2 that
        // flattens `ComplexityViolation` and carries the typed `actions`
        // list plus the optional audit-mode `introduced` flag natively.
        // `ComplexityViolation` is the inner payload; both definitions
        // ship in `docs/output-schema.json` since the wrapper's
        // `#[serde(flatten)]` keeps the on-the-wire shape compatible with
        // pre-wrapper consumers that read the inner fields at the top
        // level of each `findings[]` item.
        "ComplexityViolation",
        "ContributorEntry",
        "CoverageGapSummary",
        "CoverageGaps",
        "FileHealthScore",
        "HealthActionsMeta",
        "HealthFinding",
        "HealthScore",
        "HealthScorePenalties",
        "HealthSummary",
        "HealthTrend",
        "HotspotEntry",
        "HotspotFinding",
        "HotspotSummary",
        "LargeFunctionEntry",
        "OwnershipMetrics",
        "RefactoringTarget",
        "RefactoringTargetFinding",
        "RiskProfile",
        "RuntimeCoverageReport",
        "TargetThresholds",
        "TrendCount",
        "UntestedExport",
        "UntestedExportFinding",
        "UntestedFile",
        "UntestedFileFinding",
        "VitalSigns",
        "VitalSignsCounts",
        // crates/types/src/output_health.rs - per-finding action wrappers
        "HealthFindingAction",
        "HotspotAction",
        "RefactoringTargetAction",
        "UntestedExportAction",
        "UntestedFileAction",
        // crates/types/src/envelope.rs - shared envelope / utility shapes.
        // Scalar utility newtypes (SchemaVersion / ToolVersion / ElapsedMs /
        // AuditIntroduced) have no properties to drift-check; they are
        // registered so refs from envelopes resolve and so future shape
        // tightening (range constraints, enum variants) flows through the
        // gate.
        "AuditIntroduced",
        "BaselineDeltas",
        "BaselineMatch",
        "CheckSummary",
        "ElapsedMs",
        "EntryPoints",
        "Meta",
        "RegressionResult",
        "SchemaVersion",
        "ToolVersion",
        // crates/cli/src/health_types/runtime_coverage.rs - per-finding
        // helpers + enums emitted as separate definitions in the
        // committed schema. The full subtree is drift-checked so a
        // future Rust field change in a helper fires the gate.
        "RuntimeCoverageAction",
        "RuntimeCoverageBlastRadiusEntry",
        "RuntimeCoverageCaptureQuality",
        "RuntimeCoverageConfidence",
        "RuntimeCoverageEvidence",
        "RuntimeCoverageFinding",
        "RuntimeCoverageHotPath",
        "RuntimeCoverageImportanceEntry",
        "RuntimeCoverageMessage",
        "RuntimeCoverageReportVerdict",
        "RuntimeCoverageRiskBand",
        "RuntimeCoverageSignal",
        "RuntimeCoverageSummary",
        "RuntimeCoverageVerdict",
        "RuntimeCoverageWatermark",
        // Bare body shapes referenced from CombinedOutput / AuditOutput
        // for the sub-results where the wire emits the body without
        // envelope-header wrapping. Drift-checking them here forces the
        // committed `$ref`s on the parent envelopes to resolve against the
        // same shape the wire produces.
        "DuplicationReport",
        "HealthReport",
        // crates/cli/src/output_envelope.rs - per-command envelope structs.
        "AuditOutput",
        "CheckGroupedEntry",
        "CheckGroupedOutput",
        "CheckOutput",
        "CodeClimateIssue",
        "CodeClimateOutput",
        "CombinedOutput",
        "CoverageSetupFileToEdit",
        "CoverageSetupMember",
        "CoverageSetupOutput",
        "CoverageSetupSnippet",
        "DupesOutput",
        "ExplainOutput",
        "GitHubReviewComment",
        "GitLabReviewComment",
        "GitLabReviewPosition",
        "HealthGroup",
        "HealthOutput",
        "ReviewEnvelopeOutput",
        "ReviewEnvelopeSummary",
        "ReviewReconcileOutput",
        // crates/cli/src/output_envelope.rs - typed document root that
        // wraps the 11 object-shaped envelopes via `#[serde(untagged)]`.
        // Drives the schema's document-root `oneOf` (see
        // `rewrite_document_root_one_of` in `merge_with_committed`); the
        // committed schema's root therefore becomes a derived artifact.
        "FallowOutput",
        // crates/cli/src/output_envelope.rs - list --boundaries envelope
        // and building blocks (issue #373).
        "BoundariesListLogicalGroup",
        "BoundariesListRule",
        "BoundariesListZone",
        "BoundariesListing",
        "ListBoundariesOutput",
        // crates/config/src/config/boundaries.rs - referenced by
        // BoundariesListLogicalGroup and also surfaced on the resolved
        // boundary config for in-process consumers.
        "AuthoredRule",
        "LogicalGroup",
        "LogicalGroupStatus",
        // crates/cli/src/report/dupes_grouping.rs - per-group duplication
        // attribution payload (`fallow dupes --group-by`).
        "AttributedCloneGroup",
        "AttributedInstance",
        "DuplicationGroup",
        // crates/cli/src/output_dupes.rs - typed duplication wrappers
        // introduced in #409 (PR C of the #384 ladder). Each wraps the
        // matching bare finding via `#[serde(flatten)]` and carries the
        // typed `actions[]` array (plus optional `introduced` audit flag
        // on the top-level CloneGroupFinding) natively, retiring the
        // legacy `inject_dupes_actions` post-pass.
        "AttributedCloneGroupFinding",
        "CloneFamilyAction",
        "CloneFamilyActionType",
        "CloneFamilyFinding",
        "CloneGroupAction",
        "CloneGroupActionType",
        "CloneGroupFinding",
        "DupesReportPayload",
        // crates/cli/src/output_envelope.rs - typed CoverageAnalyzeOutput
        // root envelope introduced in #410 (PR D of the #384 ladder).
        // Replaces the hand-built `serde_json::json!` macro in
        // `crates/cli/src/coverage/analyze.rs::print_runtime_json` and
        // joins `FallowOutput` as a sibling object variant.
        "CoverageAnalyzeOutput",
        "CoverageAnalyzeSchemaVersion",
    ]
}

/// Names of finding-type definitions that the JSON output layer wraps with
/// the `actions` array plus the optional `introduced` flag. The schema gets
/// these properties appended after derivation so the public contract stays
/// in lock-step with what `crates/cli/src/report/json.rs` actually emits.
///
/// New finding types added in `crates/types/src/results.rs` must also be
/// added here, otherwise the emitted schema will under-document the JSON
/// output and the drift test will flag the missing entry.
///
/// `augment_finding_definition` unconditionally pushes `"actions"` into the
/// per-finding `required` array. The runtime always emits `actions: [...]`
/// (possibly empty) on every finding, so requiring the field on the wire is
/// honest. The previous "augmentation is non-opinionated" stance was a
/// pre-Phase-8 escape hatch that documented some finding types as having
/// optional `actions` while emitting them; it is retired.
fn finding_definition_names() -> &'static [&'static str] {
    // Every finding family has now been migrated to typed `*Finding` wrappers
    // (in `crates/types/src/output_dead_code.rs`, `crates/cli/src/health_types/finding.rs`,
    // or `crates/cli/src/output_dupes.rs`); the wrappers flatten the bare
    // finding via `#[serde(flatten)]` and carry the typed `actions[]` (plus
    // optional `introduced`) array natively via schemars. No definition
    // requires the legacy `augment_finding_definition` post-graft anymore.
    //
    // Kept as a function returning an empty slice (rather than a const) so
    // adding a future hand-augmented finding requires the same one-liner
    // edit, and the in-test scaffolding (`augment_finding_definition`,
    // `FindingAugmentation`, `finding_augmentation`) stays in place ready
    // for the rare case it is needed again.
    &[]
}

/// Per-finding override for `augment_finding_definition`.
///
/// The default augmentation attaches `actions: array<IssueAction>` and an
/// `introduced` audit-mode flag. Health findings (`HealthFinding`,
/// `HotspotFinding`, `RefactoringTargetFinding`) are no longer augmented
/// because they became typed wrappers in #384 B2 and B3 that flatten
/// their respective inner payloads and carry typed `actions` (plus
/// `introduced` for `HealthFinding` only) natively via schemars.
#[derive(Debug, Clone, Copy)]
struct FindingAugmentation {
    /// Schema `$ref` for the items in the `actions` array.
    actions_item_ref: &'static str,
    /// Whether to attach the optional `introduced` audit breadcrumb.
    include_introduced: bool,
}

/// Augmentation applied to dead-code findings: actions ref `IssueAction`,
/// `introduced` flag attached.
const DEFAULT_FINDING_AUGMENTATION: FindingAugmentation = FindingAugmentation {
    actions_item_ref: "#/definitions/IssueAction",
    include_introduced: true,
};

/// Pick the augmentation for a specific finding. Every finding family has
/// migrated to typed `*Finding` wrappers (most recently the duplication
/// family in #409 and the standalone coverage envelope in #410); no
/// definition currently routes through here. The function stays in place
/// so a future hand-augmented finding can be wired with a single arm.
fn finding_augmentation(_name: &str) -> FindingAugmentation {
    DEFAULT_FINDING_AUGMENTATION
}

/// Build derived schemas for every in-scope type using one shared generator.
///
/// Registering each type as a subschema (rather than a root schema) collects
/// every transitively-referenced definition into a single map keyed by the
/// Rust type name, which we then merge into the schema's `definitions`.
#[allow(
    clippy::too_many_lines,
    reason = "this function is fundamentally a registration list: one `subschema_for::<T>()` call per type in the public output contract. Splitting by module obscures the registration set; the linear list is the cleanest representation."
)]
fn derived_definitions() -> Map<String, Value> {
    let mut generator = SchemaSettings::draft07().into_generator();

    // Trigger registration of every in-scope type. Return values are discarded
    // because we only want the side effect of populating the generator's
    // definitions table. AnalysisResults pulls in every per-finding type
    // transitively, and DuplicationReport pulls in every clone-detection
    // type, so a small set of top-level subschema calls covers all leaves.
    let _ = generator.subschema_for::<AnalysisResults>();
    let _ = generator.subschema_for::<DuplicationReport>();

    // Belt-and-braces: register every type by name to guarantee its presence
    // even if a future refactor stops referencing it from the top-level
    // containers. Cheap (no-op for already-registered types) and keeps the
    // derived set predictable for the drift test.
    let _ = generator.subschema_for::<UnusedFile>();
    let _ = generator.subschema_for::<UnusedExport>();
    let _ = generator.subschema_for::<PrivateTypeLeak>();
    let _ = generator.subschema_for::<UnusedDependency>();
    let _ = generator.subschema_for::<DependencyLocation>();
    let _ = generator.subschema_for::<UnusedMember>();
    let _ = generator.subschema_for::<UnresolvedImport>();
    let _ = generator.subschema_for::<UnlistedDependency>();
    let _ = generator.subschema_for::<ImportSite>();
    let _ = generator.subschema_for::<DuplicateExport>();
    let _ = generator.subschema_for::<DuplicateLocation>();
    let _ = generator.subschema_for::<TypeOnlyDependency>();
    let _ = generator.subschema_for::<UnusedCatalogEntry>();
    let _ = generator.subschema_for::<EmptyCatalogGroup>();
    let _ = generator.subschema_for::<UnresolvedCatalogReference>();
    let _ = generator.subschema_for::<DependencyOverrideSource>();
    let _ = generator.subschema_for::<UnusedDependencyOverride>();
    let _ = generator.subschema_for::<DependencyOverrideMisconfigReason>();
    let _ = generator.subschema_for::<MisconfiguredDependencyOverride>();
    let _ = generator.subschema_for::<TestOnlyDependency>();
    let _ = generator.subschema_for::<CircularDependency>();
    let _ = generator.subschema_for::<BoundaryViolation>();
    let _ = generator.subschema_for::<SuppressionOrigin>();
    let _ = generator.subschema_for::<StaleSuppression>();
    let _ = generator.subschema_for::<FlagKind>();
    let _ = generator.subschema_for::<FlagConfidence>();
    let _ = generator.subschema_for::<FeatureFlag>();
    let _ = generator.subschema_for::<ExportUsage>();
    let _ = generator.subschema_for::<ReferenceLocation>();
    let _ = generator.subschema_for::<EntryPointSummary>();
    let _ = generator.subschema_for::<MemberKind>();
    let _ = generator.subschema_for::<CloneInstance>();
    let _ = generator.subschema_for::<CloneGroup>();
    let _ = generator.subschema_for::<RefactoringKind>();
    let _ = generator.subschema_for::<RefactoringSuggestion>();
    let _ = generator.subschema_for::<CloneFamily>();
    let _ = generator.subschema_for::<MirroredDirectory>();

    // Per-group duplication attribution (crates/cli/src/report/dupes_grouping.rs).
    let _ = generator.subschema_for::<AttributedInstance>();
    let _ = generator.subschema_for::<AttributedCloneGroup>();
    let _ = generator.subschema_for::<DuplicationGroup>();
    let _ = generator.subschema_for::<DuplicationStats>();

    // Typed duplication wrappers (crates/cli/src/output_dupes.rs).
    // Each wraps a bare clone finding via `#[serde(flatten)]` and carries
    // a typed `actions[]` array natively, retiring the legacy
    // `inject_dupes_actions` post-pass in `crates/cli/src/report/json.rs`
    // (#409 / PR C of the #384 ladder).
    let _ = generator.subschema_for::<CloneGroupFinding>();
    let _ = generator.subschema_for::<CloneFamilyFinding>();
    let _ = generator.subschema_for::<AttributedCloneGroupFinding>();
    let _ = generator.subschema_for::<CloneGroupAction>();
    let _ = generator.subschema_for::<CloneGroupActionType>();
    let _ = generator.subschema_for::<CloneFamilyAction>();
    let _ = generator.subschema_for::<CloneFamilyActionType>();
    let _ = generator.subschema_for::<DupesReportPayload>();

    // JSON-output augmentation types from `crates/types/src/output.rs`.
    let _ = generator.subschema_for::<IssueAction>();
    let _ = generator.subschema_for::<FixAction>();
    let _ = generator.subschema_for::<FixActionType>();
    let _ = generator.subschema_for::<SuppressLineAction>();
    let _ = generator.subschema_for::<SuppressLineKind>();
    let _ = generator.subschema_for::<SuppressLineScope>();
    let _ = generator.subschema_for::<SuppressFileAction>();
    let _ = generator.subschema_for::<SuppressFileKind>();
    let _ = generator.subschema_for::<AddToConfigAction>();
    let _ = generator.subschema_for::<AddToConfigKind>();
    let _ = generator.subschema_for::<AddToConfigValue>();
    let _ = generator.subschema_for::<IgnoreExportsRule>();

    // Typed dead-code finding wrappers from
    // `crates/types/src/output_dead_code.rs`. Each wraps a bare finding via
    // `#[serde(flatten)]` and carries a typed `actions` array natively,
    // retiring the per-finding `augment_finding_definition` graft.
    let _ = generator.subschema_for::<UnusedFileFinding>();
    let _ = generator.subschema_for::<PrivateTypeLeakFinding>();
    let _ = generator.subschema_for::<UnresolvedImportFinding>();
    let _ = generator.subschema_for::<CircularDependencyFinding>();
    let _ = generator.subschema_for::<BoundaryViolationFinding>();
    let _ = generator.subschema_for::<UnusedExportFinding>();
    let _ = generator.subschema_for::<UnusedTypeFinding>();
    let _ = generator.subschema_for::<UnusedEnumMemberFinding>();
    let _ = generator.subschema_for::<UnusedClassMemberFinding>();
    let _ = generator.subschema_for::<UnusedDependencyFinding>();
    let _ = generator.subschema_for::<UnusedDevDependencyFinding>();
    let _ = generator.subschema_for::<UnusedOptionalDependencyFinding>();
    let _ = generator.subschema_for::<UnlistedDependencyFinding>();
    let _ = generator.subschema_for::<TypeOnlyDependencyFinding>();
    let _ = generator.subschema_for::<TestOnlyDependencyFinding>();

    // Health output subtree (crates/cli/src/health_types/).
    let _ = generator.subschema_for::<HealthSummary>();
    let _ = generator.subschema_for::<ComplexityViolation>();
    let _ = generator.subschema_for::<ExceededThreshold>();
    let _ = generator.subschema_for::<FindingSeverity>();
    let _ = generator.subschema_for::<CoverageTier>();
    let _ = generator.subschema_for::<CoverageModel>();
    let _ = generator.subschema_for::<LargeFunctionEntry>();
    let _ = generator.subschema_for::<FileHealthScore>();
    let _ = generator.subschema_for::<HotspotEntry>();
    let _ = generator.subschema_for::<HotspotFinding>();
    let _ = generator.subschema_for::<HotspotSummary>();
    let _ = generator.subschema_for::<OwnershipMetrics>();
    let _ = generator.subschema_for::<ContributorEntry>();
    let _ = generator.subschema_for::<ContributorIdentifierFormat>();
    let _ = generator.subschema_for::<RefactoringTarget>();
    let _ = generator.subschema_for::<RefactoringTargetFinding>();
    let _ = generator.subschema_for::<RecommendationCategory>();
    let _ = generator.subschema_for::<TargetThresholds>();
    let _ = generator.subschema_for::<HealthTrend>();
    let _ = generator.subschema_for::<TrendCount>();
    let _ = generator.subschema_for::<CoverageGaps>();
    let _ = generator.subschema_for::<CoverageGapSummary>();
    let _ = generator.subschema_for::<UntestedFile>();
    let _ = generator.subschema_for::<UntestedFileFinding>();
    let _ = generator.subschema_for::<UntestedExport>();
    let _ = generator.subschema_for::<UntestedExportFinding>();
    let _ = generator.subschema_for::<HealthScore>();
    let _ = generator.subschema_for::<HealthScorePenalties>();
    let _ = generator.subschema_for::<VitalSigns>();
    let _ = generator.subschema_for::<VitalSignsCounts>();
    let _ = generator.subschema_for::<RiskProfile>();
    let _ = generator.subschema_for::<RuntimeCoverageReport>();
    let _ = generator.subschema_for::<HealthActionsMeta>();

    // Envelope and utility shapes (crates/types/src/envelope.rs).
    let _ = generator.subschema_for::<SchemaVersion>();
    let _ = generator.subschema_for::<ToolVersion>();
    let _ = generator.subschema_for::<ElapsedMs>();
    let _ = generator.subschema_for::<AuditIntroduced>();
    let _ = generator.subschema_for::<EntryPoints>();
    let _ = generator.subschema_for::<CheckSummary>();
    let _ = generator.subschema_for::<BaselineDeltas>();
    let _ = generator.subschema_for::<BaselineCategoryDelta>();
    let _ = generator.subschema_for::<BaselineMatch>();
    let _ = generator.subschema_for::<RegressionResult>();
    let _ = generator.subschema_for::<RegressionStatus>();
    let _ = generator.subschema_for::<RegressionToleranceKind>();
    let _ = generator.subschema_for::<Meta>();
    let _ = generator.subschema_for::<MetaMetric>();
    let _ = generator.subschema_for::<MetaRule>();

    register_per_command_envelope_definitions(&mut generator);

    // Typed document root. Must be registered AFTER every variant struct so
    // schemars resolves each variant against the already-registered
    // definition rather than inlining.
    let _ = generator.subschema_for::<FallowOutput>();

    register_list_boundaries_definitions(&mut generator);

    // Per-finding action wrapper types (crates/types/src/output_health.rs).
    let _ = generator.subschema_for::<HealthFindingAction>();
    let _ = generator.subschema_for::<HealthFindingActionType>();
    let _ = generator.subschema_for::<HotspotAction>();
    let _ = generator.subschema_for::<HotspotActionType>();
    let _ = generator.subschema_for::<HotspotActionHeuristic>();
    let _ = generator.subschema_for::<RefactoringTargetAction>();
    let _ = generator.subschema_for::<RefactoringTargetActionType>();
    let _ = generator.subschema_for::<UntestedFileAction>();
    let _ = generator.subschema_for::<UntestedFileActionType>();
    let _ = generator.subschema_for::<UntestedExportAction>();
    let _ = generator.subschema_for::<UntestedExportActionType>();

    // `apply_transforms = true` runs any registered schema transforms (e.g.
    // inline-subschemas) before returning, matching what `into_root_schema_for`
    // would have produced. We do not register custom transforms, so this is a
    // no-op today; passing `true` keeps the output stable if a future settings
    // change adds one.
    generator.take_definitions(true)
}

/// Register per-command envelope structs from `crates/cli/src/output_envelope.rs`.
/// Extracted from [`derived_definitions`] to keep the orchestrator under the
/// SIG unit-size threshold (the per-envelope list grew past the 150-line cap
/// when `FallowOutput` was added in #384 item 6).
fn register_per_command_envelope_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<AuditOutput>();
    let _ = generator.subschema_for::<AuditCommand>();
    let _ = generator.subschema_for::<CoverageSetupOutput>();
    let _ = generator.subschema_for::<CoverageSetupMember>();
    let _ = generator.subschema_for::<CoverageSetupFileToEdit>();
    let _ = generator.subschema_for::<CoverageSetupSnippet>();
    let _ = generator.subschema_for::<CoverageSetupSchemaVersion>();
    let _ = generator.subschema_for::<CoverageSetupFramework>();
    let _ = generator.subschema_for::<CoverageSetupPackageManager>();
    let _ = generator.subschema_for::<CoverageSetupRuntimeTarget>();
    let _ = generator.subschema_for::<CoverageAnalyzeOutput>();
    let _ = generator.subschema_for::<CoverageAnalyzeSchemaVersion>();
    let _ = generator.subschema_for::<CombinedOutput>();
    let _ = generator.subschema_for::<CheckOutput>();
    let _ = generator.subschema_for::<CheckGroupedOutput>();
    let _ = generator.subschema_for::<CheckGroupedEntry>();
    let _ = generator.subschema_for::<DupesOutput>();
    let _ = generator.subschema_for::<HealthOutput>();
    let _ = generator.subschema_for::<fallow_cli::health_types::HealthGroup>();
    let _ = generator.subschema_for::<fallow_cli::health_types::HealthReport>();
    let _ = generator.subschema_for::<GroupByMode>();
    let _ = generator.subschema_for::<ExplainOutput>();
    let _ = generator.subschema_for::<CodeClimateOutput>();
    let _ = generator.subschema_for::<CodeClimateIssue>();
    let _ = generator.subschema_for::<CodeClimateIssueKind>();
    let _ = generator.subschema_for::<CodeClimateSeverity>();
    let _ = generator.subschema_for::<CodeClimateLocation>();
    let _ = generator.subschema_for::<CodeClimateLines>();
    let _ = generator.subschema_for::<ReviewEnvelopeOutput>();
    let _ = generator.subschema_for::<ReviewEnvelopeSummary>();
    let _ = generator.subschema_for::<ReviewEnvelopeEvent>();
    let _ = generator.subschema_for::<ReviewComment>();
    let _ = generator.subschema_for::<GitHubReviewComment>();
    let _ = generator.subschema_for::<GitHubReviewSide>();
    let _ = generator.subschema_for::<GitLabReviewComment>();
    let _ = generator.subschema_for::<GitLabReviewPosition>();
    let _ = generator.subschema_for::<GitLabReviewPositionType>();
    let _ = generator.subschema_for::<ReviewEnvelopeMeta>();
    let _ = generator.subschema_for::<ReviewEnvelopeSchema>();
    let _ = generator.subschema_for::<ReviewProvider>();
    let _ = generator.subschema_for::<ReviewCheckConclusion>();
    let _ = generator.subschema_for::<ReviewReconcileOutput>();
    let _ = generator.subschema_for::<ReviewReconcileSchema>();
}

/// Register the `fallow list --boundaries --format json` envelope and its
/// building blocks. Extracted from [`derived_definitions`] to keep the
/// orchestrator under the SIG unit-size threshold; the pre-expansion
/// logical-group types live in `fallow_config` (issue #373) and ride along
/// via `JsonSchema` so the committed schema's `$ref`s resolve.
fn register_list_boundaries_definitions(generator: &mut schemars::SchemaGenerator) {
    let _ = generator.subschema_for::<ListBoundariesOutput>();
    let _ = generator.subschema_for::<BoundariesListing>();
    let _ = generator.subschema_for::<BoundariesListZone>();
    let _ = generator.subschema_for::<BoundariesListRule>();
    let _ = generator.subschema_for::<BoundariesListLogicalGroup>();
    let _ = generator.subschema_for::<LogicalGroup>();
    let _ = generator.subschema_for::<LogicalGroupStatus>();
    let _ = generator.subschema_for::<AuthoredRule>();
}

/// Merge derived definitions back into the hand-written schema document.
///
/// The committed `docs/output-schema.json` carries:
/// - top-level metadata (`$schema`, `title`, `description`, `oneOf`),
/// - hand-written envelopes and out-of-scope subtrees inside `definitions`.
///
/// We replace every entry in `definitions` whose key appears in
/// `derived_definition_names()` with the derived schema, and leave the rest
/// untouched. The diff between this output and the committed file is the
/// drift gate's signal.
fn merge_with_committed(derived: &Map<String, Value>) -> Result<Value, String> {
    let source = committed_schema_source()?;
    let mut document: Value = serde_json::from_str(&source)
        .map_err(|err| format!("failed to parse committed docs/output-schema.json: {err}"))?;

    let definitions = document
        .get_mut("definitions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            "committed docs/output-schema.json has no top-level `definitions` object".to_string()
        })?;

    let finding_names: rustc_hash::FxHashSet<&'static str> =
        finding_definition_names().iter().copied().collect();

    for name in derived_definition_names() {
        let derived_schema = derived.get(*name).ok_or_else(|| {
            format!(
                "derived schema missing for '{name}'; check that the type carries `#[cfg_attr(feature = \"schema\", derive(schemars::JsonSchema))]` and is registered in derived_definitions"
            )
        })?;
        let mut value = derived_schema.clone();
        normalize_schema(&mut value);
        if finding_names.contains(name) {
            augment_finding_definition(&mut value, finding_augmentation(name))?;
        }
        definitions.insert((*name).to_string(), value);
    }

    // Schemars produces transitively-referenced helper definitions for every
    // typed enum / payload subtype on the in-scope structs (`FixActionType`,
    // the kebab-case kind enums, `DependencyLocation`, `MemberKind`,
    // `CoverageSetupFramework`, etc.). After Phase 8 every
    // helper that appears in `docs/output-schema.json` is a derived artifact,
    // so always overwrite the committed entry rather than preserving it. The
    // previous "skip if already present" guard silently froze the helper
    // shape on the first regen; any subsequent change to a serde rename or
    // schemars attribute would be invisible until the helper was manually
    // deleted from the committed file. The explicit `derived_definition_names()`
    // list above is the drift-checked surface; this loop fills in every
    // transitively-referenced helper so the `$ref` graph resolves.
    let in_scope: rustc_hash::FxHashSet<&'static str> =
        derived_definition_names().iter().copied().collect();
    for (name, value) in derived {
        if in_scope.contains(name.as_str()) {
            continue;
        }
        let mut value = value.clone();
        normalize_schema(&mut value);
        definitions.insert(name.clone(), value);
    }

    rewrite_document_root_one_of(&mut document)?;

    Ok(document)
}

/// Hand-maintained root-level envelope definitions that are NOT yet typed
/// via Rust + schemars but DO appear as top-level `--format json` outputs.
/// Each entry is referenced from the document-root `oneOf` so the typed
/// surface (`FallowOutput`) plus the bare-array CodeClimate spec plus these
/// hand-maintained envelopes together document every shape fallow can emit.
///
/// Entries here MUST also appear as a `$ref` from the document-root `oneOf`
/// (the drift test `hand_maintained_root_envelopes_appear_in_root_one_of`
/// asserts this). Removing an entry means the migration has landed and the
/// envelope is now a variant of `FallowOutput`; in that case the
/// corresponding `definitions[<name>]` block must also be removed (or
/// remain only as a transitive helper) so the test
/// `every_registered_name_resolves_to_a_derived_schema` still passes.
const HAND_MAINTAINED_ROOT_ENVELOPES: &[&str] = &[];

/// Drive the document-root `oneOf` from the typed `FallowOutput` enum plus
/// the two non-object branches (`CodeClimateOutput`, hand-maintained
/// envelopes). Replaces the previously hand-maintained block.
///
/// Also rewrites the root `description` to point readers at the discriminator
/// rules (untagged + unique-field-presence) rather than the per-command
/// enumeration the old prose carried.
fn rewrite_document_root_one_of(document: &mut Value) -> Result<(), String> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| "schema document root is not a JSON object".to_string())?;

    let mut one_of: Vec<Value> = Vec::with_capacity(2 + HAND_MAINTAINED_ROOT_ENVELOPES.len());
    one_of.push(serde_json::json!({ "$ref": "#/definitions/FallowOutput" }));
    // CodeClimateOutput serializes as `Vec<CodeClimateIssue>` via
    // `#[serde(transparent)]`. `#[serde(tag = ...)]` cannot internally tag
    // a non-object variant and wrapping the array would break the Code
    // Climate / GitLab Code Quality spec, so it stays as a sibling root
    // branch outside `FallowOutput`.
    one_of.push(serde_json::json!({ "$ref": "#/definitions/CodeClimateOutput" }));
    for name in HAND_MAINTAINED_ROOT_ENVELOPES {
        one_of.push(serde_json::json!({ "$ref": format!("#/definitions/{name}") }));
    }
    root.insert("oneOf".to_string(), Value::Array(one_of));

    root.insert(
        "description".to_string(),
        Value::String(
            "Schemas for the JSON output of fallow commands. To identify which \
             envelope you have, check for the unique top-level field: \
             `summary.total_issues` (check), `health_score` (health), \
             `clone_groups` (dupes), `runtime_coverage` (coverage analyze), \
             `boundaries` (list --boundaries), `command: \"audit\"` (audit), \
             `body` plus `comments` (review-github / review-gitlab), \
             `schema: \"fallow-review-reconcile/v1\"` (ci reconcile-review), \
             `framework_detected` plus `members` (coverage setup), `id` plus \
             `how_to_fix` (explain), `check`+`dupes`+`health` keys together \
             (bare combined invocation). `HealthOutput` and `DupesOutput` \
             flatten their body (`HealthReport` / `DupesReportPayload`) into \
             top-level fields, so the discriminator field is from the body \
             shape itself, not a wrapper key. Every object-shaped envelope \
             is a variant of `FallowOutput`; `CodeClimateOutput` is a bare \
             JSON array (per the Code Climate / GitLab Code Quality spec) \
             and stays a sibling root branch."
                .to_string(),
        ),
    );

    Ok(())
}

/// Add the `actions` array and optional `introduced` flag to a derived
/// finding schema. These two fields are injected by the JSON output layer
/// (`crates/cli/src/report/json.rs`) on every issue object but are not on the
/// Rust source struct, so the schema needs them grafted in to match what
/// downstream consumers actually receive.
///
/// The augmentation is idempotent: if the derived schema already carries an
/// `actions` property (e.g. because a future PR refactors the JSON layer to
/// serialize through typed wrappers), the augmentation step skips and the
/// derived shape wins.
///
/// `augmentation` selects the `actions[]` `$ref` and whether `introduced` is
/// attached. Dead-code findings use [`DEFAULT_FINDING_AUGMENTATION`] (actions
/// of type `IssueAction`, `introduced` attached); health findings use the
/// matching per-finding wrapper (`HealthFindingAction` / `HotspotAction` /
/// `RefactoringTargetAction`) and skip `introduced` when the finding does not
/// flow through `fallow audit`.
fn augment_finding_definition(
    value: &mut Value,
    augmentation: FindingAugmentation,
) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "finding definition is not a JSON object".to_string())?;

    let properties = object
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    let properties = properties
        .as_object_mut()
        .ok_or_else(|| "finding definition `properties` is not a JSON object".to_string())?;

    if !properties.contains_key("actions") {
        properties.insert(
            "actions".to_string(),
            serde_json::json!({
                "type": "array",
                "items": { "$ref": augmentation.actions_item_ref },
                "description": "Suggested actions to resolve this issue."
            }),
        );
    }
    if augmentation.include_introduced && !properties.contains_key("introduced") {
        properties.insert(
            "introduced".to_string(),
            serde_json::json!({ "$ref": "#/definitions/AuditIntroduced" }),
        );
    }

    let required = object
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(arr) = required
        && !arr.iter().any(|v| v.as_str() == Some("actions"))
    {
        arr.push(Value::String("actions".to_string()));
    }

    Ok(())
}

/// Apply post-processing to derived schemas so they match the conventions of
/// the hand-written `docs/output-schema.json`.
///
/// Production normalization (this function, applied to the emitted document):
///
/// - Drop the `$schema` keyword that schemars writes on each subschema; only
///   the top-level document carries it.
/// - Schemars 1 prefers `$ref` -> `#/$defs/Foo`, but the committed file uses
///   `#/definitions/Foo`. Rewrite refs so they line up with the merged
///   document layout.
///
/// Drift-comparison normalization (the `normalize_one` helper inside
/// `#[cfg(test)] mod drift_tests`, applied ONLY before structural equality
/// checks): drops `format`/`minimum`/`maximum`/`description` keywords,
/// collapses `type: ["X", "null"]` to `type: "X"`, collapses single-element
/// `allOf: [{$ref: X}]` wrappers to the bare `$ref`, and canonicalizes
/// `oneOf`/`anyOf`. Those rewrites do NOT run on the emitted document;
/// they exist so the drift gate can compare structures while tolerating
/// schemars' integer-format hints, nullable-union output, and doc-comment
/// prose churn that the committed schema does not encode the same way.
/// Editing this function's behavior should usually be mirrored in
/// `normalize_one`, and vice versa.
fn normalize_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("$schema");
            // Strip schemars cosmetic output that the committed schema does not
            // encode: `default` from `#[serde(default)]`, integer-width formats
            // and bounds from `u8`/`u32`/`usize`/etc, and per-property example
            // hints. These survive into the regenerated document otherwise and
            // would force every consumer to handle schemars-version churn. The
            // test-side normalizer at `normalize_one` mirrors these strips so
            // the strict drift gate stays symmetric.
            map.remove("default");
            map.remove("examples");
            map.remove("format");
            map.remove("minimum");
            map.remove("maximum");
            map.remove("exclusiveMinimum");
            map.remove("exclusiveMaximum");
            if let Some(Value::String(reference)) = map.get_mut("$ref")
                && let Some(rest) = reference.strip_prefix("#/$defs/")
            {
                *reference = format!("#/definitions/{rest}");
            }
            // Schemars wraps `$ref` in a single-arm `allOf` when the field also
            // carries a `description` (so the description does not lose its
            // owner). Collapse to a bare `$ref` alongside the description; the
            // committed schema uses the flat form and downstream tools handle
            // both interchangeably.
            if let Some(Value::Array(all_of)) = map.get("allOf")
                && all_of.len() == 1
                && let Some(Value::Object(only)) = all_of.first()
                && only.len() == 1
                && only.contains_key("$ref")
            {
                let reference = only.get("$ref").cloned().unwrap_or(Value::Null);
                map.remove("allOf");
                map.insert("$ref".to_string(), reference);
            }
            for (key, child) in map.iter_mut() {
                // Keys inside `properties` / `definitions` / `$defs` /
                // `patternProperties` maps are user-facing names (struct field
                // names, type names), not schema keywords. Recurse into each
                // VALUE (each is itself a schema) without applying the
                // keyword strip to the surrounding map's keys; otherwise a
                // struct field literally named `format` / `default` /
                // `minimum` / etc. would be silently dropped from the
                // emitted schema. Issue #394 fired this for
                // `ContributorEntry.format: ContributorIdentifierFormat`.
                if matches!(
                    key.as_str(),
                    "properties" | "definitions" | "$defs" | "patternProperties"
                ) && let Value::Object(inner) = child
                {
                    for inner_value in inner.values_mut() {
                        normalize_schema(inner_value);
                    }
                    continue;
                }
                normalize_schema(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_schema(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod drift_tests {
    //! Drift gate for the Rust → `docs/output-schema.json` chain.
    //!
    //! The structural gate walks every definition schemars produces (not just
    //! the explicit `derived_definition_names()` allow-list) and compares it
    //! against the matching entry in the committed schema after
    //! canonicalization. Transitive helpers (`AnalysisResults`, `MemberKind`,
    //! `FixActionType`, every kebab-case enum, every utility newtype) are
    //! drift-checked alongside the explicitly-registered envelopes.
    //!
    //! Canonicalization erases documented cosmetic differences (doc-comment
    //! prose, schemars-style `nullable` integer formats, `oneOf` vs `anyOf`,
    //! single-arm `allOf` wrappers) so the comparison fires only on real
    //! structural drift.
    //!
    //! `derived_definition_names()` survives as the allow-list for the
    //! post-derivation augmentation (`actions` / `introduced` graft on
    //! findings); the drift tests below iterate the full derived map.
    //!
    //! Real drift fires loudly: a renamed Rust field, a new struct field, or
    //! a type change shows up as a property/required/type mismatch on the
    //! relevant definition. Pure prose changes do not fire; those are tracked
    //! by the prose-migration phase that moves descriptions into `///` doc
    //! comments.

    use super::*;

    /// Run a single normalization pass on a JSON value, recursively. Returns
    /// the canonical form used by the drift comparison.
    fn canonicalize(mut value: Value) -> Value {
        normalize_one(&mut value);
        value
    }

    fn normalize_one(value: &mut Value) {
        match value {
            Value::Object(map) => {
                // Drop description prose entirely. Phase 8 will sync prose
                // back from Rust doc comments; until then the drift gate
                // tolerates description divergence by design.
                map.remove("description");
                // Schemars derives integer constraints from the underlying
                // Rust width. The committed schema does not encode width
                // today, so strip the integer-format hints before comparing.
                map.remove("format");
                map.remove("minimum");
                map.remove("maximum");
                map.remove("exclusiveMinimum");
                map.remove("exclusiveMaximum");
                // Schemars 1 emits `Option<T>` as `type: ["X", "null"]`. The
                // committed schema marks optionals via `skip_serializing_if`
                // alone, so collapse the nullable union to a scalar `type`.
                if let Some(Value::Array(arr)) = map.get_mut("type") {
                    arr.retain(|v| v.as_str() != Some("null"));
                    if arr.len() == 1 {
                        let only = arr.remove(0);
                        map.insert("type".to_string(), only);
                    }
                }
                // Single-element `allOf: [{$ref: X}]` -> bare `{$ref: X}`.
                // Schemars emits the wrapper when a variant carries doc text.
                if let Some(Value::Array(all_of)) = map.get("allOf")
                    && all_of.len() == 1
                    && let Some(Value::Object(only)) = all_of.first()
                    && only.len() == 1
                    && only.contains_key("$ref")
                {
                    let reference = only.get("$ref").cloned().unwrap_or(Value::Null);
                    map.remove("allOf");
                    map.insert("$ref".to_string(), reference);
                }
                // Treat `oneOf` and `anyOf` as equivalent for discriminated
                // unions: canonicalize to `oneOf`. Both validate the same
                // instances for mutually-exclusive variants in practice.
                if let Some(any_of) = map.remove("anyOf") {
                    map.insert("oneOf".to_string(), any_of);
                }
                // Sort `required` and `enum` arrays so order differences do
                // not fire the gate.
                if let Some(Value::Array(items)) = map.get_mut("required") {
                    items.sort_by(|a, b| {
                        a.as_str()
                            .unwrap_or_default()
                            .cmp(b.as_str().unwrap_or_default())
                    });
                }
                if let Some(Value::Array(items)) = map.get_mut("enum") {
                    items.sort_by(|a, b| {
                        a.as_str()
                            .unwrap_or_default()
                            .cmp(b.as_str().unwrap_or_default())
                    });
                }
                for (key, child) in map.iter_mut() {
                    // Mirror the production-side guard in `normalize_schema`:
                    // do not apply the keyword strip to keys inside
                    // `properties` / `definitions` / `$defs` /
                    // `patternProperties` maps because those keys are
                    // property/type names, not schema keywords. Without this
                    // guard a struct field named `format` (or `default` /
                    // `minimum` / etc) is dropped before the drift gate
                    // compares structures, masking a real schema regression.
                    if matches!(
                        key.as_str(),
                        "properties" | "definitions" | "$defs" | "patternProperties"
                    ) && let Value::Object(inner) = child
                    {
                        for inner_value in inner.values_mut() {
                            normalize_one(inner_value);
                        }
                        continue;
                    }
                    normalize_one(child);
                }
            }
            Value::Array(items) => {
                for item in items {
                    normalize_one(item);
                }
            }
            _ => {}
        }
    }

    fn committed_definitions() -> Map<String, Value> {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        document
            .get("definitions")
            .and_then(Value::as_object)
            .cloned()
            .expect("committed docs/output-schema.json must carry `definitions`")
    }

    /// Build the full set of derived definitions for drift comparison: every
    /// key schemars emits, normalized; augmented only for entries in
    /// `derived_definition_names()` (the post-pass `actions`/`introduced` graft
    /// applies to findings). Transitive helpers (e.g., `AnalysisResults`,
    /// `MemberKind`, `FixActionType`, every kebab-case enum) are included
    /// without augmentation so the strict gate covers every committed
    /// definition, not just the explicit allow-list.
    fn derived_definitions_for_drift() -> Map<String, Value> {
        let raw = derived_definitions();
        let mut out = Map::new();
        let finding_names: rustc_hash::FxHashSet<&'static str> =
            finding_definition_names().iter().copied().collect();
        let in_scope: rustc_hash::FxHashSet<&'static str> =
            derived_definition_names().iter().copied().collect();
        for (name, raw_value) in &raw {
            let mut value = raw_value.clone();
            normalize_schema(&mut value);
            if in_scope.contains(name.as_str()) && finding_names.contains(name.as_str()) {
                augment_finding_definition(&mut value, finding_augmentation(name))
                    .expect("augment_finding_definition must not fail");
            }
            out.insert(name.clone(), value);
        }
        out
    }

    /// Catch new derives that landed in Rust without being registered in
    /// `derived_definition_names()`. Without this assertion a contributor
    /// could add `JsonSchema` to a new struct, forget the registration step,
    /// and the drift gate would silently skip the new type forever.
    #[test]
    fn every_registered_name_resolves_to_a_derived_schema() {
        let derived = derived_definitions();
        for name in derived_definition_names() {
            assert!(
                derived.contains_key(*name),
                "no derived schema for `{name}`: either the type lacks `#[cfg_attr(feature = \"schema\", derive(schemars::JsonSchema))]`, or the call to `generator.subschema_for::<{name}>()` is missing in `derived_definitions()`."
            );
        }
    }

    /// Every variant of [`FallowOutput`] must have its inner type registered
    /// in [`derived_definitions`]. Without registration, schemars inlines the
    /// variant's schema in the root `FallowOutput` `oneOf` rather than
    /// emitting a `$ref`; the document-root union then drifts from the
    /// `definitions/` map and the drift gate may or may not catch it
    /// depending on whether the variant's inner type is transitively
    /// referenced from another registered type.
    ///
    /// The `VARIANTS` list is hand-maintained because Rust does not provide
    /// reflection over enum variants. The nested `_variant_count_is_locked`
    /// match produces a `non-exhaustive patterns` compile error if a
    /// contributor adds a variant to [`FallowOutput`] without updating this
    /// test, so the list cannot silently drift.
    ///
    /// Regression for issue #417: mechanizes the `#[allow(dead_code)]`
    /// social contract on the enum into a `cargo test`-time assertion.
    #[test]
    fn every_fallow_output_variant_is_registered_in_derived_definitions() {
        // (variant tag for the diagnostic, inner type name as schemars
        // emits it). Keep in sync with `enum FallowOutput` in
        // `crates/cli/src/output_envelope.rs`. The exhaustive match below
        // enforces this at compile time.
        const VARIANTS: &[(&str, &str)] = &[
            ("Audit", "AuditOutput"),
            ("Explain", "ExplainOutput"),
            ("ReviewEnvelope", "ReviewEnvelopeOutput"),
            ("ReviewReconcile", "ReviewReconcileOutput"),
            ("CoverageSetup", "CoverageSetupOutput"),
            ("CoverageAnalyze", "CoverageAnalyzeOutput"),
            ("ListBoundaries", "ListBoundariesOutput"),
            ("Health", "HealthOutput"),
            ("Dupes", "DupesOutput"),
            ("CheckGrouped", "CheckGroupedOutput"),
            ("Check", "CheckOutput"),
            ("Combined", "CombinedOutput"),
        ];

        // Compile-time exhaustiveness check. Adding a new variant to
        // `FallowOutput` without extending `VARIANTS` above fails this
        // match with `non-exhaustive patterns`. The function is never
        // called; it exists solely to lock the variant count.
        #[expect(
            dead_code,
            reason = "compile-time exhaustiveness guard for the VARIANTS list above; never called at runtime"
        )]
        fn variant_count_is_locked(value: &FallowOutput) -> &'static str {
            // The leading `variant_` (not `_variant_`) is intentional:
            // rustc auto-silences `dead_code` on identifiers starting
            // with `_`, which would make `#[expect(dead_code)]`
            // unfulfilled and trigger `unfulfilled_lint_expectations`.
            match value {
                FallowOutput::Audit(_) => "Audit",
                FallowOutput::Explain(_) => "Explain",
                FallowOutput::ReviewEnvelope(_) => "ReviewEnvelope",
                FallowOutput::ReviewReconcile(_) => "ReviewReconcile",
                FallowOutput::CoverageSetup(_) => "CoverageSetup",
                FallowOutput::CoverageAnalyze(_) => "CoverageAnalyze",
                FallowOutput::ListBoundaries(_) => "ListBoundaries",
                FallowOutput::Health(_) => "Health",
                FallowOutput::Dupes(_) => "Dupes",
                FallowOutput::CheckGrouped(_) => "CheckGrouped",
                FallowOutput::Check(_) => "Check",
                FallowOutput::Combined(_) => "Combined",
            }
        }

        let derived = derived_definitions();
        let mut missing: Vec<String> = Vec::new();
        for (variant, inner) in VARIANTS {
            if !derived.contains_key(*inner) {
                missing.push(format!(
                    "variant `FallowOutput::{variant}({inner})` produces an inline schema in the root `oneOf` because `{inner}` is not registered in `derived_definitions()`. Add `let _ = generator.subschema_for::<{inner}>();` (or include it via `register_per_command_envelope_definitions` / `register_list_boundaries_definitions`)."
                ));
            }
        }
        assert!(
            missing.is_empty(),
            "{} `FallowOutput` variant(s) missing registration:\n\n{}",
            missing.len(),
            missing.join("\n\n"),
        );
    }

    /// Each finding type listed in `finding_definition_names()` must exist in
    /// the registered set, otherwise the augmentation pass silently skips it.
    #[test]
    fn finding_names_are_subset_of_registered_names() {
        let registered: rustc_hash::FxHashSet<&'static str> =
            derived_definition_names().iter().copied().collect();
        for name in finding_definition_names() {
            assert!(
                registered.contains(name),
                "finding type `{name}` is augmented with `actions`/`introduced` but never registered as a derived definition. Add it to `derived_definition_names()` (and the corresponding `subschema_for::<{name}>()` call) before listing it as a finding."
            );
        }
    }

    /// Augmentation attaches the `actions` array to every finding type, and
    /// the `introduced` flag to every audit-aware finding (see
    /// `finding_augmentation`: hotspot and refactoring target are not
    /// audit-aware today, so their derived schemas must NOT carry
    /// `introduced`). The required-flag for `actions` is decided by the
    /// committed schema per-type; the augmentation step is non-opinionated.
    #[test]
    fn augmentation_attaches_actions_and_introduced_to_each_finding() {
        let derived = derived_definitions_for_drift();
        for name in finding_definition_names() {
            let entry = derived
                .get(*name)
                .unwrap_or_else(|| panic!("finding `{name}` missing from derived"));
            let properties = entry
                .get("properties")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("finding `{name}` missing properties"));
            assert!(
                properties.contains_key("actions"),
                "finding `{name}` was not augmented with `actions`",
            );
            let aug = finding_augmentation(name);
            if aug.include_introduced {
                assert!(
                    properties.contains_key("introduced"),
                    "finding `{name}` was not augmented with `introduced` (audit-aware finding)",
                );
            } else {
                assert!(
                    !properties.contains_key("introduced"),
                    "finding `{name}` carries `introduced` but `finding_augmentation` opted out",
                );
            }
        }
    }

    /// Field-level drift gate: for every in-scope definition, every property
    /// in the derived schema must exist in the committed schema (and vice
    /// versa, modulo known JSON-layer augmentations `actions` / `introduced`).
    /// Required-field sets must match exactly modulo the same augmentations.
    ///
    /// Catches the high-value drift classes:
    /// - Rust struct field added → committed schema is missing the property
    /// - Rust struct field renamed → committed has the old name only
    /// - Rust struct field removed → committed has a stale property
    /// - `Option<T>` flipped to `T` (or vice versa) → required mismatch
    ///
    /// Does NOT catch property-value drift (e.g., `u32` → `String`).
    /// Tightening that check is deferred until the prose-migration phase
    /// lets the canonicalizer be strict about schemars-vs-handwritten shape
    /// differences.
    #[test]
    fn committed_definitions_match_derived_property_keys() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        // Augmentation keys live only in the committed schema for finding
        // types because they get grafted on by `augment_finding_definition`.
        // `actions_meta` was previously here for the `HealthOutput` post-pass
        // injection, but Phase 8 modelled it as `Option<HealthActionsMeta>` on
        // `HealthReport` (flattened into `HealthOutput`) so schemars emits the
        // field natively. As of #384 B2 the typed `HealthFinding` wrapper
        // also carries `actions` + `introduced` natively, so those keys do
        // not need an augmentation graft for `HealthFinding`. Permit
        // `actions` / `introduced` to differ between sides without firing
        // the gate; everything else must match.
        const AUGMENTATION_KEYS: &[&str] = &["actions", "introduced"];

        let mut failures: Vec<String> = Vec::new();
        for name in derived.keys() {
            let Some(committed_entry) = committed.get(name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`. Add a stub entry to `definitions` (the drift test only compares; it does not insert)."
                ));
                continue;
            };
            let derived_entry = derived
                .get(name)
                .expect("iterating derived's own keys; entry must exist");

            let committed_props = committed_entry.get("properties").and_then(Value::as_object);
            let derived_props = derived_entry.get("properties").and_then(Value::as_object);

            if let (Some(committed_props), Some(derived_props)) = (committed_props, derived_props) {
                for key in derived_props.keys() {
                    if !committed_props.contains_key(key) {
                        failures.push(format!(
                            "drift on `{name}`: property `{key}` is in the Rust struct (derived schema) but missing from `docs/output-schema.json`"
                        ));
                    }
                }
                for key in committed_props.keys() {
                    if !derived_props.contains_key(key)
                        && !AUGMENTATION_KEYS.contains(&key.as_str())
                    {
                        failures.push(format!(
                            "drift on `{name}`: property `{key}` is in `docs/output-schema.json` but missing from the Rust struct (derived schema)"
                        ));
                    }
                }
            }

            let committed_required: rustc_hash::FxHashSet<String> = committed_entry
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let derived_required: rustc_hash::FxHashSet<String> = derived_entry
                .get("required")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            for key in &derived_required {
                if !committed_required.contains(key) {
                    failures.push(format!(
                        "drift on `{name}`: property `{key}` is required by the Rust struct but optional in `docs/output-schema.json`"
                    ));
                }
            }
            for key in &committed_required {
                if !derived_required.contains(key) && !AUGMENTATION_KEYS.contains(&key.as_str()) {
                    failures.push(format!(
                        "drift on `{name}`: property `{key}` is required by `docs/output-schema.json` but optional in the Rust struct"
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "schema drift detected ({} issue{}):\n\n  - {}\n\nRegenerate the in-scope `definitions` blocks with:\n    cargo run -p fallow-cli --features schema-emit --bin fallow-schema-emit > /tmp/emitted-schema.json\nthen reconcile the relevant entries in `docs/output-schema.json` against the derived shape, or update the Rust source if the schema change was the intended source of truth.",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n  - "),
        );
    }

    /// Targeted property-`$ref` drift gate. For every property on every
    /// in-scope definition, if BOTH sides have a `$ref` at the same key,
    /// the ref targets must match. Catches the specific failure mode where
    /// the committed schema documents a sub-key as pointing at one
    /// definition (e.g. `CombinedOutput.dupes` -> `DupesOutput`) while the
    /// derived Rust source actually produces a different shape on the wire
    /// (bare `DuplicationReport`). The property-key gate above misses this
    /// because the property exists on both sides under the same name; only
    /// the `$ref` VALUE differs.
    ///
    /// Canonicalisation reuses [`normalize_one`] so schemars's
    /// `allOf: [{$ref: X}]` wrapper around doc-bearing fields and
    /// `anyOf: [{$ref: X}, {type: null}]` wrapper around `Option<T>`
    /// fields both collapse to bare `$ref` before comparison.
    /// Per-array `items.$ref` is intentionally NOT compared: arrays whose
    /// element type changes already fire the property-key gate via
    /// transitive schemas, and adding items-level checks here would
    /// require deeper structural unification that belongs in the
    /// `#[ignore]`d strict gate.
    #[test]
    fn committed_property_refs_match_derived_property_refs() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        let mut failures: Vec<String> = Vec::new();

        for name in derived.keys() {
            let Some(committed_entry) = committed.get(name) else {
                continue;
            };
            let Some(derived_entry) = derived.get(name) else {
                continue;
            };

            let committed_props = committed_entry.get("properties").and_then(Value::as_object);
            let derived_props = derived_entry.get("properties").and_then(Value::as_object);

            if let (Some(committed_props), Some(derived_props)) = (committed_props, derived_props) {
                for (key, derived_value) in derived_props {
                    let Some(committed_value) = committed_props.get(key) else {
                        continue;
                    };
                    let derived_ref = canonical_ref(derived_value);
                    let committed_ref = canonical_ref(committed_value);
                    if let (Some(dref), Some(cref)) = (&derived_ref, &committed_ref)
                        && dref != cref
                    {
                        failures.push(format!(
                            "drift on `{name}.{key}`: derived schema points at `{dref}` but committed schema points at `{cref}`"
                        ));
                    }
                }
            }
        }

        assert!(
            failures.is_empty(),
            "schema `$ref` drift detected ({} issue{}):\n\n  - {}\n\nThe wire format produced by the Rust source disagrees with the type the committed schema documents. Either update `docs/output-schema.json` to point at the type the wire actually emits, or change the runtime to produce the documented shape.",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n  - "),
        );
    }

    /// Extract the canonical `$ref` target from a property value, peeling
    /// schemars' `allOf` / `anyOf` / `oneOf` wrappers. Returns `None` for
    /// properties that do not reference another definition at the top
    /// level (primitive types, arrays, free-form objects).
    fn canonical_ref(value: &Value) -> Option<String> {
        let mut canonical = value.clone();
        normalize_one(&mut canonical);
        if let Some(Value::String(s)) = canonical.get("$ref") {
            return Some(s.clone());
        }
        if let Some(Value::Array(arr)) = canonical.get("oneOf") {
            for variant in arr {
                if let Some(Value::String(s)) = variant.get("$ref") {
                    return Some(s.clone());
                }
            }
        }
        None
    }

    /// The emitted schema's `$ref` graph must close: every `#/definitions/X`
    /// reference must point at a definition that exists in the merged
    /// document. A dangling ref means the schema is invalid for AJV-strict
    /// consumers and would fail downstream validation. Schemars produces
    /// helper definitions for typed enum / payload subtypes
    /// (`FixActionType`, `DependencyLocation`,
    /// `MemberKind`, ...) on the in-scope structs; if `merge_with_committed`
    /// drops any of them, this test fires.
    #[test]
    fn emitted_schema_has_no_dangling_refs() {
        let derived = derived_definitions();
        let document =
            merge_with_committed(&derived).expect("merge must succeed on committed schema");

        let mut defined: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        if let Some(map) = document.get("definitions").and_then(Value::as_object) {
            for key in map.keys() {
                defined.insert(key.clone());
            }
        }

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        fn collect_refs(node: &Value, out: &mut rustc_hash::FxHashSet<String>) {
            match node {
                Value::Object(map) => {
                    if let Some(Value::String(reference)) = map.get("$ref")
                        && let Some(name) = reference.strip_prefix("#/definitions/")
                    {
                        out.insert(name.to_string());
                    }
                    for child in map.values() {
                        collect_refs(child, out);
                    }
                }
                Value::Array(items) => {
                    for child in items {
                        collect_refs(child, out);
                    }
                }
                _ => {}
            }
        }
        collect_refs(&document, &mut refs);

        let mut missing: Vec<String> = refs.difference(&defined).cloned().collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "emitted schema has {} dangling `$ref` target{}: {}\n\n\
             A regenerated `docs/output-schema.json` with dangling refs is invalid; \
             every referenced name must appear under `definitions`. If schemars \
             produced a transitive helper definition, ensure `merge_with_committed` \
             inserts every entry from the derived map (not just names in \
             `derived_definition_names()`).",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            missing.join(", "),
        );
    }

    /// Strict drift gate: full structural comparison of every in-scope
    /// definition against the committed schema, after canonicalization.
    ///
    /// Runs on every `cargo test` invocation now that the committed schema is
    /// regenerated from Rust as the source of truth. The canonicalization
    /// step erases the documented cosmetic differences (doc-comment prose,
    /// `oneOf` vs `anyOf`, single-arm `allOf` wrappers, schemars integer-
    /// width hints, `Option<T>` nullable-union forms). Anything else fires.
    #[test]
    fn committed_definitions_match_derived_structurally() {
        let committed = committed_definitions();
        let derived = derived_definitions_for_drift();
        let mut failures: Vec<String> = Vec::new();
        for (name, derived_value) in &derived {
            let Some(committed_value) = committed.get(name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`."
                ));
                continue;
            };
            let derived_entry = canonicalize(derived_value.clone());
            let committed_entry = canonicalize(committed_value.clone());
            if committed_entry != derived_entry {
                let committed_pretty = serde_json::to_string_pretty(&committed_entry)
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                let derived_pretty = serde_json::to_string_pretty(&derived_entry)
                    .unwrap_or_else(|_| "<unprintable>".to_string());
                failures.push(format!(
                    "drift on `{name}`:\n--- committed (canonicalized) ---\n{committed_pretty}\n--- derived (canonicalized) ---\n{derived_pretty}"
                ));
            }
        }
        // Catch orphans in the committed file: any definition listed in
        // `docs/output-schema.json` that schemars no longer emits is a stale
        // hand-edit waiting to drift. After Phase 8 every helper is
        // overwritten on regen, so an orphan can only land via a manual edit.
        //
        // The allow-list below holds definitions that are legitimately
        // hand-maintained pending other #384 items. Each entry MUST link to
        // the issue item that will retire it; this is not a permanent
        // escape hatch.
        const HAND_MAINTAINED_ALLOW_LIST: &[(&str, &str)] = &[];
        let allow_list: rustc_hash::FxHashSet<&'static str> = HAND_MAINTAINED_ALLOW_LIST
            .iter()
            .map(|(name, _)| *name)
            .collect();
        for name in committed.keys() {
            if !derived.contains_key(name) && !allow_list.contains(name.as_str()) {
                failures.push(format!(
                    "orphan in `docs/output-schema.json`: definition `{name}` is not produced by `derived_definitions()`. Either register the type via `subschema_for::<{name}>()` in `derived_definitions`, or delete the stale entry. (If the entry is hand-maintained pending another #384 item, add it to `HAND_MAINTAINED_ALLOW_LIST` with a reason linking the issue.)"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} structural drift issue{}:\n\n{}",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n\n"),
        );
    }

    /// Regression for issue #394: `normalize_schema` recursively walks every
    /// JSON object and strips schema-keyword names (`format`, `default`,
    /// `minimum`, `maximum`, `examples`, `exclusiveMinimum`,
    /// `exclusiveMaximum`). Before the fix it also stripped those keys when
    /// they appeared as struct-field names inside a `properties` map,
    /// silently dropping `ContributorEntry.format` from the emitted schema
    /// and triggering ajv `strictRequired` because `format` stayed in the
    /// `required` array. The guard skips the strip inside `properties` /
    /// `definitions` / `$defs` / `patternProperties` so a property named
    /// `format` (or any other keyword name) survives.
    #[test]
    fn normalize_schema_preserves_property_named_format() {
        let mut value = serde_json::json!({
            "type": "object",
            "properties": {
                "format": { "$ref": "#/definitions/SomeEnum" },
                "minimum": { "type": "integer" },
                "default": { "type": "string" },
                "regular": { "type": "string", "format": "uri" }
            },
            "required": ["format", "minimum", "default", "regular"]
        });
        super::normalize_schema(&mut value);
        let properties = value
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties stays an object");
        assert!(
            properties.contains_key("format"),
            "property `format` must survive normalize_schema (issue #394)"
        );
        assert!(
            properties.contains_key("minimum"),
            "property `minimum` must survive normalize_schema"
        );
        assert!(
            properties.contains_key("default"),
            "property `default` must survive normalize_schema"
        );
        let regular = properties
            .get("regular")
            .and_then(Value::as_object)
            .expect("`regular` stays an object");
        assert!(
            !regular.contains_key("format"),
            "schemars `format` keyword inside a property's schema is still stripped"
        );
    }

    /// Mirror of `normalize_schema_preserves_property_named_format` for the
    /// drift-test side `normalize_one`. Without the same guard the
    /// canonicalized committed schema would lose its `format` property
    /// before the comparison and the drift gate would silently accept a
    /// Rust-side rename or removal of that field.
    #[test]
    fn normalize_one_preserves_property_named_format() {
        let mut value = serde_json::json!({
            "type": "object",
            "properties": {
                "format": { "$ref": "#/definitions/SomeEnum" },
                "minimum": { "type": "integer" },
                "regular": { "type": "string", "format": "uri" }
            },
            "required": ["format", "minimum", "regular"]
        });
        normalize_one(&mut value);
        let properties = value
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties stays an object");
        assert!(
            properties.contains_key("format"),
            "property `format` must survive normalize_one"
        );
        assert!(
            properties.contains_key("minimum"),
            "property `minimum` must survive normalize_one"
        );
        let regular = properties
            .get("regular")
            .and_then(Value::as_object)
            .expect("`regular` stays an object");
        assert!(
            !regular.contains_key("format"),
            "schemars `format` keyword inside a property's schema is still stripped"
        );
    }

    /// Every entry in `HAND_MAINTAINED_ROOT_ENVELOPES` MUST appear as a
    /// `$ref` in the document-root `oneOf`. Without this gate, a future
    /// migration that types `CoverageAnalyzeOutput` and removes its
    /// `definitions` entry could silently drop it from the documented
    /// union if the implementer forgot to add the variant to
    /// `FallowOutput`. The drift test fires so the regression surfaces
    /// at `cargo test` time rather than at downstream-consumer time.
    #[test]
    fn hand_maintained_root_envelopes_appear_in_root_one_of() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let one_of = document
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("committed schema must carry a root-level `oneOf`");

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for entry in one_of {
            if let Some(reference) = entry.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/definitions/")
            {
                refs.insert(name.to_string());
            }
        }

        for name in HAND_MAINTAINED_ROOT_ENVELOPES {
            assert!(
                refs.contains(*name),
                "hand-maintained root envelope `{name}` is registered in \
                 `HAND_MAINTAINED_ROOT_ENVELOPES` but is not referenced from \
                 the document-root `oneOf`. Either (a) re-add the entry to \
                 the rewritten `oneOf` in `rewrite_document_root_one_of`, \
                 or (b) remove it from `HAND_MAINTAINED_ROOT_ENVELOPES` \
                 because the migration to a typed `FallowOutput` variant \
                 has landed. Root `oneOf` refs today: {:?}",
                refs.iter().collect::<Vec<_>>(),
            );
        }
    }

    /// The document-root `oneOf` MUST always reference `FallowOutput` as
    /// its first entry plus the bare-array `CodeClimateOutput` branch.
    /// Catches accidental removal of either reference by a future
    /// `rewrite_document_root_one_of` edit.
    #[test]
    fn root_one_of_carries_fallow_output_and_codeclimate() {
        let document: Value = serde_json::from_str(COMMITTED_SCHEMA)
            .expect("committed docs/output-schema.json must parse");
        let one_of = document
            .get("oneOf")
            .and_then(Value::as_array)
            .expect("committed schema must carry a root-level `oneOf`");

        let mut refs: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for entry in one_of {
            if let Some(reference) = entry.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/definitions/")
            {
                refs.insert(name.to_string());
            }
        }

        assert!(
            refs.contains("FallowOutput"),
            "document-root `oneOf` must reference `#/definitions/FallowOutput`; \
             found refs: {:?}",
            refs.iter().collect::<Vec<_>>(),
        );
        assert!(
            refs.contains("CodeClimateOutput"),
            "document-root `oneOf` must reference `#/definitions/CodeClimateOutput` \
             as a sibling root branch (the bare-array spec form); found refs: {:?}",
            refs.iter().collect::<Vec<_>>(),
        );
    }
}
