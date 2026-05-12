import { countCheckIssues } from "./analysis-utils.js";
import type { FallowCheckResult, FallowDupesResult } from "./types.js";

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
  duplicateExports: number;
  typeOnlyDependencies: number;
  testOnlyDependencies: number;
  circularDependencies: number;
  boundaryViolations: number;
  staleSuppressions: number;
  unusedCatalogEntries: number;
  unresolvedCatalogReferences: number;
  unusedDependencyOverrides: number;
  misconfiguredDependencyOverrides: number;
  duplicationPercentage: number;
  cloneGroups: number;
}

/**
 * Convert CLI analysis results into the same shape the LSP notification
 * delivers, so the status bar text and tooltip can be built from a single
 * source of truth regardless of whether LSP or CLI produced the data.
 */
export const buildParamsFromCli = (
  check: FallowCheckResult | null,
  dupes: FallowDupesResult | null
): AnalysisCompleteParams => ({
  totalIssues: countCheckIssues(check),
  unusedFiles: check?.unused_files.length ?? 0,
  unusedExports: check?.unused_exports.length ?? 0,
  unusedTypes: check?.unused_types.length ?? 0,
  privateTypeLeaks: check?.private_type_leaks?.length ?? 0,
  unusedDependencies: check?.unused_dependencies.length ?? 0,
  unusedDevDependencies: check?.unused_dev_dependencies.length ?? 0,
  unusedOptionalDependencies: check?.unused_optional_dependencies?.length ?? 0,
  unusedEnumMembers: check?.unused_enum_members.length ?? 0,
  unusedClassMembers: check?.unused_class_members.length ?? 0,
  unresolvedImports: check?.unresolved_imports.length ?? 0,
  unlistedDependencies: check?.unlisted_dependencies.length ?? 0,
  duplicateExports: check?.duplicate_exports.length ?? 0,
  typeOnlyDependencies: check?.type_only_dependencies?.length ?? 0,
  testOnlyDependencies: check?.test_only_dependencies?.length ?? 0,
  circularDependencies: check?.circular_dependencies?.length ?? 0,
  boundaryViolations: check?.boundary_violations?.length ?? 0,
  staleSuppressions: check?.stale_suppressions?.length ?? 0,
  unusedCatalogEntries: check?.unused_catalog_entries?.length ?? 0,
  unresolvedCatalogReferences:
    check?.unresolved_catalog_references?.length ?? 0,
  unusedDependencyOverrides:
    check?.unused_dependency_overrides?.length ?? 0,
  misconfiguredDependencyOverrides:
    check?.misconfigured_dependency_overrides?.length ?? 0,
  duplicationPercentage: dupes?.stats.duplication_percentage ?? 0,
  cloneGroups: dupes?.stats.clone_groups ?? 0,
});

type SeverityKey =
  | "statusBarItem.errorBackground"
  | "statusBarItem.warningBackground";

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
    count: "circularDependencies",
    icon: "$(warning)",
    label: "circular dependencies",
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
];

export const getDuplicationPercentage = (
  duplicationPercentage: number
): number => (Number.isFinite(duplicationPercentage) ? duplicationPercentage : 0);

export const buildStatusBarPartsFromLsp = (
  params: AnalysisCompleteParams
): string[] => [
  `${params.totalIssues} issues`,
  `${getDuplicationPercentage(params.duplicationPercentage).toFixed(1)}% duplication`,
];

export const getStatusBarSeverityKey = (
  params: AnalysisCompleteParams
): SeverityKey | null => {
  if (params.unresolvedImports > 0) {
    return "statusBarItem.errorBackground";
  }

  if (params.totalIssues > 0) {
    return "statusBarItem.warningBackground";
  }

  return null;
};

const normalizeInlineText = (value: string): string =>
  value.replace(/\s+/g, " ").trim();

export const formatChangedSinceRefForStatusBar = (ref: string): string => {
  const normalized = normalizeInlineText(ref);
  return normalized.length > 48
    ? `${normalized.slice(0, 45).trimEnd()}...`
    : normalized;
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
 * Pure: takes the resolved ref so it can be unit-tested without a vscode
 * mock. Callers in `statusBar.ts` pass `getChangedSince()` or `null`.
 */
export const renderStatusBarText = (
  base: string,
  changedSince: string | null
): string => {
  if (!changedSince) {
    return base;
  }
  return `${base} (since ${formatChangedSinceRefForStatusBar(changedSince)})`;
};

const escapeMarkdownText = (value: string): string =>
  normalizeInlineText(value).replace(/([\\`*_{}[\]()#+.!|>-])/g, "\\$1");

export const buildStatusBarTooltipMarkdown = (
  params: AnalysisCompleteParams,
  changedSinceRef: string | null = null
): string => {
  const lines: string[] = ["**Fallow** - Analysis Results\n"];
  const duplicationPercentage = getDuplicationPercentage(
    params.duplicationPercentage
  );

  if (changedSinceRef) {
    lines.push(
      `$(git-branch) Scoped to changes since ${escapeMarkdownText(changedSinceRef)}`
    );
  }

  for (const line of BREAKDOWN_LINES) {
    const count = params[line.count];
    if (typeof count === "number" && count > 0) {
      lines.push(`${line.icon} ${count} ${line.label}`);
    }
  }

  if (params.cloneGroups > 0) {
    lines.push(
      `$(copy) ${params.cloneGroups} clone groups (${duplicationPercentage.toFixed(1)}% duplication)`
    );
  }

  if (params.totalIssues === 0 && params.cloneGroups === 0) {
    lines.push("$(check) No issues found");
  }

  lines.push("\n---\n");
  lines.push(
    "[$(play) Run Analysis](command:fallow.analyze) · [$(wrench) Auto-Fix](command:fallow.fix) · [$(output) Output](command:fallow.showOutput)"
  );

  return lines.join("\n\n");
};
