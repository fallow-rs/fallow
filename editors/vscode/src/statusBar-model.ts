/**
 * Pure presentation of the single Fallow status bar item: the text, the hover,
 * the accessibility label, the background and the click menu. No `vscode`
 * import, so vitest runs these headless. `statusBar.ts` owns the item and
 * applies the result.
 */

import { buildAuditSummaryLines } from "./audit-utils.js";
import { escapeMarkdownText } from "./markdown-utils.js";
import {
  buildAnalysisSummaryLines,
  getDuplicationPercentage,
  renderStatusBarText,
} from "./statusBar-utils.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type { LicenseStatusJson } from "./license-types.js";
import type { AuditOutput } from "./types.js";
import { WORKSPACE_SCOPE_DISCLOSURE } from "./workspacePicker-utils.js";

/** State of the dead-code and duplication analysis. */
export type AnalysisStatus =
  | { readonly kind: "idle" }
  | { readonly kind: "analyzing" }
  | { readonly kind: "error" }
  | { readonly kind: "result"; readonly params: AnalysisCompleteParams };

/** State of the on-demand audit of the current change set. */
export type AuditStatus =
  | { readonly kind: "idle" }
  | { readonly kind: "running" }
  | { readonly kind: "error" }
  | { readonly kind: "result"; readonly audit: AuditOutput };

/**
 * State of the license indicator. `hidden` is for a machine with no license
 * material: such a machine never sees license text.
 */
export type LicenseIndicator =
  | { readonly kind: "hidden" }
  | { readonly kind: "unchecked" }
  | { readonly kind: "status"; readonly status: LicenseStatusJson };

/** Health score of the last health run, for example grade `C` and label `C (72)`. */
export interface HealthSummary {
  readonly grade: string;
  readonly label: string;
}

/** Workspace scope state. `pickerAvailable` is false on a single-package repo. */
export interface WorkspaceSummary {
  readonly pickerAvailable: boolean;
  /** Active package name, or an empty string for the whole project. */
  readonly active: string;
}

/** Visibility of Fallow findings in the editor (the diagnostic filter state). */
export interface FindingsVisibility {
  readonly mutedAll: boolean;
  readonly mutedCategoryCount: number;
}

/** Everything that the single status bar item shows. */
export interface StatusModel {
  readonly hasWorkspaceFolder: boolean;
  readonly analysis: AnalysisStatus;
  /** The live `fallow.changedSince` setting, or null when it is not set. */
  readonly changedSince: string | null;
  readonly health: HealthSummary | null;
  /** `fallow.health.statusBar`: show the grade in the item text. */
  readonly showHealthGrade: boolean;
  readonly audit: AuditStatus;
  /** `fallow.audit.statusBar.enabled`: show the audit verdict in the item. */
  readonly showAudit: boolean;
  readonly license: LicenseIndicator;
  readonly workspace: WorkspaceSummary;
  readonly findings: FindingsVisibility;
}

/** Theme color key for the item background, or null for no background. */
export type StatusBackground =
  | "statusBarItem.errorBackground"
  | "statusBarItem.warningBackground"
  | null;

/** The rendered item. */
export interface StatusPresentation {
  readonly text: string;
  /** Trusted markdown with codicons. */
  readonly tooltipMarkdown: string;
  /** Plain text for screen readers. */
  readonly accessibilityLabel: string;
  readonly background: StatusBackground;
}

/** One row of the click menu. `command` is an existing Fallow command id. */
export interface StatusMenuEntry {
  readonly label: string;
  readonly description?: string;
  readonly command: string;
}

/**
 * The only commands that the trusted hover may run. The hover sets
 * `isTrusted` to this list, so a link to any other command does nothing.
 */
export const HOVER_COMMANDS: readonly string[] = [
  "fallow.analyze",
  "fallow.fix",
  "fallow.showOutput",
  "fallow.audit",
  "fallow.selectWorkspace",
  "fallow.license.status",
  "fallow.license.refresh",
  "fallow.license.activate",
];

/** The command that the item runs on click. */
export const STATUS_MENU_COMMAND = "fallow.showStatusMenu";

/**
 * A valid license shows in the item text when it expires in this number of
 * days or fewer. Before that, the license shows only in the hover.
 */
export const LICENSE_EXPIRY_NOTICE_DAYS = 14;

const BRAND = "Fallow";
const SEPARATOR = " · ";

