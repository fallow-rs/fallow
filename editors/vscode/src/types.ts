export interface IssueTypeConfig {
  readonly "unused-files": boolean;
  readonly "unused-exports": boolean;
  readonly "unused-types": boolean;
  readonly "private-type-leaks": boolean;
  readonly "unused-dependencies": boolean;
  readonly "unused-dev-dependencies": boolean;
  readonly "unused-optional-dependencies": boolean;
  readonly "unused-enum-members": boolean;
  readonly "unused-class-members": boolean;
  readonly "unresolved-imports": boolean;
  readonly "unlisted-dependencies": boolean;
  readonly "duplicate-exports": boolean;
  readonly "type-only-dependencies": boolean;
  readonly "test-only-dependencies": boolean;
  readonly "circular-dependencies": boolean;
  readonly "boundary-violation": boolean;
  readonly "stale-suppressions": boolean;
  readonly "unused-catalog-entries": boolean;
}

export type DuplicationMode = "strict" | "mild" | "weak" | "semantic";

export type TraceLevel = "off" | "messages" | "verbose";

export interface FallowCheckResult {
  readonly schema_version?: number;
  readonly version?: string;
  readonly elapsed_ms?: number;
  readonly total_issues?: number;
  readonly unused_files: ReadonlyArray<UnusedFile>;
  readonly unused_exports: ReadonlyArray<UnusedExport>;
  readonly unused_types: ReadonlyArray<UnusedExport>;
  readonly private_type_leaks?: ReadonlyArray<PrivateTypeLeak>;
  readonly unused_dependencies: ReadonlyArray<UnusedDependency>;
  readonly unused_dev_dependencies: ReadonlyArray<UnusedDependency>;
  readonly unused_optional_dependencies?: ReadonlyArray<UnusedDependency>;
  readonly unused_enum_members: ReadonlyArray<UnusedMember>;
  readonly unused_class_members: ReadonlyArray<UnusedMember>;
  readonly unresolved_imports: ReadonlyArray<UnresolvedImport>;
  readonly unlisted_dependencies: ReadonlyArray<UnlistedDependency>;
  readonly duplicate_exports: ReadonlyArray<DuplicateExport>;
  readonly type_only_dependencies?: ReadonlyArray<TypeOnlyDependency>;
  readonly test_only_dependencies?: ReadonlyArray<TestOnlyDependency>;
  readonly circular_dependencies?: ReadonlyArray<CircularDependency>;
  readonly boundary_violations?: ReadonlyArray<BoundaryViolation>;
  readonly stale_suppressions?: ReadonlyArray<StaleSuppression>;
  readonly unused_catalog_entries?: ReadonlyArray<UnusedCatalogEntry>;
  readonly entry_points?: EntryPoints;
  readonly summary?: CheckSummary;
}

