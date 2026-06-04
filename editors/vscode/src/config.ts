import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { clampMinLines, clampMinOccurrences } from "./duplication-utils.js";
import type { DuplicationMode, IssueTypeConfig, TraceLevel } from "./types.js";

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
    "unresolved-imports": true,
    "unlisted-dependencies": true,
    "duplicate-exports": true,
    "type-only-dependencies": true,
    "test-only-dependencies": true,
    "circular-dependencies": true,
    "re-export-cycles": true,
    "boundary-violation": true,
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

export const getDuplicationModeOverride = (): DuplicationMode | undefined =>
  getConfiguredValue<DuplicationMode>("duplication.mode");

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

export const getProduction = (): boolean => getConfig().get<boolean>("production", false);

export const getChangedSince = (): string => getConfig().get<string>("changedSince", "").trim();

export const getHealthEnabled = (): boolean => getConfig().get<boolean>("health.enabled", true);

export const getHealthHotspots = (): boolean => getConfig().get<boolean>("health.hotspots", true);

export const getHealthTopFindings = (): number => {
  const value = getConfig().get<number>("health.topFindings", 20);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : 20;
};

export const getHealthStatusBar = (): boolean =>
  getConfig().get<boolean>("health.statusBar", true);

export const getTraceLevel = (): TraceLevel => getConfig().get<TraceLevel>("trace.server", "off");

export const onConfigChange = (
  callback: (e: vscode.ConfigurationChangeEvent) => void,
): vscode.Disposable =>
  vscode.workspace.onDidChangeConfiguration((e) => {
    if (e.affectsConfiguration(SECTION)) {
      callback(e);
    }
  });
