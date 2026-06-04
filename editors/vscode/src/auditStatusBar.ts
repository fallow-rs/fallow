// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  auditVerdictPresentation,
  buildAuditTooltipMarkdown,
  gatingCount,
} from "./audit-utils.js";
import { getChangedSince } from "./config.js";
import type { AuditOutput } from "./types.js";

let auditStatusBarItem: vscode.StatusBarItem | null = null;

/**
 * Create the dedicated audit verdict status-bar item, just right of the main
 * analysis item (priority 49 vs 50). Idle state advertises the on-demand
 * command; no analysis runs until the user clicks it or invokes the command,
 * so creating the item is free (#902 latency: nothing on the hot path).
 */
export const createAuditStatusBar = (): vscode.StatusBarItem => {
  auditStatusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 49);
  auditStatusBarItem.command = "fallow.audit";
  auditStatusBarItem.text = "$(shield) Audit";
  auditStatusBarItem.tooltip = "Fallow: audit the current change set for a pass/warn/fail verdict.";
  auditStatusBarItem.show();
  return auditStatusBarItem;
};

export const setAuditAnalyzing = (): void => {
  if (auditStatusBarItem) {
    auditStatusBarItem.text = "$(loading~spin) Audit: running...";
    auditStatusBarItem.backgroundColor = undefined;
  }
};

export const setAuditError = (): void => {
  if (auditStatusBarItem) {
    auditStatusBarItem.text = "$(error) Audit: error";
    auditStatusBarItem.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
    auditStatusBarItem.tooltip =
      "Fallow: the audit run failed. See the Fallow output channel for details.";
  }
};

/** Render the verdict (and gating-candidate count) from a completed audit run. */
export const updateAuditStatusBar = (audit: AuditOutput): void => {
  if (!auditStatusBarItem) {
    return;
  }

  const presentation = auditVerdictPresentation(audit.verdict);
  const count = gatingCount(audit);
  const suffix = audit.verdict === "fail" && count > 0 ? ` (${count})` : "";
  auditStatusBarItem.text = `${presentation.icon} Audit: ${presentation.label}${suffix}`;
  auditStatusBarItem.backgroundColor = presentation.background
    ? new vscode.ThemeColor(presentation.background)
    : undefined;

  const tooltip = new vscode.MarkdownString(
    buildAuditTooltipMarkdown(audit, getChangedSince() || null),
  );
  tooltip.isTrusted = true;
  // Required so `$(name)` codicons render as icons rather than literal text.
  tooltip.supportThemeIcons = true;
  auditStatusBarItem.tooltip = tooltip;
};

export const disposeAuditStatusBar = (): void => {
  if (auditStatusBarItem) {
    auditStatusBarItem.dispose();
    auditStatusBarItem = null;
  }
};