export const initialStatusModel = (): StatusModel => ({
  hasWorkspaceFolder: true,
  analysis: { kind: "idle" },
  changedSince: null,
  health: null,
  showHealthGrade: true,
  audit: { kind: "idle" },
  showAudit: true,
  license: { kind: "hidden" },
  workspace: { pickerAvailable: false, active: "" },
  findings: { mutedAll: false, mutedCategoryCount: 0 },
});

const plural = (count: number, singular: string, pluralForm = `${singular}s`): string =>
  `${count} ${count === 1 ? singular : pluralForm}`;

/** Short license notice for the item text, or null when the license needs no action. */
export const licenseBarNotice = (license: LicenseIndicator): string | null => {
  if (license.kind !== "status") {
    return null;
  }
  const { status } = license;
  switch (status.state) {
    case "valid": {
      const days = status.days_until_expiry;
      if (days === null || days > LICENSE_EXPIRY_NOTICE_DAYS) {
        return null;
      }
      return `$(key) license: ${plural(days, "day")} left`;
    }
    case "expired_warning":
    case "expired_watermark":
      return "$(warning) license expired";
    case "hard_fail":
      return "$(error) license expired";
    case "missing":
      return "$(warning) license invalid";
    default: {
      const never: never = status.state;
      return never;
    }
  }
};

const REFRESH_LICENSE_LINK = "[Refresh](command:fallow.license.refresh)";

const validLicenseHoverLine = (days: number | null, tier: string): string => {
  if (days === null) {
    return `$(verified) License: active${tier}.`;
  }
  const refresh = days <= LICENSE_EXPIRY_NOTICE_DAYS ? ` ${REFRESH_LICENSE_LINK}` : "";
  return `$(verified) License: active${tier}, expires in ${plural(days, "day")}.${refresh}`;
};

const licenseHoverLine = (license: LicenseIndicator): string | null => {
  if (license.kind === "hidden") {
    return null;
  }
  if (license.kind === "unchecked") {
    return "$(key) License: not checked yet. [Show status](command:fallow.license.status)";
  }
  const { status } = license;
  const tier = status.tier !== null ? ` (${escapeMarkdownText(status.tier)})` : "";
  switch (status.state) {
    case "valid":
      return validLicenseHoverLine(status.days_until_expiry, tier);
    case "expired_warning":
    case "expired_watermark":
      return `$(warning) License: expired${tier}. ${REFRESH_LICENSE_LINK}`;
    case "hard_fail":
      return `$(error) License: expired${tier}, paid features are blocked. ${REFRESH_LICENSE_LINK}`;
    case "missing":
      return "$(warning) License: the license file is not valid. [Activate](command:fallow.license.activate)";
    default: {
      const never: never = status.state;
      return never;
    }
  }
};

const isAuditFailure = (model: StatusModel): boolean =>
  model.showAudit &&
  (model.audit.kind === "error" ||
    (model.audit.kind === "result" && model.audit.audit.verdict === "fail"));

const isAuditWarning = (model: StatusModel): boolean =>
  model.showAudit && model.audit.kind === "result" && model.audit.audit.verdict === "warn";

const leadingIcon = (model: StatusModel): string => {
  if (model.analysis.kind === "analyzing" || (model.showAudit && model.audit.kind === "running")) {
    return "$(loading~spin)";
  }
  if (model.analysis.kind === "error" || isAuditFailure(model)) {
    return "$(error)";
  }
  if (isAuditWarning(model)) {
    return "$(warning)";
  }
  if (model.findings.mutedAll) {
    return "$(eye-closed)";
  }
  return "$(search)";
};

const analysisText = (model: StatusModel): string => {
  switch (model.analysis.kind) {
    case "idle":
      return BRAND;
    case "analyzing":
      return `${BRAND}: analyzing`;
    case "error":
      return `${BRAND}: error`;
    case "result": {
      const grade =
        model.showHealthGrade && model.health ? `${SEPARATOR}${model.health.grade}` : "";
      return `${BRAND} ${model.analysis.params.totalIssues}${grade}`;
    }
    default: {
      const never: never = model.analysis;
      return never;
    }
  }
};

/**
 * Item text, for example `$(search) Fallow 9 · C`. The changed-since suffix
 * stays visible in each state, so the scope of the count is always clear.
 */
