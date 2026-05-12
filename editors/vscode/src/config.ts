import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type { DuplicationMode, IssueTypeConfig, TraceLevel } from "./types.js";

const SECTION = "fallow";

const getConfig = (): vscode.WorkspaceConfiguration =>
  vscode.workspace.getConfiguration(SECTION);

export const getLspPath = (): string => getConfig().get<string>("lspPath", "");

const getConfigPath = (): string =>
  getConfig().get<string>("configPath", "").trim();

export const getResolvedConfigPath = (): string => {
  const configPath = getConfigPath();
  if (!configPath || path.isAbsolute(configPath)) {
    return configPath;
  }

  const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return workspaceRoot ? path.resolve(workspaceRoot, configPath) : configPath;
};

export const getAutoDownload = (): boolean =>
  getConfig().get<boolean>("autoDownload", true);

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
    "boundary-violation": true,
    "stale-suppressions": true,
    "unused-catalog-entries": true,
    "unresolved-catalog-references": true,
    "unused-dependency-overrides": true,
    "misconfigured-dependency-overrides": true,
  });

export const getDuplicationThreshold = (): number =>
  getConfig().get<number>("duplication.threshold", 5);

export const getDuplicationMode = (): DuplicationMode =>
  getConfig().get<DuplicationMode>("duplication.mode", "mild");

export const getProduction = (): boolean =>
  getConfig().get<boolean>("production", false);

export const getChangedSince = (): string =>
  getConfig().get<string>("changedSince", "").trim();

export const getTraceLevel = (): TraceLevel =>
  getConfig().get<TraceLevel>("trace.server", "off");

export const onConfigChange = (
  callback: (e: vscode.ConfigurationChangeEvent) => void
): vscode.Disposable =>
  vscode.workspace.onDidChangeConfiguration((e) => {
    if (e.affectsConfiguration(SECTION)) {
      callback(e);
    }
  });
