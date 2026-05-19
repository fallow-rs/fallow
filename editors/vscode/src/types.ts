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

export type {
  AddToConfigAction,
  AttributedCloneGroup,
  AttributedCloneGroupFinding,
  AuditOutput,
  BoundaryViolationFinding,
  CheckOutput,
  CheckSummary,
  CircularDependencyFinding,
  CloneFamily,
  CloneFamilyAction,
  CloneFamilyFinding,
  CloneGroup,
  CloneGroupAction,
  CloneGroupFinding,
  CloneInstance,
  CombinedOutput,
  CoverageAnalyzeOutput,
  DuplicateExportFinding,
  DuplicateLocation,
  DupesOutput,
  DupesReportPayload,
  DuplicationReport,
  DuplicationStats,
  EmptyCatalogGroupFinding,
  EntryPoints,
  FixAction as SuggestionFixAction,
  HealthOutput,
  ImportSite,
  IssueAction,
  MisconfiguredDependencyOverrideFinding,
  PrivateTypeLeakFinding,
  RefactoringSuggestion,
  StaleSuppression,
  SuppressFileAction,
  SuppressLineAction,
  TestOnlyDependencyFinding,
  TypeOnlyDependencyFinding,
  UnlistedDependencyFinding,
  UnresolvedCatalogReferenceFinding,
  UnresolvedImportFinding,
  UnusedCatalogEntryFinding,
  UnusedClassMemberFinding,
  UnusedDependencyFinding,
  UnusedDependencyOverrideFinding,
  UnusedDevDependencyFinding,
  UnusedEnumMemberFinding,
  UnusedExportFinding,
  UnusedFileFinding,
  UnusedOptionalDependencyFinding,
  UnusedTypeFinding,
} from "./generated/output-contract.js";

// Backwards-compat aliases for downstream consumers that import the
// pre-#384-item-1 bare type names. The wire shape is byte-identical: each
// wrapper flattens the bare finding's fields and adds `actions` plus
// optional `introduced`. New code should prefer the `*Finding` names above.
//
// Dupes wrappers (`CloneGroup`, `CloneFamily`, `AttributedCloneGroup`,
// `DuplicationReport`) are aliased upstream in the generated
// `output-contract.d.ts` via `editors/vscode/scripts/codegen-types.mjs`'s
// `FLATTEN_DEDUPED_ALIASES` table, so they're re-exported from the
// `export type {...}` block above rather than redeclared here.
import type {
  BoundaryViolationFinding,
  CircularDependencyFinding,
  DuplicateExportFinding,
  EmptyCatalogGroupFinding,
  MisconfiguredDependencyOverrideFinding,
  PrivateTypeLeakFinding,
  TestOnlyDependencyFinding,
  TypeOnlyDependencyFinding,
  UnlistedDependencyFinding,
  UnresolvedCatalogReferenceFinding,
  UnresolvedImportFinding,
  UnusedCatalogEntryFinding,
  UnusedClassMemberFinding,
  UnusedDependencyFinding,
  UnusedDependencyOverrideFinding,
  UnusedDevDependencyFinding,
  UnusedEnumMemberFinding,
  UnusedExportFinding,
  UnusedFileFinding,
  UnusedOptionalDependencyFinding,
} from "./generated/output-contract.js";
export type BoundaryViolation = BoundaryViolationFinding;
export type CircularDependency = CircularDependencyFinding;
export type DuplicateExport = DuplicateExportFinding;
export type EmptyCatalogGroup = EmptyCatalogGroupFinding;
export type MisconfiguredDependencyOverride =
  MisconfiguredDependencyOverrideFinding;
export type PrivateTypeLeak = PrivateTypeLeakFinding;
export type TestOnlyDependency = TestOnlyDependencyFinding;
export type TypeOnlyDependency = TypeOnlyDependencyFinding;
export type UnlistedDependency = UnlistedDependencyFinding;
export type UnresolvedCatalogReference = UnresolvedCatalogReferenceFinding;
export type UnresolvedImport = UnresolvedImportFinding;
export type UnusedCatalogEntry = UnusedCatalogEntryFinding;
export type UnusedDependency =
  | UnusedDependencyFinding
  | UnusedDevDependencyFinding
  | UnusedOptionalDependencyFinding;
export type UnusedDependencyOverride = UnusedDependencyOverrideFinding;
export type UnusedExport = UnusedExportFinding;
export type UnusedFile = UnusedFileFinding;
export type UnusedMember = UnusedClassMemberFinding | UnusedEnumMemberFinding;

export type { CheckOutput as FallowCheckResult } from "./generated/output-contract.js";
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

export type { DuplicationMode, IssueTypeConfig, TraceLevel } from "./settings.js";
export type { IssueCategory } from "./labels.js";
export { ISSUE_CATEGORY_LABELS } from "./labels.js";
export type { FallowFixResult, FixAction } from "./fix-types.js";
