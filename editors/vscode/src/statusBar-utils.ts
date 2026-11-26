import {
  COUNTED_FINDING_KEYS,
  countCheckIssues,
  countEnforcedFindings,
  countFindings,
  ENFORCED_ONLY_FINDING_KEYS,
  ISSUE_TYPE_FILTERS,
} from "./analysis-utils.js";
import type { PackageBaselineStatus } from "./generated/output-contract.js";
import { escapeMarkdownText, normalizeInlineText } from "./markdown-utils.js";
import type {
  FallowAnalysisScope,
  FallowCheckResult,
  FallowDupesResult,
  IssueTypeConfig,
} from "./types.js";

export type { PackageBaselineStatus } from "./generated/output-contract.js";

/** Whether the LSP server applied or dropped a requested changed-since scope. */
export type ChangedSinceScopeState = "applied" | "dropped";

/** Structured changed-since status reported by the LSP server. */
export interface ChangedSinceScopeStatus {
  readonly requestedRef: string;
  readonly state: ChangedSinceScopeState;
  readonly reason?: string;
}

export interface AnalysisCompleteParams {
  totalIssues: number;
  unusedFiles: number;
  unusedExports: number;
  unusedTypes: number;
  privateTypeLeaks: number;
  unusedDependencies: number;
  unusedDevDependencies: number;
  unusedOptionalDependencies: number;
  unusedEnumMembers: number;
  unusedClassMembers: number;
  unresolvedImports: number;
  unlistedDependencies: number;
  unlistedBinaries: number;
  duplicateExports: number;
  typeOnlyDependencies: number;
  testOnlyDependencies: number;
  devDependenciesInProduction: number;
  circularDependencies: number;
  reExportCycles: number;
  packageCycles: number;
  boundaryViolations: number;
  staleSuppressions: number;
  unusedCatalogEntries: number;
  unresolvedCatalogReferences: number;
  unusedDependencyOverrides: number;
  misconfiguredDependencyOverrides: number;
  // The fields below came after the first status bar release. An older LSP
  // does not send them.
  unusedNamespaceMembers?: number;
  unusedProtectedClassMembers?: number;
  namespaceEscapes?: number;
  testOnlyExports?: number;
  unusedStoreMembers?: number;
  unusedServerActions?: number;
  unusedLoadDataKeys?: number;
  unusedComponentProps?: number;
  absentComponentProps?: number;
  constantParameters?: number;
  unusedComponentEmits?: number;
  unusedComponentInputs?: number;
  unusedComponentOutputs?: number;
  unusedSvelteEvents?: number;
  unusedEffectLayers?: number;
  unusedTaggedErrors?: number;
  unproducedCatchTags?: number;
  unrenderedComponents?: number;
  unprovidedInjects?: number;
  invalidClientExports?: number;
  mixedClientServerBarrels?: number;
  misplacedDirectives?: number;
  routeCollisions?: number;
  dynamicSegmentNameConflicts?: number;
  duplicateExportAliases?: number;
  policyViolations?: number;
  emptyCatalogGroups?: number;
  enforcedPropDrillingChains?: number;
  enforcedThinWrappers?: number;
  enforcedDuplicatePropShapes?: number;
  duplicationPercentage: number;
  cloneGroups: number;
  changedSinceScope?: ChangedSinceScopeStatus;
  packageBaselines?: ReadonlyArray<PackageBaselineStatus>;
}

const changedSinceScopeFromCli = (
  scope: FallowAnalysisScope | null,
): ChangedSinceScopeStatus | undefined => {
  const outcome = scope?.request_outcomes?.["changed-since"];
  return outcome
    ? {
        requestedRef: outcome.requested,
        state: outcome.status === "applied" ? "applied" : "dropped",
        reason: outcome.message ?? undefined,
      }
    : undefined;
};

const toParamsField = (key: string): string =>
  key.replace(/_([a-z])/g, (_match: string, letter: string) => letter.toUpperCase());

/** The CLI arrays that the LSP counts in its one `boundaryViolations` field. */
const BOUNDARY_ARRAYS: ReadonlySet<string> = new Set([
  "boundary_violations",
  "boundary_coverage_violations",
  "boundary_call_violations",
]);

const ENFORCED_ONLY_ARRAYS: ReadonlySet<string> = new Set(ENFORCED_ONLY_FINDING_KEYS);

/**
 * The params field that counts the CLI array `key`, as the LSP names it. A
 * component health array has an `enforced` field: it counts only the
 * findings at `error` severity.
 */
