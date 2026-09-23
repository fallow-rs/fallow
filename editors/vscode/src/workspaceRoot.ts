// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { resolveFilePath, type ResolvedPath } from "./treeView-utils.js";

/** The first workspace folder, or null when no folder is open. */
export const getWorkspaceRoot = (): string | null =>
  vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? null;

/** Resolve a report path against the first workspace folder. */
export const resolveWorkspaceFilePath = (filePath: string | undefined): ResolvedPath =>
  resolveFilePath(filePath, getWorkspaceRoot() ?? undefined);
