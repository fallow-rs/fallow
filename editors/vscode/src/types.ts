/**
 * Public type surface for the extension. Re-exports schema-derived types from
 * `./generated/output-contract.js` plus hand-written types from `./settings`,
 * `./labels`, and `./fix-types`.
 *
 * Schema-derived contract types are generated from `docs/output-schema.json`
 * by `scripts/codegen-types.mjs`. Edit the schema (and the upstream Rust
 * struct), regenerate, commit. See the banner of
 * `src/generated/output-contract.d.ts` for the full recipe.
 *
 * The `Fallow*Result` aliases below preserve the historical names used by
 * existing consumers. New code should prefer the schema-derived names
 * (`CheckOutput`, `DupesOutput`, `CombinedOutput`).
 */

// Bare-name backwards-compat aliases (`UnusedExport`, `CloneGroup`, ...) and
// per-alias rationale live in the generated `output-contract.d.ts` under the
// `// Backwards-compat aliases` section. They are sourced from the same
// `export type { ... }` block below so the published `fallow/types` subpath
// carries the v2.x stable surface. Public-consumer policy:
// `docs/backwards-compatibility.md`.
export type {
  AddToConfigAction,
  AttributedCloneGroup,
  AttributedCloneGroupFinding,
  AuditGate,
  AuditOutput,
  AuditVerdict,
  BoundaryViolation,
  BoundaryViolationFinding,
  CheckOutput,
  CheckSummary,
  CircularDependency,
  CircularDependencyFinding,
  CloneFamily,
  CloneFamilyAction,
  CloneFamilyFinding,
  CloneGroup,
  CloneGroupAction,
  CloneGroupFinding,
  CloneInstance,
  CombinedOutput,
  ComplexityContribution,
  ComplexityContributionKind,
  CoverageAnalyzeOutput,
  RuntimeCoverageReport,
  RuntimeCoverageHotPath,
  RuntimeCoverageFinding,
  RuntimeCoverageVerdict,
  RuntimeCoverageConfidence,
  RuntimeCoverageWatermark,
  DuplicateExport,
  DuplicateExportFinding,
  DuplicateLocation,
  DupesOutput,
  DupesReportPayload,
  DuplicationReport,
  DuplicationStats,
  EmptyCatalogGroup,
  EmptyCatalogGroupFinding,
  EntryPoints,
  FindingSeverity,
  FixAction as SuggestionFixAction,
  FallowOutput,
  HealthFinding,
  HealthOutput,
  HealthReport,
  HealthScore,
  HealthScorePenalties,
  HotspotFinding,
  ImportSite,
  IssueAction,
  MisconfiguredDependencyOverride,
  MisconfiguredDependencyOverrideFinding,
  PrivateTypeLeak,
  PrivateTypeLeakFinding,
  RefactoringSuggestion,
  RefactoringTargetFinding,
  SecurityFinding,
  SecurityFindingKind,
  SecurityOutput,
  SecurityReachability,
  StaleSuppression,
  SuppressFileAction,
  SuppressLineAction,
  TestOnlyDependency,
  TestOnlyDependencyFinding,
  TraceHop,
  TraceHopRole,
  TypeOnlyDependency,
  TypeOnlyDependencyFinding,
  UnlistedDependency,
  UnlistedDependencyFinding,
  UnresolvedCatalogReference,
  UnresolvedCatalogReferenceFinding,
  UnresolvedImport,
  UnresolvedImportFinding,
  UnusedCatalogEntry,
  UnusedCatalogEntryFinding,
  UnusedClassMemberFinding,
  UnusedDependency,
  UnusedDependencyFinding,
  UnusedDependencyOverride,
  UnusedDependencyOverrideFinding,
  UnusedDevDependencyFinding,
  UnusedEnumMemberFinding,
  UnusedExport,
  UnusedExportFinding,
  UnusedFile,
  UnusedFileFinding,
  UnusedMember,
  UnusedOptionalDependencyFinding,
  UnusedTypeFinding,
  WorkspaceInfo,
  WorkspacesOutput,
} from "./generated/output-contract.js";

import type { FallowOutput } from "./generated/output-contract.js";

export type { CheckOutput as FallowCheckResult } from "./generated/output-contract.js";
export type FallowInspectResult = Extract<FallowOutput, { kind: "inspect_target" }>;
// The VS Code extension reads dupes only via the combined invocation
// (`fallow --format json`), where `combined.dupes` is the typed
// `DupesReportPayload` body (introduced in #409), NOT the full
// `DupesOutput` envelope with schema_version / version / elapsed_ms.
// Aliasing `FallowDupesResult` to `DupesReportPayload` keeps every
// downstream consumer's existing usage (clone_groups, clone_families,
// stats, mirrored_directories) honest; the inner `clone_groups[]` and
// `clone_families[]` items are now `CloneGroupFinding` /
// `CloneFamilyFinding` (each carrying typed actions[]). If a future VS
// Code feature calls `fallow dupes` standalone, switch its return type
// to the full `DupesOutput` instead.
export type { DupesReportPayload as FallowDupesResult } from "./generated/output-contract.js";
export type { CombinedOutput as FallowCombinedResult } from "./generated/output-contract.js";

export type {
  DiagnosticSeveritySetting,
  DuplicationMode,
  IssueTypeConfig,
  TraceLevel,
} from "./settings.js";
export type { IssueCategory } from "./labels.js";
export { ISSUE_CATEGORY_LABELS } from "./labels.js";
export type { FallowFixResult, FixAction } from "./fix-types.js";
export type {
  LicenseActionResult,
  LicenseErrorJson,
  LicenseKind,
  LicenseParseResult,
  LicenseState,
  LicenseStatusJson,
} from "./license-types.js";
