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
    ContributorEntry, ContributorIdentifierFormat, CoverageGapSummary, CoverageGaps, CoverageModel,
    CoverageTier, ExceededThreshold, FileHealthScore, FindingSeverity, HealthActionsMeta,
    HealthFinding, HealthScore, HealthScorePenalties, HealthSummary, HealthTrend, HotspotEntry,
    HotspotSummary, LargeFunctionEntry, OwnershipMetrics, RecommendationCategory,
    RefactoringTarget, RiskProfile, RuntimeCoverageReport, TargetThresholds, TrendCount,
    UntestedExport, UntestedFile, VitalSigns, VitalSignsCounts,
};
use fallow_cli::output_envelope::{
    AuditCommand, AuditOutput, CheckGroupedEntry, CheckGroupedOutput, CheckOutput,
    CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation,
    CodeClimateOutput, CodeClimateSeverity, CombinedOutput, CoverageSetupFileToEdit,
    CoverageSetupFramework, CoverageSetupMember, CoverageSetupOutput, CoverageSetupPackageManager,
    CoverageSetupRuntimeTarget, CoverageSetupSchemaVersion, CoverageSetupSnippet, DupesOutput,
    ExplainOutput, GitHubReviewComment, GitHubReviewSide, GitLabReviewComment,
    GitLabReviewPosition, GitLabReviewPositionType, GroupByMode, HealthOutput,
    ReviewCheckConclusion, ReviewComment, ReviewEnvelopeEvent, ReviewEnvelopeMeta,
    ReviewEnvelopeOutput, ReviewEnvelopeSchema, ReviewProvider, ReviewReconcileOutput,
    ReviewReconcileSchema,
};
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
        // crates/cli/src/health_types/ - health output subtree
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
        "HotspotSummary",
        "LargeFunctionEntry",
        "OwnershipMetrics",
        "RefactoringTarget",
        "RiskProfile",
        "RuntimeCoverageReport",
        "TargetThresholds",
        "TrendCount",
        "UntestedExport",
        "UntestedFile",
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
        "ReviewReconcileOutput",
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
    // Each entry MUST appear in `actions_for_issue_type` (dead-code findings)
    // or `inject_health_actions` (health findings) in
    // `crates/cli/src/report/json.rs`. `StaleSuppression` is intentionally
    // excluded: the JSON layer does not inject actions on stale_suppressions
    // today, and the committed schema matches that.
    &[
        // Dead-code findings (actions[] -> IssueAction, with `introduced`)
        "BoundaryViolation",
        "CircularDependency",
        "DuplicateExport",
        "EmptyCatalogGroup",
        "MisconfiguredDependencyOverride",
        "PrivateTypeLeak",
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
        // Health findings (actions[] -> per-finding action wrapper).
        // `introduced` attaches per `finding_augmentation` below: HealthFinding
        // is audit-aware (carries `introduced`), HotspotEntry and
        // RefactoringTarget are not.
        "HealthFinding",
        "HotspotEntry",
        "RefactoringTarget",
        // Coverage-gap items (`coverage_gaps.files[]` and
        // `coverage_gaps.exports[]`). `inject_health_actions` walks both
        // arrays and appends an `actions` field to every item, but the
        // Rust source structs do not carry the field, so the schema
        // augmentation pass grafts it on per `finding_augmentation`.
        // Neither flows through `fallow audit`, so `introduced` is
        // omitted.
        "UntestedExport",
        "UntestedFile",
        // Duplication findings (`clone_groups[]` and `clone_families[]`).
        // `inject_dupes_actions` in `crates/cli/src/report/json.rs` walks
        // both arrays and appends an `actions` field to every item; the
        // Rust source structs (`CloneGroup`, `CloneFamily` in
        // `crates/core/src/duplicates/types.rs`) do not carry the field.
        // `CloneGroup` carries the audit `introduced` flag because
        // `fallow audit` attributes clone groups; `CloneFamily` does not.
        "CloneFamily",
        "CloneGroup",
    ]
}

/// Per-finding override for `augment_finding_definition`.
///
/// The default augmentation attaches `actions: array<IssueAction>` and an
/// `introduced` audit-mode flag. Health findings carry a typed action wrapper
/// (`HealthFindingAction`, `HotspotAction`, `RefactoringTargetAction`), and
/// only `HealthFinding` carries the audit `introduced` flag today.
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

