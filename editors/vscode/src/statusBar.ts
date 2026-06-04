// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getChangedSince, getHealthStatusBar } from "./config.js";
import { formatHealthStatusPart } from "./health-utils.js";
import {
  buildParamsFromCli,
  buildStatusBarPartsFromLsp,
  buildStatusBarTooltipMarkdown,
  getStatusBarSeverityKey,
  renderStatusBarText,
} from "./statusBar-utils.js";
import type { FallowCheckResult, FallowDupesResult, HealthReport } from "./types.js";
export type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";

let statusBarItem: vscode.StatusBarItem | null = null;

/**
 * Last health score segment (e.g. `B (82)`), or null when health has not run or
 * is disabled. Stored so the segment survives the CLI/LSP status-bar updates
 * that fire independently of the lazy health spawn.
 */
let healthPart: string | null = null;

const liveChangedSince = (): string | null => getChangedSince() || null;

const healthSuffix = (): string => (healthPart ? ` | health ${healthPart}` : "");

export const createStatusBar = (): vscode.StatusBarItem => {
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  statusBarItem.command = "fallow.analyze";
  statusBarItem.text = renderStatusBarText("$(search) Fallow", liveChangedSince());
  statusBarItem.show();
  return statusBarItem;
};

/** Update the status bar from CLI-driven analysis results. */
export const updateStatusBar = (
  checkResult: FallowCheckResult | null,
  dupesResult: FallowDupesResult | null,
): void => {
  if (!statusBarItem) {
    return;
  }

  const params = buildParamsFromCli(checkResult, dupesResult);
  applyTooltipAndSeverity(params);

  const parts: string[] = [];
  if (checkResult) {
    parts.push(`${params.totalIssues} issues`);
  }
  if (dupesResult) {
    parts.push(`${params.duplicationPercentage.toFixed(1)}% duplication`);
  }
  applyStatusBarText(parts);
};

/** Update the status bar from LSP notification data. */
export const updateStatusBarFromLsp = (params: AnalysisCompleteParams): void => {
  if (!statusBarItem) {
    return;
  }

  applyTooltipAndSeverity(params);
  applyStatusBarText(buildStatusBarPartsFromLsp(params));
};

const applyTooltipAndSeverity = (params: AnalysisCompleteParams): void => {
  if (!statusBarItem) {
    return;
  }

  const severity = getStatusBarSeverityKey(params);
  statusBarItem.backgroundColor = severity ? new vscode.ThemeColor(severity) : undefined;

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

const applyStatusBarText = (parts: string[]): void => {
  if (!statusBarItem) {
    return;
  }
  lastBaseParts = parts;
  const joined = parts.length > 0 ? `$(search) Fallow: ${parts.join(" | ")}` : "$(search) Fallow";
  const base = `${joined}${healthSuffix()}`;
  statusBarItem.text = renderStatusBarText(base, liveChangedSince());
};

/**
 * Update the cached health score segment shown in the status bar and re-render.
 * Independent of the CLI/LSP analysis updates: the lazy health spawn calls this
 * when it completes. A null report (or the `health.statusBar` setting off)
 * clears the segment so the existing behavior is unchanged.
 */
export const updateStatusBarHealth = (report: HealthReport | null): void => {
  healthPart = getHealthStatusBar() ? formatHealthStatusPart(report) : null;
  if (!statusBarItem) {
    return;
  }
  // Re-render against the cached analysis parts so the health segment appends
  // without clobbering the issue/duplication counts.
  applyStatusBarText(lastBaseParts);
};

export const setStatusBarAnalyzing = (): void => {
  if (statusBarItem) {
    statusBarItem.text = renderStatusBarText(
      "$(loading~spin) Fallow: Analyzing...",
      liveChangedSince(),
    );
  }
};

export const setStatusBarError = (): void => {
  if (statusBarItem) {
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
};
