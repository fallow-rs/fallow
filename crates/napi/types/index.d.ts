export interface AnalysisOptions {
  root?: string;
  configPath?: string;
  noCache?: boolean;
  threads?: number;
  diffFile?: string;
  production?: boolean;
  changedSince?: string;
  workspace?: string[];
  changedWorkspaces?: string;
  explain?: boolean;
  legacyEnvelope?: boolean;
}

export interface DeadCodeOptions extends AnalysisOptions {
  unusedFiles?: boolean;
  unusedExports?: boolean;
  unusedDeps?: boolean;
  unusedTypes?: boolean;
  privateTypeLeaks?: boolean;
  unusedEnumMembers?: boolean;
  unusedClassMembers?: boolean;
  unresolvedImports?: boolean;
  unlistedDeps?: boolean;
  duplicateExports?: boolean;
  circularDeps?: boolean;
  boundaryViolations?: boolean;
  staleSuppressions?: boolean;
  files?: string[];
  includeEntryExports?: boolean;
}

export type DuplicationMode = 'strict' | 'mild' | 'weak' | 'semantic';

export interface DuplicationOptions extends AnalysisOptions {
  mode?: DuplicationMode;
  minTokens?: number;
  minLines?: number;
  threshold?: number;
  skipLocal?: boolean;
  crossLanguage?: boolean;
  ignoreImports?: boolean;
  top?: number;
}

export interface FeatureFlagsOptions extends AnalysisOptions {
  top?: number;
}

export interface FeatureFlagFinding {
  flag_name: string;
  kind: string;
  confidence: string;
  path: string;
  line: number;
  col: number;
  [key: string]: unknown;
}

export interface FeatureFlagsReport {
  kind: 'feature-flags';
  schema_version: number;
  version: string;
  elapsed_ms: number;
  feature_flags: FeatureFlagFinding[];
  total_flags: number;
  _meta?: Record<string, unknown>;
}

export type ComplexitySort = 'cyclomatic' | 'cognitive' | 'lines' | 'severity';
export type OwnershipEmailMode = 'raw' | 'handle' | 'anonymized' | 'hash';
export type TargetEffort = 'low' | 'medium' | 'high';

export interface ComplexityOptions extends AnalysisOptions {
  maxCyclomatic?: number;
  maxCognitive?: number;
  maxCrap?: number;
  top?: number;
  sort?: ComplexitySort;
  complexityBreakdown?: boolean;
  complexity?: boolean;
  fileScores?: boolean;
  coverageGaps?: boolean;
  hotspots?: boolean;
  ownership?: boolean;
  ownershipEmails?: OwnershipEmailMode;
  targets?: boolean;
  css?: boolean;
  effort?: TargetEffort;
  score?: boolean;
  since?: string;
  minCommits?: number;
  coverage?: string;
  coverageRoot?: string;
}

export interface AnalysisAction {
  type?: string;
  auto_fixable?: boolean;
  description?: string;
  comment?: string;
  note?: string;
  [key: string]: unknown;
}

export interface DeadCodeSummary {
  total_issues: number;
  unused_files: number;
  unused_exports: number;
  unused_types: number;
  private_type_leaks: number;
  unused_dependencies: number;
  unused_enum_members: number;
  unused_class_members: number;
  unresolved_imports: number;
  unlisted_dependencies: number;
  duplicate_exports: number;
  type_only_dependencies: number;
  test_only_dependencies: number;
  circular_dependencies: number;
  boundary_violations: number;
  stale_suppressions: number;
}

export interface EntryPointSummary {
  total: number;
  sources: Record<string, number>;
}