/// Pick the augmentation for a specific finding. Health findings use typed
/// per-finding action wrappers and (with the exception of `HealthFinding`)
/// skip the audit `introduced` flag because hotspot ranking and refactoring
/// targets do not run through `fallow audit`'s introduced-vs-inherited
/// classifier.
fn finding_augmentation(name: &str) -> FindingAugmentation {
    match name {
        "HealthFinding" => FindingAugmentation {
            actions_item_ref: "#/definitions/HealthFindingAction",
            include_introduced: true,
        },
        "HotspotEntry" => FindingAugmentation {
            actions_item_ref: "#/definitions/HotspotAction",
            include_introduced: false,
        },
        "RefactoringTarget" => FindingAugmentation {
            actions_item_ref: "#/definitions/RefactoringTargetAction",
            include_introduced: false,
        },
        "UntestedFile" => FindingAugmentation {
            actions_item_ref: "#/definitions/UntestedFileAction",
            include_introduced: false,
        },
        "UntestedExport" => FindingAugmentation {
            actions_item_ref: "#/definitions/UntestedExportAction",
            include_introduced: false,
        },
        "CloneFamily" => FindingAugmentation {
            actions_item_ref: "#/definitions/CloneFamilyAction",
            include_introduced: false,
        },
        "CloneGroup" => FindingAugmentation {
            actions_item_ref: "#/definitions/CloneGroupAction",
            include_introduced: true,
        },
        _ => DEFAULT_FINDING_AUGMENTATION,
    }
}

/// Build derived schemas for every in-scope type using one shared generator.
///
/// Registering each type as a subschema (rather than a root schema) collects
/// every transitively-referenced definition into a single map keyed by the
/// Rust type name, which we then merge into the schema's `definitions`.
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
    let _ = generator.subschema_for::<DuplicationStats>();

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

    // Health output subtree (crates/cli/src/health_types/).
    let _ = generator.subschema_for::<HealthSummary>();
    let _ = generator.subschema_for::<HealthFinding>();
    let _ = generator.subschema_for::<ExceededThreshold>();
    let _ = generator.subschema_for::<FindingSeverity>();
    let _ = generator.subschema_for::<CoverageTier>();
    let _ = generator.subschema_for::<CoverageModel>();
    let _ = generator.subschema_for::<LargeFunctionEntry>();
    let _ = generator.subschema_for::<FileHealthScore>();
    let _ = generator.subschema_for::<HotspotEntry>();
    let _ = generator.subschema_for::<HotspotSummary>();
    let _ = generator.subschema_for::<OwnershipMetrics>();
    let _ = generator.subschema_for::<ContributorEntry>();
    let _ = generator.subschema_for::<ContributorIdentifierFormat>();
    let _ = generator.subschema_for::<RefactoringTarget>();
    let _ = generator.subschema_for::<RecommendationCategory>();
    let _ = generator.subschema_for::<TargetThresholds>();
    let _ = generator.subschema_for::<HealthTrend>();
    let _ = generator.subschema_for::<TrendCount>();
    let _ = generator.subschema_for::<CoverageGaps>();
    let _ = generator.subschema_for::<CoverageGapSummary>();
    let _ = generator.subschema_for::<UntestedFile>();
    let _ = generator.subschema_for::<UntestedExport>();
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

    // Per-command envelope structs (crates/cli/src/output_envelope.rs).
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
        if *name == "RuntimeCoverageReport" {
            augment_runtime_coverage_report(&mut value)?;
        }
        definitions.insert((*name).to_string(), value);
    }

    // Schemars produces transitively-referenced helper definitions for every
    // typed enum / payload subtype on the in-scope structs (`FixActionType`,
    // `SuppressAutoFixable`, the kebab-case kind enums, `DependencyLocation`,
    // `MemberKind`, `CoverageSetupFramework`, etc.). After Phase 8 every
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

    Ok(document)
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

