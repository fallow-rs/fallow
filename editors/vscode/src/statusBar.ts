// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getChangedSince, getHealthStatusBar } from "./config.js";
import { formatHealthStatusPart } from "./health-utils.js";
import {
  buildParamsFromCli,
  buildStatusBarPartsFromLsp,
  buildStatusBarTooltipMarkdown,
  renderStatusBarText,
} from "./statusBar-utils.js";
import type {
  FallowAnalysisScope,
  FallowCheckResult,
  FallowDupesResult,
  HealthOutput,
} from "./types.js";
export type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type {
  AnalysisCompleteParams,
  ChangedSinceScopeStatus,
  PackageBaselineStatus,
} from "./statusBar-utils.js";

let statusBarItem: vscode.StatusBarItem | null = null;
let showingAnalysisResult = true;

/**
 * Last health score segment (e.g. `B (82)`), or null when health has not run or
 * is disabled. Stored so the segment survives the CLI/LSP status-bar updates
 * that fire independently of the lazy health spawn.
 */
let healthPart: string | null = null;

const liveChangedSince = (): string | null => getChangedSince() || null;

// Use `health:` (colon) so the score binds visually to its label and does not
// read as another issue/duplication count in the ` | `-joined segment list
// (e.g. `5 issues | 2.3% duplication | health: B (82)`) (#902).
const healthSuffix = (): string => (healthPart ? ` | health: ${healthPart}` : "");

export const createStatusBar = (): vscode.StatusBarItem => {
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  showingAnalysisResult = true;
  statusBarItem.command = "fallow.analyze";
  statusBarItem.text = renderStatusBarText("$(search) Fallow", liveChangedSince());
  statusBarItem.show();
  return statusBarItem;
};

/** Update the status bar from CLI-driven analysis results. */
export const updateStatusBar = (
  checkResult: FallowCheckResult | null,
  dupesResult: FallowDupesResult | null,
  scope: FallowAnalysisScope | null,
): void => {
  if (!statusBarItem) {
    return;
  }

  const params = buildParamsFromCli(checkResult, dupesResult, scope);
  applyTooltipAndSeverity(params);

  const parts: string[] = [];
  if (checkResult) {
    parts.push(`${params.totalIssues} issues`);
  }
  if (dupesResult) {
    parts.push(`${params.duplicationPercentage.toFixed(1)}% duplication`);
  }
  applyStatusBarText(parts, params.changedSinceScope, params.packageBaselines);
};

/** Update the status bar from LSP notification data. */
export const updateStatusBarFromLsp = (params: AnalysisCompleteParams): void => {
  if (!statusBarItem) {
    return;
  }

  applyTooltipAndSeverity(params);
  applyStatusBarText(
    buildStatusBarPartsFromLsp(params),
    params.changedSinceScope,
    params.packageBaselines,
  );
};

const applyTooltipAndSeverity = (params: AnalysisCompleteParams): void => {
  if (!statusBarItem) {
    return;
  }

  // The main status bar item is intentionally left uncolored: a full-width
  // red/yellow background for a poor health score is more distracting than
  // informative, and the grade + issue counts in the text already convey it.
  statusBarItem.backgroundColor = undefined;

  const tooltip = new vscode.MarkdownString(
    buildStatusBarTooltipMarkdown(params, getChangedSince() || null),
  );
  tooltip.isTrusted = true;
  // Required so `$(name)` codicons in the markdown render as icons rather
  // than literal text. Without this the popup shows raw `$(error)`,
  // `$(warning)`, etc. (issue #179).
  tooltip.supportThemeIcons = true;
  statusBarItem.tooltip = tooltip;
};

/**
 * Most recent non-health status parts, cached so an out-of-band health update
 * can re-render the full text (analysis parts plus the health segment) without
 * a fresh analysis run.
 */
let lastBaseParts: string[] = [];
let lastChangedSinceScope: ChangedSinceScopeStatus | undefined;
let lastPackageBaselines: ReadonlyArray<PackageBaselineStatus> = [];

const applyStatusBarText = (
  parts: string[],
  changedSinceScope?: ChangedSinceScopeStatus,
  packageBaselines: ReadonlyArray<PackageBaselineStatus> = [],
): void => {
  if (!statusBarItem) {
    return;
  }
  lastBaseParts = parts;
  lastChangedSinceScope = changedSinceScope;
  lastPackageBaselines = packageBaselines;
  showingAnalysisResult = true;
  const joined = parts.length > 0 ? `$(search) Fallow: ${parts.join(" | ")}` : "$(search) Fallow";
  const base = `${joined}${healthSuffix()}`;
  statusBarItem.text = renderStatusBarText(
    base,
    liveChangedSince(),
    changedSinceScope,
    packageBaselines,
  );
};

/**
 * Update the cached health score segment shown in the status bar and re-render.
 * Independent of the CLI/LSP analysis updates: the lazy health spawn calls this
 * when it completes. A null report (or the `health.statusBar` setting off)
 * clears the segment so the existing behavior is unchanged.
 */
export const updateStatusBarHealth = (report: HealthOutput | null): void => {
  healthPart = getHealthStatusBar() ? formatHealthStatusPart(report) : null;
  if (!statusBarItem) {
    return;
  }
  if (!showingAnalysisResult) {
    return;
  }
  // Re-render against the cached analysis parts so the health segment appends
  // without clobbering the issue/duplication counts.
  applyStatusBarText(lastBaseParts, lastChangedSinceScope, lastPackageBaselines);
};

export const setStatusBarAnalyzing = (): void => {
  if (statusBarItem) {
    showingAnalysisResult = false;
    statusBarItem.tooltip = undefined;
    statusBarItem.text = renderStatusBarText(
      "$(loading~spin) Fallow: Analyzing...",
      liveChangedSince(),
    );
  }
};

export const setStatusBarError = (): void => {
  if (statusBarItem) {
    showingAnalysisResult = false;
    statusBarItem.tooltip = undefined;
    statusBarItem.text = renderStatusBarText("$(error) Fallow: Error", liveChangedSince());
  }
};

export const disposeStatusBar = (): void => {
  if (statusBarItem) {
    statusBarItem.dispose();
    statusBarItem = null;
  }
  healthPart = null;
  lastBaseParts = [];
  lastChangedSinceScope = undefined;
  lastPackageBaselines = [];
  showingAnalysisResult = true;
};
