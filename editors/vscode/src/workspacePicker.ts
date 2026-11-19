// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getWorkspaceScope } from "./config.js";
import {
  CLEAR_WORKSPACE_SCOPE,
  WORKSPACE_SCOPE_DISCLOSURE,
  buildWorkspaceQuickPickItems,
  clearedScopeToast,
  partitionWorkspaces,
  resolveWorkspaceScope,
  shouldShowWorkspacePicker,
} from "./workspacePicker-utils.js";
import type { WorkspaceQuickPickItem } from "./workspacePicker-utils.js";
import { setStatusBarWorkspace } from "./statusBar.js";
import type { WorkspacesOutput } from "./types.js";

export { parseWorkspacesOutput } from "./workspacePicker-utils.js";

/**
 * `workspaceState` key holding the picker's per-folder workspace-scope
 * override (a package name). Absent / empty = fall back to the
 * `fallow.workspace` setting, then to whole-project.
 */
const WORKSPACE_STATE_KEY = "fallow.workspaceScope";

/**
 * The most recent `fallow workspaces` output observed this session. It decides
 * whether the status bar item shows the workspace entry (not on single-package
 * repos, n2). `null` means "not yet probed" and keeps the entry, so it stays
 * reachable until a probe resolves.
 */
let lastWorkspacesOutput: WorkspacesOutput | null = null;

/**
 * Per-session cache of `fallow workspaces` output keyed by binary path. The
 * package list does not change within a session for a given binary, so probe
 * once on first picker open and reuse; a "Refresh" QuickPick entry busts it.
 * Mirrors `cliVersionCache` in `commands.ts`.
 */
const workspacesCache = new Map<string, WorkspacesOutput>();

/** Read the persisted per-folder override (empty string when unset). */
const getWorkspaceStateOverride = (context: vscode.ExtensionContext): string =>
  context.workspaceState.get<string>(WORKSPACE_STATE_KEY, CLEAR_WORKSPACE_SCOPE);

/**
 * Resolve the effective workspace scope: `workspaceState` override (picker)
 * wins, else the `fallow.workspace` setting, else whole-project.
 */
export const resolveActiveWorkspaceScope = (context: vscode.ExtensionContext): string =>
  resolveWorkspaceScope(getWorkspaceStateOverride(context), getWorkspaceScope());

/** The `fallow.workspace` setting still applies after the user clears the picker override. */
const resolveResidualScope = (): string =>
  resolveWorkspaceScope(CLEAR_WORKSPACE_SCOPE, getWorkspaceScope());

/** Cache the parsed `workspaces` output for a binary path; null clears it. */
export const cacheWorkspacesOutput = (
  binaryPath: string,
  output: WorkspacesOutput | null,
): void => {
  if (output) {
    workspacesCache.set(binaryPath, output);
  } else {
    workspacesCache.delete(binaryPath);
  }
};

export const getCachedWorkspacesOutput = (binaryPath: string): WorkspacesOutput | undefined =>
  workspacesCache.get(binaryPath);

/**
 * Show the workspace state in the status bar item. A single-package repo (or
 * one whose workspaces could not be listed) gets no workspace entry, since
 * scoping is meaningless there. `null` keeps the entry (the list was not
 * probed, for example on an older CLI). Called after a lazy `fallow workspaces`
 * probe (n2).
 */
export const applyWorkspaceVisibility = (
  context: vscode.ExtensionContext,
  output: WorkspacesOutput | null,
): void => {
  lastWorkspacesOutput = output;
  refreshWorkspacePicker(context);
};

/** Re-render the workspace state of the status bar item from the resolved scope. */
export const refreshWorkspacePicker = (context: vscode.ExtensionContext): void => {
  setStatusBarWorkspace({
    pickerAvailable: shouldShowWorkspacePicker(lastWorkspacesOutput),
    active: resolveActiveWorkspaceScope(context),
  });
};