export interface UnusedFileFinding {
  path: string;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface UnusedExportFinding {
  path: string;
  export_name: string;
  line: number;
  col: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface PrivateTypeLeakFinding {
  path: string;
  export_name: string;
  type_name: string;
  line: number;
  col: number;
  span_start?: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface UnusedDependencyFinding {
  path: string;
  package_name: string;
  line: number;
  col: number;
  used_in_workspaces?: string[];
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface UnusedMemberFinding {
  path: string;
  parent_name: string;
  member_name: string;
  line: number;
  col: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface UnresolvedImportFinding {
  path: string;
  specifier: string;
  line: number;
  col: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface UnlistedDependencyFinding {
  package_name: string;
  imported_from: Array<{ path: string; line: number; [key: string]: unknown }>;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface DuplicateExportFinding {
  export_name: string;
  locations: Array<{ path: string; line: number; [key: string]: unknown }>;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface TypeOnlyDependencyFinding {
  path: string;
  package_name: string;
  line: number;
  col: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface TestOnlyDependencyFinding {
  path: string;
  package_name: string;
  line: number;
  col: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface CircularDependencyFinding {
  files: string[];
  length: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface BoundaryViolationFinding {
  from_path: string;
  to_path: string;
  line: number;
  col: number;
  from_zone?: string;
  to_zone?: string;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface StaleSuppressionFinding {
  path: string;
  line: number;
  col: number;
  issue_kind?: string;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface DeadCodeReport {
  schema_version: number;
  version: string;
  elapsed_ms: number;
  total_issues: number;
  summary: DeadCodeSummary;
  entry_points?: EntryPointSummary;
  unused_files: UnusedFileFinding[];
  unused_exports: UnusedExportFinding[];
  unused_types: UnusedExportFinding[];
  private_type_leaks: PrivateTypeLeakFinding[];
  unused_dependencies: UnusedDependencyFinding[];
  unused_dev_dependencies: UnusedDependencyFinding[];
  unused_optional_dependencies: UnusedDependencyFinding[];
  unused_enum_members: UnusedMemberFinding[];
  unused_class_members: UnusedMemberFinding[];
  unresolved_imports: UnresolvedImportFinding[];
  unlisted_dependencies: UnlistedDependencyFinding[];
  duplicate_exports: DuplicateExportFinding[];
  type_only_dependencies: TypeOnlyDependencyFinding[];
  test_only_dependencies: TestOnlyDependencyFinding[];
  circular_dependencies: CircularDependencyFinding[];
  boundary_violations: BoundaryViolationFinding[];
  stale_suppressions: StaleSuppressionFinding[];
  _meta?: Record<string, unknown>;
}

export interface CloneInstance {
  file: string;
  start_line: number;
  end_line: number;
  start_col: number;
  end_col: number;
  fragment?: string;
  [key: string]: unknown;
}

export interface CloneGroup {
  instances: CloneInstance[];
  token_count: number;
  line_count: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface DuplicationStats {
  total_files: number;
  files_with_clones: number;
  total_lines: number;
  duplicated_lines: number;
  total_tokens: number;
  duplicated_tokens: number;
  clone_groups: number;
  clone_instances: number;
  duplication_percentage: number;
}

export interface DuplicationReport {
  schema_version: number;
  version: string;
  elapsed_ms: number;
  clone_groups: CloneGroup[];
  clone_families?: Record<string, unknown>[];
  mirrored_directories?: Record<string, unknown>[];
  stats: DuplicationStats;
  _meta?: Record<string, unknown>;
}

export interface HealthFinding {
  path: string;
  name: string;
  line: number;
  col: number;
  cyclomatic: number;
  cognitive: number;
  line_count: number;
  param_count: number;
  exceeded: string;
  severity: string;
  crap?: number;
  coverage_pct?: number;
  actions?: AnalysisAction[];
  [key: string]: unknown;
}

export interface FileHealthScore {
  path: string;
  fan_in: number;
  fan_out: number;
  dead_code_ratio: number;
  complexity_density: number;
  maintainability_index: number;
  total_cyclomatic: number;
  total_cognitive: number;
  function_count: number;
  lines: number;
  crap_max: number;
  crap_above_threshold: number;
  [key: string]: unknown;
}

export interface CoverageGaps {
  summary: {
    runtime_files: number;
    covered_files: number;
    file_coverage_pct: number;
    untested_files: number;
    untested_exports: number;
  };
  files?: Array<{ path: string; value_export_count: number; [key: string]: unknown }>;
  exports?: Array<{ path: string; export_name: string; line: number; col: number; [key: string]: unknown }>;
}

export interface RefactoringTarget {
  path: string;
  priority: number;
  efficiency: number;
  recommendation: string;
  category: string;
  effort: string;
  confidence: string;
  factors?: Array<Record<string, unknown>>;
  evidence?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface HealthReport {
  schema_version: number;
  version: string;
  elapsed_ms: number;
  findings: HealthFinding[];
  summary: Record<string, unknown>;
  vital_signs?: Record<string, unknown>;
  health_score?: Record<string, unknown>;
  file_scores?: FileHealthScore[];
  coverage_gaps?: CoverageGaps;
  hotspots?: Array<Record<string, unknown>>;
  hotspot_summary?: Record<string, unknown>;
  runtime_coverage?: Record<string, unknown>;
  large_functions?: Array<Record<string, unknown>>;
  targets?: RefactoringTarget[];
  target_thresholds?: Record<string, unknown>;
  health_trend?: Record<string, unknown>;
  _meta?: Record<string, unknown>;
}

export interface FallowNodeErrorShape {
  message: string;
  exitCode: number;
  code?: string;
  help?: string;
  context?: string;
}

export type FallowNodeError = Error & FallowNodeErrorShape;

export function detectDeadCode(options?: DeadCodeOptions): Promise<DeadCodeReport>;
export function detectCircularDependencies(options?: DeadCodeOptions): Promise<DeadCodeReport>;
export function detectBoundaryViolations(options?: DeadCodeOptions): Promise<DeadCodeReport>;
export function detectDuplication(options?: DuplicationOptions): Promise<DuplicationReport>;
export function detectFeatureFlags(options?: FeatureFlagsOptions): Promise<FeatureFlagsReport>;
export function computeComplexity(options?: ComplexityOptions): Promise<HealthReport>;
export function computeHealth(options?: ComplexityOptions): Promise<HealthReport>;