export const renderStatusText = (model: StatusModel): string => {
  if (!model.hasWorkspaceFolder) {
    return `$(search) ${BRAND}: no folder`;
  }
  const params = model.analysis.kind === "result" ? model.analysis.params : undefined;
  const base = renderStatusBarText(
    `${leadingIcon(model)} ${analysisText(model)}`,
    model.changedSince,
    params?.changedSinceScope,
    params?.packageBaselines ?? [],
  );
  const notice = licenseBarNotice(model.license);
  return notice ? `${base}${SEPARATOR}${notice}` : base;
};

/** Background tint: only the audit verdict colors the item. */
export const statusBackground = (model: StatusModel): StatusBackground => {
  if (!model.hasWorkspaceFolder) {
    return null;
  }
  if (isAuditFailure(model)) {
    return "statusBarItem.errorBackground";
  }
  if (isAuditWarning(model)) {
    return "statusBarItem.warningBackground";
  }
  return null;
};

const analysisHoverLines = (model: StatusModel): string[] => {
  switch (model.analysis.kind) {
    case "idle":
      return ["$(search) Analysis: not run yet."];
    case "analyzing":
      return ["$(loading~spin) Analysis: running."];
    case "error":
      return ["$(error) Analysis: the run failed. See the Fallow output channel."];
    case "result": {
      const { params } = model.analysis;
      const duplication = getDuplicationPercentage(params.duplicationPercentage).toFixed(1);
      return [
        ...buildAnalysisSummaryLines(params, model.changedSince),
        `$(copy) Duplication: ${duplication}% (${plural(params.cloneGroups, "clone group")})`,
      ];
    }
    default: {
      const never: never = model.analysis;
      return never;
    }
  }
};

const healthHoverLine = (model: StatusModel): string =>
  model.health
    ? `$(pulse) Health: ${escapeMarkdownText(model.health.label)}`
    : "$(pulse) Health: not computed yet. Open the Health view to compute it.";

const auditHoverLines = (model: StatusModel): string[] => {
  if (!model.showAudit) {
    return [];
  }
  switch (model.audit.kind) {
    case "idle":
      return ["$(shield) Audit: not run yet. [Run audit](command:fallow.audit)"];
    case "running":
      return ["$(loading~spin) Audit: running."];
    case "error":
      return ["$(error) Audit: the run failed. See the Fallow output channel."];
    case "result":
      return buildAuditSummaryLines(model.audit.audit);
    default: {
      const never: never = model.audit;
      return never;
    }
  }
};

const workspaceHoverLine = (workspace: WorkspaceSummary): string | null => {
  if (!workspace.pickerAvailable) {
    return null;
  }
  if (!workspace.active) {
    return "$(layers) Workspace: whole project. [Switch](command:fallow.selectWorkspace)";
  }
  // Editor diagnostics are not workspace-scoped, so a scoped hover says so (#906 C3).
  return `$(layers) Workspace: ${escapeMarkdownText(workspace.active)}. [Switch](command:fallow.selectWorkspace) ${WORKSPACE_SCOPE_DISCLOSURE}`;
};

const findingsHoverLine = (findings: FindingsVisibility): string => {
  if (findings.mutedAll) {
    return "$(eye-closed) Findings in the editor: hidden. CI and `fallow check` still report every finding.";
  }
  if (findings.mutedCategoryCount > 0) {
    return `$(eye) Findings in the editor: visible (${plural(findings.mutedCategoryCount, "category", "categories")} hidden).`;
  }
  return "$(eye) Findings in the editor: visible.";
};

const hoverTitle = (workspace: WorkspaceSummary): string => {
  if (!workspace.pickerAvailable || !workspace.active) {
    return `**${BRAND}**`;
  }
  return `**${BRAND}** (workspace: ${escapeMarkdownText(workspace.active)})`;
};

const HOVER_ACTIONS =
  "[$(play) Run analysis](command:fallow.analyze) · [$(wrench) Auto-fix](command:fallow.fix) · [$(output) Output](command:fallow.showOutput)";

