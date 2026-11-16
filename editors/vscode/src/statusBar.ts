// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  getAuditEnabled,
  getChangedSince,
  getHealthStatusBar,
  getLicenseShowStatusBar,
} from "./config.js";
import { formatHealthStatusPart } from "./health-utils.js";
import {
  HOVER_COMMANDS,
  STATUS_MENU_COMMAND,
  buildStatusMenuEntries,
  initialStatusModel,
  renderStatusPresentation,
} from "./statusBar-model.js";
import type {
  AuditStatus,
  FindingsVisibility,
  LicenseIndicator,
  StatusMenuEntry,
  StatusModel,
  WorkspaceSummary,
} from "./statusBar-model.js";
import { buildParamsFromCli } from "./statusBar-utils.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type {
  FallowAnalysisScope,
  FallowCheckResult,
  FallowDupesResult,
  HealthOutput,
} from "./types.js";
export type { AnalysisCompleteParams } from "./statusBar-utils.js";

/**
 * The single Fallow status bar item. It shows the finding count and the health
 * grade, and it carries the audit, license, workspace and findings-visibility
 * state in its hover, its icon and its background. A click opens a menu with
 * the existing Fallow commands.
 */
let item: vscode.StatusBarItem | null = null;
let model: StatusModel = initialStatusModel();

/** The name of the first folder, which the CLI and the LSP analyze, in a multi-root window. */
const multiRootAnalysisFolder = (): string | null => {
  const folders = vscode.workspace.workspaceFolders ?? [];
  return folders.length > 1 ? (folders[0]?.name ?? null) : null;
};

/** Settings are read on each render, so a settings change needs no reload. */
const withLiveSettings = (state: StatusModel): StatusModel => ({
  ...state,
  hasWorkspaceFolder: (vscode.workspace.workspaceFolders?.length ?? 0) > 0,
  analysisFolder: multiRootAnalysisFolder(),
  changedSince: getChangedSince() || null,
  showHealthGrade: getHealthStatusBar(),
  showAudit: getAuditEnabled(),
  license: getLicenseShowStatusBar() ? state.license : { kind: "hidden" },
});

const render = (): void => {
  if (!item) {
    return;
  }
  const presentation = renderStatusPresentation(withLiveSettings(model));
  item.text = presentation.text;
  const tooltip = new vscode.MarkdownString(presentation.tooltipMarkdown);
  tooltip.isTrusted = { enabledCommands: [...HOVER_COMMANDS] };
  // Without this flag the hover shows `$(error)` as literal text (issue #179).
  tooltip.supportThemeIcons = true;
  item.tooltip = tooltip;
  item.accessibilityInformation = { label: presentation.accessibilityLabel, role: "button" };
  item.backgroundColor = presentation.background
    ? new vscode.ThemeColor(presentation.background)
    : undefined;
};

const update = (patch: Partial<StatusModel>): void => {
  model = { ...model, ...patch };
  render();
};

export const createStatusBar = (): vscode.StatusBarItem => {
  item = vscode.window.createStatusBarItem("fallow.status", vscode.StatusBarAlignment.Left, 50);
  item.name = "Fallow";
  item.command = STATUS_MENU_COMMAND;
  render();
  item.show();
  return item;
};

/** Re-render after a change of a setting that the item reads. */
export const refreshStatusBar = (): void => {
  render();
};

/** Update the item from CLI analysis results. */
export const updateStatusBar = (
  checkResult: FallowCheckResult | null,
  dupesResult: FallowDupesResult | null,
  scope: FallowAnalysisScope | null,
): void => {
  update({
    analysis: { kind: "result", params: buildParamsFromCli(checkResult, dupesResult, scope) },
  });
};

/** Update the item from the `fallow/analysisComplete` LSP notification. */
export const updateStatusBarFromLsp = (params: AnalysisCompleteParams): void => {
  update({ analysis: { kind: "result", params } });
};

export const setStatusBarAnalyzing = (): void => {
  update({ analysis: { kind: "analyzing" } });
};

export const setStatusBarError = (): void => {
  update({ analysis: { kind: "error" } });
};

/**
 * Store the health score of the last health run. A null report clears it. The
 * `fallow.health.statusBar` setting controls only the grade in the item text.
 */
export const updateStatusBarHealth = (report: HealthOutput | null): void => {
  const label = formatHealthStatusPart(report);
  const grade = report?.health_score?.grade.trim() || "?";
  update({ health: label ? { grade, label } : null });
};

export const setStatusBarAudit = (audit: AuditStatus): void => {
  update({ audit });
};

export const setStatusBarLicense = (license: LicenseIndicator): void => {
  update({ license });
};

export const getStatusBarLicense = (): LicenseIndicator => model.license;

export const setStatusBarWorkspace = (workspace: WorkspaceSummary): void => {
  update({ workspace });
};

export const setStatusBarFindings = (findings: FindingsVisibility): void => {
  update({ findings });
};

interface StatusMenuPick extends vscode.QuickPickItem {
  readonly entry: StatusMenuEntry;
}

/** Show the click menu and run the command of the picked row. */
export const showStatusBarMenu = async (): Promise<void> => {
  const entries = buildStatusMenuEntries(withLiveSettings(model));
  const picked = await vscode.window.showQuickPick<StatusMenuPick>(
    entries.map((entry) => ({ label: entry.label, description: entry.description, entry })),
    { title: "Fallow", placeHolder: "Pick a Fallow action" },
  );
  if (picked) {
    await vscode.commands.executeCommand(picked.entry.command);
  }
};

export const disposeStatusBar = (): void => {
  if (item) {
    item.dispose();
    item = null;
  }
  model = initialStatusModel();
};