const paramsFieldOf = (key: string): string => {
  if (BOUNDARY_ARRAYS.has(key)) {
    return "boundaryViolations";
  }
  const field = toParamsField(key);
  return ENFORCED_ONLY_ARRAYS.has(key)
    ? `enforced${field.charAt(0).toUpperCase()}${field.slice(1)}`
    : field;
};

/** The params fields whose sum is the total of the sidebar. */
export const COUNTED_PARAMS_FIELDS: ReadonlyArray<string> = [
  ...new Set([...COUNTED_FINDING_KEYS, ...ENFORCED_ONLY_FINDING_KEYS].map(paramsFieldOf)),
];

/**
 * Convert CLI analysis results into the same shape the LSP notification
 * delivers, so the status bar text and tooltip can be built from a single
 * source of truth regardless of whether LSP or CLI produced the data.
 */
export const buildParamsFromCli = (
  check: FallowCheckResult | null,
  dupes: FallowDupesResult | null,
  scope: FallowAnalysisScope | null,
): AnalysisCompleteParams => {
  // One field for each counted array, named as the LSP names it, so the hover
  // breakdown of a CLI run covers each category of its total.
  const counts: Record<string, number> = {};
  for (const key of COUNTED_FINDING_KEYS) {
    const field = paramsFieldOf(key);
    counts[field] = (counts[field] ?? 0) + (check ? countFindings(check, [key]) : 0);
  }
  for (const key of ENFORCED_ONLY_FINDING_KEYS) {
    counts[paramsFieldOf(key)] = check ? countEnforcedFindings(check, key) : 0;
  }
  return {
    ...(counts as unknown as AnalysisCompleteParams),
    totalIssues: countCheckIssues(check),
    duplicationPercentage: dupes?.stats.duplication_percentage ?? 0,
    cloneGroups: dupes?.stats.clone_groups ?? 0,
    changedSinceScope: changedSinceScopeFromCli(scope),
    packageBaselines: scope?.package_baselines,
  };
};

/**
 * Align LSP params with the count of the sidebar and of the "found N issues"
 * notification.
 *
 * The LSP total counts every category of the run, also the categories that
 * `fallow.issueTypes` turns off and findings that the sidebar does not count.
 * Without this, each LSP run set the status bar to a higher count than the
 * sidebar showed (#5278). The function sets each disabled category to zero and
 * sums the categories of {@link COUNTED_FINDING_KEYS}. The LSP sends one
 * `boundaryViolations` count for the three boundary arrays. An older LSP that
 * does not send each counted field keeps its total, less the disabled fields.
 */
export const alignWithSidebarCount = (
  params: AnalysisCompleteParams,
  issueTypes: IssueTypeConfig,
): AnalysisCompleteParams => {
  const counts: Record<string, unknown> = { ...params };
  const disabled = new Set(
    ISSUE_TYPE_FILTERS.filter(([, type]) => !issueTypes[type]).map(([key]) => paramsFieldOf(key)),
  );
  let removed = 0;
  for (const field of disabled) {
    const value = counts[field];
    if (typeof value === "number") {
      removed += value;
      counts[field] = 0;
    }
  }
  const values = COUNTED_PARAMS_FIELDS.map((field) => counts[field]);
  const totalIssues = values.every((value) => typeof value === "number")
    ? values.reduce<number>((sum, value) => sum + (value as number), 0)
    : Math.max(0, params.totalIssues - removed);
  return { ...(counts as unknown as AnalysisCompleteParams), totalIssues };
};

interface BreakdownLine {
  readonly count: keyof AnalysisCompleteParams;
  readonly icon: string;
  readonly label: string;
}