/** Trusted hover markdown. Every line uses codicons, so the caller sets `supportThemeIcons`. */
export const renderStatusTooltip = (model: StatusModel): string => {
  if (!model.hasWorkspaceFolder) {
    return [`**${BRAND}**`, "Open a folder to analyze it with Fallow."].join("\n\n");
  }
  const lines: string[] = [
    hoverTitle(model.workspace),
    ...analysisHoverLines(model),
    healthHoverLine(model),
    ...auditHoverLines(model),
  ];
  for (const line of [
    licenseHoverLine(model.license),
    workspaceHoverLine(model.workspace),
    findingsHoverLine(model.findings),
  ]) {
    if (line !== null) {
      lines.push(line);
    }
  }
  lines.push("---", HOVER_ACTIONS);
  return lines.join("\n\n");
};

const analysisA11y = (model: StatusModel): string => {
  switch (model.analysis.kind) {
    case "idle":
      return "analysis not run yet";
    case "analyzing":
      return "analysis running";
    case "error":
      return "analysis failed";
    case "result": {
      const findings = plural(model.analysis.params.totalIssues, "finding");
      return model.health ? `${findings}, health grade ${model.health.grade}` : findings;
    }
    default: {
      const never: never = model.analysis;
      return never;
    }
  }
};

const auditA11y = (model: StatusModel): string | null => {
  if (!model.showAudit) {
    return null;
  }
  switch (model.audit.kind) {
    case "idle":
      return null;
    case "running":
      return "audit running";
    case "error":
      return "audit failed to run";
    case "result":
      return `audit ${model.audit.audit.verdict}`;
    default: {
      const never: never = model.audit;
      return never;
    }
  }
};

const licenseA11y = (license: LicenseIndicator): string | null => {
  const notice = licenseBarNotice(license);
  return notice ? notice.replace(/^\$\([^)]+\)\s*/, "") : null;
};

/** Plain-text label for screen readers. Codicon names never reach this label. */
export const renderStatusAccessibilityLabel = (model: StatusModel): string => {
  if (!model.hasWorkspaceFolder) {
    return `${BRAND}: no folder is open.`;
  }
  const parts = [analysisA11y(model), auditA11y(model), licenseA11y(model.license)];
  if (model.findings.mutedAll) {
    parts.push("findings hidden in the editor");
  }
  const summary = parts.filter((part): part is string => part !== null).join(", ");
  return `${BRAND}: ${summary}. Activate to open the Fallow menu.`;
};

export const renderStatusPresentation = (model: StatusModel): StatusPresentation => ({
  text: renderStatusText(model),
  tooltipMarkdown: renderStatusTooltip(model),
  accessibilityLabel: renderStatusAccessibilityLabel(model),
  background: statusBackground(model),
});

const auditMenuDescription = (model: StatusModel): string | undefined => {
  if (!model.showAudit) {
    return undefined;
  }
  if (model.audit.kind === "result") {
    return `last verdict: ${model.audit.audit.verdict}`;
  }
  if (model.audit.kind === "error") {
    return "last run failed";
  }
  return undefined;
};

const licenseMenuEntry = (license: LicenseIndicator): StatusMenuEntry | null => {
  if (licenseBarNotice(license) === null || license.kind !== "status") {
    return null;
  }
  if (license.status.state === "missing") {
    return { label: "$(key) Activate license", command: "fallow.license.activate" };
  }
  return { label: "$(key) Refresh license", command: "fallow.license.refresh" };
};

/** Rows of the click menu, in display order. Each row runs an existing command. */
export const buildStatusMenuEntries = (model: StatusModel): StatusMenuEntry[] => {
  const entries: StatusMenuEntry[] = [
    { label: "$(play) Run analysis", command: "fallow.analyze" },
    model.findings.mutedAll
      ? {
          label: "$(eye) Show findings in the editor",
          command: "fallow.toggleAllDiagnostics",
        }
      : {
          label: "$(eye-closed) Hide findings in the editor",
          description: "CI and fallow check are unaffected",
          command: "fallow.toggleAllDiagnostics",
        },
  ];
  const auditDescription = auditMenuDescription(model);
  entries.push(
    auditDescription
      ? { label: "$(shield) Run audit", description: auditDescription, command: "fallow.audit" }
      : { label: "$(shield) Run audit", command: "fallow.audit" },
  );
  if (model.workspace.pickerAvailable) {
    entries.push({
      label: "$(layers) Switch workspace",
      description: model.workspace.active || "whole project",
      command: "fallow.selectWorkspace",
    });
  }
  const license = licenseMenuEntry(model.license);
  if (license) {
    entries.push(license);
  }
  entries.push({ label: "$(gear) Open settings", command: "fallow.openSettings" });
  return entries;
};