/// Add the runtime-coverage `schema_version` envelope marker to the derived
/// `RuntimeCoverageReport` schema.
///
/// The CLI injects `runtime_coverage.schema_version: "1"` into every JSON
/// output that carries a runtime coverage block (see
/// `crates/cli/src/report/json.rs::inject_runtime_coverage_report_schema_version`).
/// The Rust source struct does not carry a matching field today, so the
/// derived schema would otherwise miss it and the drift gate would fire.
/// Graft the property + `required` entry on derivation so the public
/// contract stays in lock-step with the wire.
///
/// MAINTENANCE: the `enum: ["1"]` constraint is tightly coupled to
/// `RUNTIME_COVERAGE_SCHEMA_VERSION` in
/// `crates/cli/src/report/json.rs`. Bumping that constant requires
/// updating the enum list here in the same PR; otherwise the drift gate
/// stays green while the emitted document quietly disagrees with the
/// wire.
///
/// Idempotent: if a future PR adds a typed `schema_version` field to
/// `RuntimeCoverageReport`, schemars derives the property natively and the
/// augmentation step skips.
fn augment_runtime_coverage_report(value: &mut Value) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "RuntimeCoverageReport definition is not a JSON object".to_string())?;

    let properties = object
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    let properties = properties
        .as_object_mut()
        .ok_or_else(|| "RuntimeCoverageReport `properties` is not a JSON object".to_string())?;

    if !properties.contains_key("schema_version") {
        properties.insert(
            "schema_version".to_string(),
            serde_json::json!({
                "type": "string",
                "enum": ["1"],
                "description": "Runtime coverage JSON contract version. This is scoped to the runtime_coverage block and is independent of the top-level fallow JSON schema_version."
            }),
        );
    }

    let required = object
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(arr) = required
        && !arr.iter().any(|v| v.as_str() == Some("schema_version"))
    {
        arr.push(Value::String("schema_version".to_string()));
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
            for child in map.values_mut() {
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
    //! The test reads the committed schema's `definitions` block and the
    //! derived schemas for every name in `derived_definition_names()`,
    //! normalizes both sides to a canonical form that erases the documented
    //! cosmetic differences (doc-comment prose, schemars-style `nullable`
    //! integer formats, `oneOf` vs `anyOf`, single-arm `allOf` wrappers), and
    //! asserts the result is structurally equal.
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
                for child in map.values_mut() {
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

    fn derived_definitions_for_drift() -> Map<String, Value> {
        let raw = derived_definitions();
        let mut out = Map::new();
        let finding_names: rustc_hash::FxHashSet<&'static str> =
            finding_definition_names().iter().copied().collect();
        for name in derived_definition_names() {
            let derived_schema = raw
                .get(*name)
                .unwrap_or_else(|| panic!("derived schema missing for '{name}'"));
            let mut value = derived_schema.clone();
            normalize_schema(&mut value);
            if finding_names.contains(name) {
                augment_finding_definition(&mut value, finding_augmentation(name))
                    .expect("augment_finding_definition must not fail");
            }
            if *name == "RuntimeCoverageReport" {
                augment_runtime_coverage_report(&mut value)
                    .expect("augment_runtime_coverage_report must not fail");
            }
            out.insert((*name).to_string(), value);
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
        // field natively; the runtime `inject_health_actions` post-pass still
        // populates it. Permit `actions` / `introduced` to differ between
        // sides without firing the gate; everything else must match.
        const AUGMENTATION_KEYS: &[&str] = &["actions", "introduced"];

        let mut failures: Vec<String> = Vec::new();
        for name in derived_definition_names() {
            let Some(committed_entry) = committed.get(*name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`. Add a stub entry to `definitions` (the drift test only compares; it does not insert)."
                ));
                continue;
            };
            let derived_entry = derived
                .get(*name)
                .expect("derived map covers every registered name (asserted by earlier test)");

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

        for name in derived_definition_names() {
            let Some(committed_entry) = committed.get(*name) else {
                continue;
            };
            let Some(derived_entry) = derived.get(*name) else {
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
        for name in derived_definition_names() {
            let Some(committed_value) = committed.get(*name) else {
                failures.push(format!(
                    "definition `{name}` is missing from `docs/output-schema.json`."
                ));
                continue;
            };
            let derived_entry = canonicalize(
                derived
                    .get(*name)
                    .expect("derived map covers every registered name")
                    .clone(),
            );
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
        assert!(
            failures.is_empty(),
            "{} structural drift issue{}:\n\n{}",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n\n"),
        );
    }
}