const BREAKDOWN_LINES: ReadonlyArray<BreakdownLine> = [
  {
    count: "unresolvedImports",
    icon: "$(error)",
    label: "unresolved imports",
  },
  { count: "unusedFiles", icon: "$(warning)", label: "unused files" },
  { count: "unusedExports", icon: "$(warning)", label: "unused exports" },
  { count: "unusedTypes", icon: "$(info)", label: "unused types" },
  {
    count: "privateTypeLeaks",
    icon: "$(warning)",
    label: "private type leaks",
  },
  {
    count: "unusedDependencies",
    icon: "$(warning)",
    label: "unused dependencies",
  },
  {
    count: "unusedDevDependencies",
    icon: "$(warning)",
    label: "unused dev dependencies",
  },
  {
    count: "unusedOptionalDependencies",
    icon: "$(warning)",
    label: "unused optional dependencies",
  },
  {
    count: "unusedEnumMembers",
    icon: "$(info)",
    label: "unused enum members",
  },
  {
    count: "unusedClassMembers",
    icon: "$(info)",
    label: "unused class members",
  },
  {
    count: "unlistedDependencies",
    icon: "$(warning)",
    label: "unlisted dependencies",
  },
  {
    count: "unlistedBinaries",
    icon: "$(warning)",
    label: "unlisted binaries",
  },
  {
    count: "duplicateExports",
    icon: "$(warning)",
    label: "duplicate exports",
  },
  {
    count: "typeOnlyDependencies",
    icon: "$(info)",
    label: "type-only dependencies",
  },
  {
    count: "testOnlyDependencies",
    icon: "$(info)",
    label: "test-only dependencies",
  },
  {
    count: "devDependenciesInProduction",
    icon: "$(warning)",
    label: "dev dependencies used in production",
  },
  {
    count: "circularDependencies",
    icon: "$(warning)",
    label: "circular dependencies",
  },
  {
    count: "reExportCycles",
    icon: "$(warning)",
    label: "re-export cycles",
  },
  {
    count: "packageCycles",
    icon: "$(warning)",
    label: "package cycles",
  },
  {
    count: "boundaryViolations",
    icon: "$(warning)",
    label: "boundary violations",
  },
  {
    count: "staleSuppressions",
    icon: "$(info)",
    label: "stale suppressions",
  },
  {
    count: "unusedCatalogEntries",
    icon: "$(warning)",
    label: "unused catalog entries",
  },
  {
    count: "unresolvedCatalogReferences",
    icon: "$(error)",
    label: "unresolved catalog references",
  },
  {
    count: "unusedDependencyOverrides",
    icon: "$(warning)",
    label: "unused dependency overrides",
  },
  {
    count: "misconfiguredDependencyOverrides",
    icon: "$(error)",
    label: "misconfigured dependency overrides",
  },
  { count: "unusedNamespaceMembers", icon: "$(info)", label: "unused namespace members" },
  {
    count: "unusedProtectedClassMembers",
    icon: "$(info)",
    label: "unused protected class members",
  },
  { count: "namespaceEscapes", icon: "$(warning)", label: "namespace escapes" },
  { count: "testOnlyExports", icon: "$(warning)", label: "test-only exports" },
  { count: "unusedStoreMembers", icon: "$(info)", label: "unused store members" },
  { count: "unusedServerActions", icon: "$(warning)", label: "unused server actions" },
  { count: "unusedLoadDataKeys", icon: "$(info)", label: "unused load data keys" },
  { count: "unusedComponentProps", icon: "$(info)", label: "unused component props" },
  { count: "absentComponentProps", icon: "$(info)", label: "absent component props" },
  { count: "constantParameters", icon: "$(info)", label: "constant parameters" },
  { count: "unusedComponentEmits", icon: "$(info)", label: "unused component emits" },
  { count: "unusedComponentInputs", icon: "$(info)", label: "unused component inputs" },
  { count: "unusedComponentOutputs", icon: "$(info)", label: "unused component outputs" },
  { count: "unusedSvelteEvents", icon: "$(info)", label: "unused Svelte events" },
  { count: "unusedEffectLayers", icon: "$(info)", label: "unused Effect layers" },
  { count: "unusedTaggedErrors", icon: "$(info)", label: "unused tagged errors" },
  { count: "unproducedCatchTags", icon: "$(info)", label: "unproduced catch tags" },
  { count: "unrenderedComponents", icon: "$(warning)", label: "unrendered components" },
  { count: "unprovidedInjects", icon: "$(warning)", label: "unprovided injects" },
  { count: "invalidClientExports", icon: "$(warning)", label: "invalid client exports" },
  { count: "mixedClientServerBarrels", icon: "$(warning)", label: "mixed client/server barrels" },
  { count: "misplacedDirectives", icon: "$(warning)", label: "misplaced directives" },
  { count: "routeCollisions", icon: "$(error)", label: "route collisions" },
  {
    count: "dynamicSegmentNameConflicts",
    icon: "$(error)",
    label: "dynamic segment name conflicts",
  },
  { count: "duplicateExportAliases", icon: "$(warning)", label: "duplicate export aliases" },
  { count: "policyViolations", icon: "$(warning)", label: "policy violations" },
  { count: "emptyCatalogGroups", icon: "$(warning)", label: "empty catalog groups" },
  {
    count: "enforcedPropDrillingChains",
    icon: "$(error)",
    label: "prop drilling chains at error severity",
  },
  {
    count: "enforcedThinWrappers",
    icon: "$(error)",
    label: "thin wrappers at error severity",
  },
  {
    count: "enforcedDuplicatePropShapes",
    icon: "$(error)",
    label: "duplicate prop shapes at error severity",
  },
];

