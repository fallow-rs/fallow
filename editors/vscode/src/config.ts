import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { clampMinLines, clampMinOccurrences } from "./duplication-utils.js";
import type {
  AuditGate,
  DiagnosticSeveritySetting,
  DuplicationMode,
  IssueTypeConfig,
  TraceLevel,
} from "./types.js";
import { getDiagnosticCategories } from "./diagnosticFilter.js";

const SECTION = "fallow";

const getConfig = (): vscode.WorkspaceConfiguration => vscode.workspace.getConfiguration(SECTION);

const getConfiguredValue = <T>(key: string): T | undefined => {
  const inspected = getConfig().inspect<T>(key);
  return (
    inspected?.workspaceFolderLanguageValue ??
    inspected?.workspaceLanguageValue ??
    inspected?.globalLanguageValue ??
    inspected?.workspaceFolderValue ??
    inspected?.workspaceValue ??
    inspected?.globalValue
  );
};

export const getLspPath = (): string => getConfig().get<string>("lspPath", "");

const getConfigPath = (): string => getConfig().get<string>("configPath", "").trim();

export const getResolvedConfigPath = (): string => {
  const configPath = getConfigPath();
  if (!configPath || path.isAbsolute(configPath)) {
    return configPath;
  }

  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return workspaceRoot ? path.resolve(workspaceRoot, configPath) : configPath;
};

export const getAutoDownload = (): boolean => getConfig().get<boolean>("autoDownload", true);

export const getIssueTypes = (): IssueTypeConfig =>
  getConfig().get<IssueTypeConfig>("issueTypes", {
    "unused-files": true,
    "unused-exports": true,
    "unused-types": true,
    "private-type-leaks": true,
    "unused-dependencies": true,
    "unused-dev-dependencies": true,
    "unused-optional-dependencies": true,
    "unused-enum-members": true,
    "unused-class-members": true,
    "unused-store-member": true,
    "unused-server-action": true,
    "unused-load-data-key": true,
    "unused-component-prop": true,
    "unused-component-emit": true,
    "unused-component-input": true,
    "unused-component-output": true,
    "unrendered-component": true,
    "unprovided-inject": true,
    "invalid-client-export": true,
    "mixed-client-server-barrel": true,
    "misplaced-directive": true,
    "route-collision": true,
    "dynamic-segment-name-conflict": true,
    "unresolved-imports": true,
    "unlisted-dependencies": true,
    "duplicate-exports": true,
    "type-only-dependencies": true,
    "test-only-dependencies": true,
    "circular-dependencies": true,
    "re-export-cycles": true,
    "boundary-violation": true,
    "policy-violation": true,
    "stale-suppressions": true,
    "unused-catalog-entries": true,
    "unresolved-catalog-references": true,
    "unused-dependency-overrides": true,
    "misconfigured-dependency-overrides": true,
  });

export const getDuplicationThresholdOverride = (): number | undefined =>
  getConfiguredValue<number>("duplication.threshold");

export const getDuplicationMinTokensOverride = (): number | undefined =>
  getConfiguredValue<number>("duplication.minTokens");

export const getDuplicationMinLinesOverride = (): number | undefined => {
  const value = getConfiguredValue<number>("duplication.minLines");
  return value === undefined ? undefined : clampMinLines(value);
};

const DUPLICATION_MODES: ReadonlySet<string> = new Set<DuplicationMode>([
  "strict",
  "mild",
  "weak",
  "semantic",
]);

export const getDuplicationModeOverride = (): DuplicationMode | undefined => {
  const value = getConfiguredValue<DuplicationMode>("duplication.mode");
  // A hand-edited settings.json can hold any string. An unknown value would be
  // forwarded as `--dupes-mode <bad>`, which the CLI rejects; `--dupes-mode` is
  // not version-gated, so planDegradation rethrows and the WHOLE analysis run
  // fails instead of degrading. Drop invalid input and fall back to the CLI
  // default rather than poisoning the run.
  return value !== undefined && DUPLICATION_MODES.has(value) ? value : undefined;
};

export const getDuplicationMinOccurrencesOverride = (): number | undefined => {
  const value = getConfiguredValue<number>("duplication.minOccurrences");
  return value === undefined ? undefined : clampMinOccurrences(value);
};

export const getDuplicationSkipLocalOverride = (): boolean | undefined =>
  getConfiguredValue<boolean>("duplication.skipLocal");

export const getDuplicationCrossLanguageOverride = (): boolean | undefined =>
  getConfiguredValue<boolean>("duplication.crossLanguage");

export const getDuplicationIgnoreImportsOverride = (): boolean | undefined =>
  getConfiguredValue<boolean>("duplication.ignoreImports");

/**
 * Resolve `fallow.production` to a production-mode override forwarded to BOTH
 * the CLI-driven sidebar AND the LSP so the two editor surfaces agree. `true`
 * (`"on"`) forces production on, `false` (`"off"`) forces it off, `undefined`
 * (`"auto"`, the default, or unset) defers to the project `.fallowrc.json`. Uses
 * the inspect-based override pattern (like the `duplication.*` getters) so an
 * unset editor value never overrides project config, and accepts a legacy
 * stored boolean (the pre-enum setting shape) as on/off (issue #1055).
 */
export const getProductionOverride = (): boolean | undefined => {
  const value = getConfiguredValue<string | boolean>("production");
  if (value === "on" || value === true) {
    return true;
  }
  if (value === "off" || value === false) {
    return false;
  }
  return undefined;
};

