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
  AuditOutput,
  BoundaryViolation,
  CheckOutput,
  CheckSummary,
  CircularDependency,
  CloneFamily,
  CloneFamilyAction,
  CloneGroup,
  CloneGroupAction,
  CloneInstance,
  CombinedOutput,
  DuplicateExport,
  DuplicateLocation,
  DupesOutput,
  DuplicationStats,
  EntryPoints,
  FixAction as SuggestionFixAction,
  HealthOutput,
  ImportSite,
  IssueAction,
  PrivateTypeLeak,
  RefactoringSuggestion,
  StaleSuppression,
  SuppressFileAction,
  SuppressLineAction,
  TestOnlyDependency,
  TypeOnlyDependency,
  UnlistedDependency,
  UnresolvedImport,
  UnusedCatalogEntry,
  UnusedDependency,
  UnusedDependencyOverride,
  MisconfiguredDependencyOverride,
  UnusedExport,
  UnusedFile,
  UnusedMember,
} from "./generated/output-contract.js";

export type { CheckOutput as FallowCheckResult } from "./generated/output-contract.js";
// The VS Code extension reads dupes only via the combined invocation
// (`fallow --format json`), where `combined.dupes` is the bare
// `DuplicationReport` body, NOT the full `DupesOutput` envelope with
// schema_version / version / elapsed_ms. Aliasing `FallowDupesResult`
// to `DuplicationReport` keeps every downstream consumer's existing
// usage (clone_groups, clone_families, stats, mirrored_directories)
// honest. If a future VS Code feature calls `fallow dupes` standalone,
// switch its return type to the full `DupesOutput` instead.
export type { DuplicationReport as FallowDupesResult } from "./generated/output-contract.js";
export type { CombinedOutput as FallowCombinedResult } from "./generated/output-contract.js";

export type { DuplicationMode, IssueTypeConfig, TraceLevel } from "./settings.js";
export type { IssueCategory } from "./labels.js";
export { ISSUE_CATEGORY_LABELS } from "./labels.js";
export type { FallowFixResult, FixAction } from "./fix-types.js";