export const getDuplicationPercentage = (duplicationPercentage: number): number =>
  Number.isFinite(duplicationPercentage) ? duplicationPercentage : 0;

export const formatChangedSinceRefForStatusBar = (ref: string): string => {
  const normalized = normalizeInlineText(ref);
  return normalized.length > 48 ? `${normalized.slice(0, 45).trimEnd()}...` : normalized;
};

/**
 * Resolve the visible status bar text for a given base label, appending
 * the persistent `changedSince` suffix when that filter is active.
 *
 * Single source of truth across the four status bar states (idle,
 * analyzing, error, post-analysis). Earlier the post-analysis path was
 * the only state that showed `(since <ref>)`, which made the filter feel
 * intermittent and forced users to hover the tooltip to verify it was
 * still active. The panel review for issue #190 flagged this as the
 * visible signal that should match the `changedSince` filter applied to
 * LSP diagnostics.
 *
 * Pure: a reported scope wins over the live editor setting, which may have
 * changed since the run. Callers pass `getChangedSince()` as a fallback.
 */
export const renderStatusBarText = (
  base: string,
  changedSince: string | null,
  scope?: ChangedSinceScopeStatus,
  packageBaselines: ReadonlyArray<PackageBaselineStatus> = [],
): string => {
  if (!scope && packageBaselines.length > 0) {
    return `${base} (package baselines)`;
  }
  const requestedRef = scope?.requestedRef || changedSince;
  if (!requestedRef) {
    return base;
  }
  const formattedRef = formatChangedSinceRefForStatusBar(requestedRef);
  if (scope?.state === "dropped") {
    return `${base} (since ${formattedRef}: scope dropped)`;
  }
  return `${base} (since ${formattedRef})`;
};

const scopeLines = (params: AnalysisCompleteParams, changedSinceRef: string | null): string[] => {
  const scope = params.changedSinceScope;
  if (scope?.state === "dropped") {
    const dropped = [
      `$(warning) Scope since ${escapeMarkdownText(scope.requestedRef)} was dropped; results are full-scope.`,
    ];
    return scope.reason ? [...dropped, `Reason: ${escapeMarkdownText(scope.reason)}`] : dropped;
  }
  const baselines = params.packageBaselines ?? [];
  const appliedRef = scope?.requestedRef || (baselines.length > 0 ? null : changedSinceRef);
  if (appliedRef) {
    return [`$(git-branch) Scoped to changes since ${escapeMarkdownText(appliedRef)}`];
  }
  if (baselines.length === 0) {
    return [];
  }
  return [
    "$(git-branch) Code diagnostics and clone groups use package baselines:",
    ...baselines.map(
      (baseline) =>
        `- ${escapeMarkdownText(baseline.workspace_root)}: ${escapeMarkdownText(baseline.reference)}`,
    ),
    "Unlisted packages and root files remain in full scope.",
    "Health and security reports use their own scopes.",
  ];
};

/**
 * The Problems panel has a diagnostic for each location: each clone instance,
 * each edge of a cycle and each import site. Its count can then differ from
 * the finding total, and that looks like a second, wrong total.
 */
const PROBLEMS_PANEL_NOTE =
  "$(info) The Problems panel lists each location of a finding, so its count can differ from this total.";

/**
 * Hover lines for a completed analysis: the scope, then one line per issue
 * category with a non-zero count. The status bar hover adds the duplication
 * line and the other sections.
 */
export const buildAnalysisSummaryLines = (
  params: AnalysisCompleteParams,
  changedSinceRef: string | null = null,
): string[] => {
  const lines = scopeLines(params, changedSinceRef);
  for (const line of BREAKDOWN_LINES) {
    const count = params[line.count];
    if (typeof count === "number" && count > 0) {
      lines.push(`${line.icon} ${count} ${line.label}`);
    }
  }
  if (params.totalIssues === 0) {
    lines.push("$(check) No issues found");
  } else {
    lines.push(PROBLEMS_PANEL_NOTE);
  }
  return lines;
};
