// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { auditGatingSuffix, auditVerdictPresentation, buildAuditTooltipMarkdown } from "./audit-utils.js";
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

/** Whether the audit status-bar item currently exists (for live create/dispose). */
export const hasAuditStatusBar = (): boolean => auditStatusBarItem !== null;

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

/**
 * Reset the item to its idle "click to audit" state, with no error tint. Used
 * when a run was skipped for a non-error reason (no workspace folder, run
 * already in flight) and there is no prior verdict to restore: flashing the
 * error state would mislabel a benign skip as a failure (#908 n4).
 */
export const setAuditIdle = (): void => {
  if (auditStatusBarItem) {
    auditStatusBarItem.text = "$(shield) Audit";
    auditStatusBarItem.backgroundColor = undefined;
    auditStatusBarItem.tooltip =
      "Fallow: audit the current change set for a pass/warn/fail verdict.";
  }
};

/** Render the verdict (and gating-candidate count) from a completed audit run. */
export const updateAuditStatusBar = (audit: AuditOutput): void => {
  if (!auditStatusBarItem) {
    return;
  }

  const presentation = auditVerdictPresentation(audit.verdict);
  // Show the gating count for any non-zero count (not just `fail`), so a `warn`
  // verdict's glance matches the tooltip's own `count > 0` branch.
  const suffix = auditGatingSuffix(audit);
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
