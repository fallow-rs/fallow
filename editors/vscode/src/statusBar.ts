// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getChangedSince } from "./config.js";
import {
  buildParamsFromCli,
  buildStatusBarPartsFromLsp,
  buildStatusBarTooltipMarkdown,
  getStatusBarSeverityKey,
  renderStatusBarText,
} from "./statusBar-utils.js";
import type { FallowCheckResult, FallowDupesResult } from "./types.js";
export type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";

let statusBarItem: vscode.StatusBarItem | null = null;

const liveChangedSince = (): string | null => getChangedSince() || null;

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

const applyStatusBarText = (parts: string[]): void => {
  if (!statusBarItem) {
    return;
  }
  const base = parts.length > 0 ? `$(search) Fallow: ${parts.join(" | ")}` : "$(search) Fallow";
  statusBarItem.text = renderStatusBarText(base, liveChangedSince());
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
};