const getCoverageCapturePath = (): string =>
  getConfig().get<string>("coverage.capturePath", "").trim();

/**
 * Resolve `fallow.coverage.capturePath` to an absolute path against the
 * workspace root, mirroring `getResolvedConfigPath`. Empty when unset.
 */
export const getCoveragePath = (): string => {
  const capturePath = getCoverageCapturePath();
  if (!capturePath || path.isAbsolute(capturePath)) {
    return capturePath;
  }

  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return workspaceRoot ? path.resolve(workspaceRoot, capturePath) : capturePath;
};

/** `--top N` cap on hot paths and findings; `0` (default) means no cap. */
export const getCoverageTop = (): number => getConfig().get<number>("coverage.top", 0);

/**
 * Which findings affect the audit verdict. Mirrors `fallow audit --gate`:
 * `new-only` (default) fails on findings introduced by the current change set,
 * `all` fails on every finding in changed files.
 */
export const getAuditGate = (): AuditGate => getConfig().get<AuditGate>("audit.gate", "new-only");

/**
 * Whether the audit verdict status-bar item is created. Default on; creating an
 * idle item runs no analysis, so this only controls the surface, not cost.
 */
export const getAuditEnabled = (): boolean =>
  getConfig().get<boolean>("audit.statusBar.enabled", true);

/**
 * Whether the diagnostics on/off toggle status-bar item is shown. Default on so
 * a first-install user has a visible button to hide the squiggles; the item runs
 * no analysis, so this only controls the surface, not cost.
 */
export const getDiagnosticStatusBar = (): boolean =>
  getConfig().get<boolean>("diagnostics.statusBar", true);

export const getDiagnosticSeverity = (): DiagnosticSeveritySetting => {
  const value = getConfig().get<string>("diagnostics.severity", "warning");
  return value === "information" || value === "hint" ? value : "warning";
};

export const getMutedDiagnosticCategories = (): ReadonlySet<string> => {
  const configured = getConfig().get<unknown>("diagnostics.mutedCategories", []);
  if (!Array.isArray(configured)) {
    return new Set();
  }

  const known = new Set(getDiagnosticCategories().map(({ code }) => code));
  const muted = new Set<string>();
  for (const value of configured) {
    if (typeof value === "string" && known.has(value)) {
      muted.add(value);
    }
  }
  return muted;
};

/**
 * Whether to re-run the audit on save of a JS/TS file. Default OFF so it cannot
 * regress idle latency; the command and status-bar item are the primary entry
 * points.
 */
export const getAuditRunOnSave = (): boolean => getConfig().get<boolean>("audit.runOnSave", false);

export const getChangedSince = (): string => getConfig().get<string>("changedSince", "").trim();

export const getHealthEnabled = (): boolean => getConfig().get<boolean>("health.enabled", true);

export const getHealthHotspots = (): boolean => getConfig().get<boolean>("health.hotspots", true);

export const getHealthTopFindings = (): number => {
  const value = getConfig().get<number>("health.topFindings", 20);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : 20;
};

export const getHealthStatusBar = (): boolean => getConfig().get<boolean>("health.statusBar", true);

export const getHealthInlineComplexity = (): boolean =>
  getConfig().get<boolean>("health.inlineComplexity", true);

/** Whether the inline complexity breakdown (per-line markers + hover) is shown. */
export const getComplexityBreakdownEnabled = (): boolean =>
  getConfig().get<boolean>("complexity.breakdownEnabled", true);

/**
 * Whether the inline `+N` after-text tier is rendered. When false the hover
 * still attaches, so a user can keep the quiet tier without the dense per-line
 * text. Inert unless `complexity.breakdownEnabled` is on.
 */
export const getComplexityAfterText = (): boolean =>
  getConfig().get<boolean>("complexity.afterText", false);

/**
 * How many top complexity findings to fetch for inline decorations when the
 * breakdown is enabled, decoupled from the tree's `health.topFindings` so an
 * open file outside the tree's top-N still gets decorated. The health spawn
 * requests `max(topFindings, decorationCap)`; the tree still displays only
 * `topFindings`.
 */
export const getComplexityDecorationCap = (): number => {
  const value = getConfig().get<number>("complexity.decorationCap", 200);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : 200;
};

/**
 * The pinned `fallow.workspace` setting (a monorepo package name). Empty =
 * whole project. A per-folder `workspaceState` override set via the picker
 * takes precedence over this; see `resolveWorkspaceScope`.
 */
export const getWorkspaceScope = (): string => getConfig().get<string>("workspace", "").trim();

export const getTraceLevel = (): TraceLevel => getConfig().get<TraceLevel>("trace.server", "off");

/**
 * Whether the opt-in Security Candidates view is enabled. Off by default: no
 * `fallow security` process runs and no security view work happens until the
 * user turns this on AND opens the Security Candidates view (#902 latency
 * protection; #903 enable toggle).
 */
export const getSecurityEnabled = (): boolean =>
  getConfig().get<boolean>("security.enabled", false);

export const getLicenseShowStatusBar = (): boolean =>
  getConfig().get<boolean>("license.showStatusBar", true);

export const getLicenseRefreshOnStartup = (): boolean =>
  getConfig().get<boolean>("license.refreshOnStartup", false);

export const onConfigChange = (
  callback: (e: vscode.ConfigurationChangeEvent) => void,
): vscode.Disposable =>
  vscode.workspace.onDidChangeConfiguration((e) => {
    if (e.affectsConfiguration(SECTION)) {
      callback(e);
    }
  });