interface UnusedFile {
  readonly path: string;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface UnusedExport {
  readonly path: string;
  readonly export_name: string;
  readonly is_type_only: boolean;
  readonly line: number;
  readonly col: number;
  readonly span_start: number;
  readonly is_re_export: boolean;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface PrivateTypeLeak {
  readonly path: string;
  readonly export_name: string;
  readonly type_name: string;
  readonly line: number;
  readonly col: number;
  readonly span_start: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

type DependencyLocation =
  | "dependencies"
  | "devDependencies"
  | "optionalDependencies";

interface UnusedDependency {
  readonly package_name: string;
  readonly location: DependencyLocation;
  readonly path: string;
  readonly line: number;
  readonly used_in_workspaces?: ReadonlyArray<string>;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface UnusedMember {
  readonly path: string;
  readonly parent_name: string;
  readonly member_name: string;
  readonly kind: "EnumMember" | "ClassMethod" | "ClassProperty" | "NamespaceMember";
  readonly line: number;
  readonly col: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface UnresolvedImport {
  readonly path: string;
  readonly specifier: string;
  readonly line: number;
  readonly col: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface ImportSite {
  readonly path: string;
  readonly line: number;
  readonly col: number;
}

interface UnlistedDependency {
  readonly package_name: string;
  readonly imported_from: ReadonlyArray<ImportSite>;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface DuplicateLocation {
  readonly path: string;
  readonly line: number;
  readonly col: number;
}

interface DuplicateExport {
  readonly export_name: string;
  readonly locations: ReadonlyArray<DuplicateLocation>;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface TypeOnlyDependency {
  readonly package_name: string;
  readonly path: string;
  readonly line: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface TestOnlyDependency {
  readonly package_name: string;
  readonly path: string;
  readonly line: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface CircularDependency {
  readonly files: ReadonlyArray<string>;
  readonly length: number;
  readonly line: number;
  readonly col: number;
  readonly is_cross_package?: boolean;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface BoundaryViolation {
  readonly from_path: string;
  readonly to_path: string;
  readonly from_zone: string;
  readonly to_zone: string;
  readonly import_specifier: string;
  readonly line: number;
  readonly col: number;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

interface StaleSuppression {
  readonly path: string;
  readonly line: number;
  readonly col: number;
  readonly origin: SuppressionOrigin;
  readonly introduced?: boolean;
}

interface UnusedCatalogEntry {
  readonly entry_name: string;
  readonly catalog_name: string;
  readonly path: string;
  readonly line: number;
  readonly hardcoded_consumers?: ReadonlyArray<string>;
  readonly actions: ReadonlyArray<FindingAction>;
  readonly introduced?: boolean;
}

type SuppressionOrigin =
  | {
      readonly type: "comment";
      readonly issue_kind?: string;
      readonly is_file_level?: boolean;
    }
  | {
      readonly type: "jsdoc_tag";
      readonly export_name: string;
    };

interface EntryPoints {
  readonly total: number;
  readonly sources: Readonly<Record<string, number>>;
}

interface CheckSummary {
  readonly total_issues?: number;
  readonly unused_files?: number;
  readonly unused_exports?: number;
  readonly unused_types?: number;
  readonly private_type_leaks?: number;
  readonly unused_dependencies?: number;
  readonly unused_dev_dependencies?: number;
  readonly unused_enum_members?: number;
  readonly unused_class_members?: number;
  readonly unresolved_imports?: number;
  readonly unlisted_dependencies?: number;
  readonly duplicate_exports?: number;
  readonly type_only_dependencies?: number;
  readonly test_only_dependencies?: number;
  readonly circular_dependencies?: number;
  readonly boundary_violations?: number;
  readonly stale_suppressions?: number;
  readonly unused_catalog_entries?: number;
}

type FindingAction =
  | FindingFixAction
  | SuppressLineAction
  | SuppressFileAction
  | AddToConfigAction;

interface FindingFixAction {
  readonly type:
    | "remove-export"
    | "delete-file"
    | "remove-dependency"
    | "move-dependency"
    | "remove-enum-member"
    | "remove-class-member"
    | "resolve-import"
    | "install-dependency"
    | "remove-duplicate"
    | "move-to-dev"
    | "refactor-cycle"
    | "refactor-boundary"
    | "export-type";
  readonly auto_fixable: boolean;
  readonly description: string;
  readonly note?: string;
}

interface SuppressLineAction {
  readonly type: "suppress-line";
  readonly auto_fixable: false;
  readonly description: string;
  readonly comment: string;
  readonly scope?: "per-location";
}

interface SuppressFileAction {
  readonly type: "suppress-file";
  readonly auto_fixable: false;
  readonly description: string;
  readonly comment: string;
}

interface AddToConfigAction {
  readonly type: "add-to-config";
  readonly auto_fixable: false;
  readonly description: string;
  readonly config_key: string;
  readonly value:
    | string
    | ReadonlyArray<{
        readonly file: string;
        readonly exports: ReadonlyArray<string>;
      }>;
  readonly value_schema?: string;
}

export interface FallowDupesResult {
  readonly clone_groups: ReadonlyArray<CloneGroup>;
  readonly clone_families: ReadonlyArray<CloneFamily>;
  readonly stats: DupesStats;
}

export interface FallowCombinedResult {
  readonly schema_version?: number;
  readonly version?: string;
  readonly elapsed_ms?: number;
  readonly check?: FallowCheckResult;
  readonly dupes?: FallowDupesResult;
}

export interface CloneGroup {
  readonly instances: ReadonlyArray<CloneInstance>;
  readonly token_count: number;
  readonly line_count: number;
  readonly actions?: ReadonlyArray<CloneGroupAction>;
  readonly introduced?: boolean;
}

interface CloneInstance {
  readonly file: string;
  readonly start_line: number;
  readonly end_line: number;
  readonly start_col: number;
  readonly end_col: number;
  readonly fragment: string;
}

interface CloneFamily {
  readonly files: ReadonlyArray<string>;
  readonly groups: ReadonlyArray<CloneGroup>;
  readonly total_duplicated_lines: number;
  readonly total_duplicated_tokens: number;
  readonly suggestions: ReadonlyArray<RefactoringSuggestion>;
  readonly actions?: ReadonlyArray<CloneFamilyAction>;
}

interface RefactoringSuggestion {
  readonly kind: "ExtractFunction" | "ExtractModule";
  readonly description: string;
  readonly estimated_savings: number;
}

interface DupesStats {
  readonly total_files: number;
  readonly files_with_clones: number;
  readonly total_lines: number;
  readonly duplicated_lines: number;
  readonly total_tokens: number;
  readonly duplicated_tokens: number;
  readonly clone_groups: number;
  readonly clone_instances: number;
  readonly duplication_percentage: number;
}

interface CloneGroupAction {
  readonly type: "extract-shared" | "suppress-line";
  readonly auto_fixable: boolean;
  readonly description: string;
  readonly comment?: string;
}

interface CloneFamilyAction {
  readonly type: "extract-shared" | "apply-suggestion" | "suppress-line";
  readonly auto_fixable: boolean;
  readonly description: string;
  readonly note?: string;
  readonly comment?: string;
}

export interface FallowFixResult {
  readonly dry_run: boolean;
  readonly fixes: ReadonlyArray<FixAction>;
  readonly total_fixed: number;
}

export interface FixAction {
  readonly type: string;
  readonly path?: string;
  readonly line?: number;
  readonly name?: string;
  readonly package?: string;
  readonly location?: string;
  readonly file?: string;
}

export type IssueCategory =
  | "unused-files"
  | "unused-exports"
  | "unused-types"
  | "private-type-leaks"
  | "unused-dependencies"
  | "unused-dev-dependencies"
  | "unused-optional-dependencies"
  | "unused-enum-members"
  | "unused-class-members"
  | "unresolved-imports"
  | "unlisted-dependencies"
  | "duplicate-exports"
  | "type-only-dependencies"
  | "test-only-dependencies"
  | "circular-dependencies"
  | "boundary-violation"
  | "stale-suppressions"
  | "unused-catalog-entries";

export const ISSUE_CATEGORY_LABELS: Record<IssueCategory, string> = {
  "unused-files": "Unused Files",
  "unused-exports": "Unused Exports",
  "unused-types": "Unused Types",
  "private-type-leaks": "Private Type Leaks",
  "unused-dependencies": "Unused Dependencies",
  "unused-dev-dependencies": "Unused Dev Dependencies",
  "unused-optional-dependencies": "Unused Optional Dependencies",
  "unused-enum-members": "Unused Enum Members",
  "unused-class-members": "Unused Class Members",
  "unresolved-imports": "Unresolved Imports",
  "unlisted-dependencies": "Unlisted Dependencies",
  "duplicate-exports": "Duplicate Exports",
  "type-only-dependencies": "Type-Only Dependencies",
  "test-only-dependencies": "Test-Only Dependencies",
  "circular-dependencies": "Circular Dependencies",
  "boundary-violation": "Boundary Violations",
  "stale-suppressions": "Stale Suppressions",
  "unused-catalog-entries": "Unused Catalog Entries",
};