interface WorkspaceScopeQuickPick extends vscode.QuickPickItem {
  readonly item: WorkspaceQuickPickItem;
}

const toQuickPickItems = (rows: ReadonlyArray<WorkspaceQuickPickItem>): WorkspaceScopeQuickPick[] =>
  rows.map((row) => ({
    label: row.label,
    description: row.description,
    kind:
      row.kind === "separator"
        ? vscode.QuickPickItemKind.Separator
        : vscode.QuickPickItemKind.Default,
    item: row,
  }));

/**
 * Show the workspace-scope QuickPick and persist the user's choice to
 * `workspaceState`. `loadWorkspaces` performs the (cached) `fallow workspaces`
 * probe; `onScopeChange` is invoked after a real change so the caller can
 * re-render the status bar item and re-run analysis. Returns the chosen scope, or
 * undefined when the user dismissed the picker without changing anything.
 */
export const showWorkspacePicker = async (
  context: vscode.ExtensionContext,
  loadWorkspaces: (forceRefresh: boolean) => Promise<WorkspacesOutput | null>,
  onScopeChange: () => void,
): Promise<string | undefined> => {
  const active = resolveActiveWorkspaceScope(context);

  const present = async (forceRefresh: boolean): Promise<string | undefined> => {
    const output = await loadWorkspaces(forceRefresh);
    if (!output) {
      void vscode.window.showWarningMessage(
        "Fallow: could not list workspaces. Ensure this is a monorepo and the fallow CLI is available (see the Fallow output channel).",
      );
      return undefined;
    }

    const partitioned = partitionWorkspaces(output.workspaces);
    if (partitioned.real.length === 0 && partitioned.internal.length === 0) {
      void vscode.window.showInformationMessage(
        "Fallow: no workspace packages found. Scoping applies to monorepos with multiple packages.",
      );
      return undefined;
    }

    const picked = await vscode.window.showQuickPick(
      toQuickPickItems(buildWorkspaceQuickPickItems(partitioned, active)),
      {
        title: "Fallow: Select Workspace Scope",
        placeHolder:
          active === CLEAR_WORKSPACE_SCOPE
            ? "Analyzing the whole project. Pick a package to scope."
            : `Scoped to ${active}. Pick another package or clear the scope.`,
      },
    );

    if (!picked) {
      return undefined;
    }

    if (picked.item.kind === "refresh") {
      return present(true);
    }

    const next = picked.item.name ?? CLEAR_WORKSPACE_SCOPE;
    if (next === active) {
      return next;
    }

    await context.workspaceState.update(WORKSPACE_STATE_KEY, next);
    onScopeChange();

    const message =
      next === CLEAR_WORKSPACE_SCOPE
        ? clearedScopeToast(resolveResidualScope())
        : `Fallow: scoped to ${next}.`;
    void vscode.window.showInformationMessage(`${message} ${WORKSPACE_SCOPE_DISCLOSURE}`);
    return next;
  };

  return present(false);
};

/**
 * Clear the per-folder picker override. The `fallow.workspace` setting still applies.
 * Return true only if the override changed, so callers can skip a no-op analysis.
 */
export const clearWorkspaceScope = async (context: vscode.ExtensionContext): Promise<boolean> => {
  const previous = getWorkspaceStateOverride(context);
  const residual = resolveResidualScope();
  if (previous === CLEAR_WORKSPACE_SCOPE) {
    const message =
      residual === CLEAR_WORKSPACE_SCOPE
        ? "Fallow: already analyzing the whole project."
        : `Fallow: no picker override to clear; still scoped to ${residual} via the fallow.workspace setting.`;
    void vscode.window.showInformationMessage(message);
    return false;
  }
  await context.workspaceState.update(WORKSPACE_STATE_KEY, CLEAR_WORKSPACE_SCOPE);
  void vscode.window.showInformationMessage(clearedScopeToast(residual));
  return true;
};
