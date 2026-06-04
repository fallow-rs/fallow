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
  renderWorkspaceStatusBarText,
  renderWorkspaceStatusBarTooltip,
  resolveWorkspaceScope,
  shouldShowWorkspacePicker,
} from "./workspacePicker-utils.js";
import type { WorkspaceQuickPickItem } from "./workspacePicker-utils.js";
import type { WorkspacesOutput } from "./workspace-types.js";

export { parseWorkspacesOutput } from "./workspacePicker-utils.js";

/**
 * `workspaceState` key holding the picker's per-folder workspace-scope
 * override (a package name). Absent / empty = fall back to the
 * `fallow.workspace` setting, then to whole-project.
 */
const WORKSPACE_STATE_KEY = "fallow.workspaceScope";

let pickerItem: vscode.StatusBarItem | null = null;

/**
 * The most recent `fallow workspaces` output observed this session, used to
 * decide whether the picker is worth showing (hidden on single-package repos,
 * n2). `null` means "not yet probed" and keeps the item shown so it stays
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

/**
 * The scope that remains in effect once the picker override is cleared: the
 * `fallow.workspace` setting still scopes the analysis when pinned. Empty means
 * truly whole-project. Used to phrase the clear toast honestly (the override no
 * longer wins, but a pinned setting still scopes).
 */
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

export const createWorkspacePicker = (context: vscode.ExtensionContext): vscode.StatusBarItem => {
  // Priority 49 sits just to the right of the main Fallow status item (50).
  pickerItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 49);
  pickerItem.command = "fallow.selectWorkspace";
  refreshWorkspacePicker(context);
  return pickerItem;
};

/**
 * Show or hide the picker based on the resolved workspaces list. A
 * single-package repo (or one whose workspaces could not be listed) gets no
 * picker, since scoping is meaningless there. `null` keeps it shown (the list
 * was not probed, e.g. older CLI). Called after a lazy `fallow workspaces`
 * probe so the picker never appears on a repo that can never use it (n2).
 */
export const applyWorkspaceVisibility = (output: WorkspacesOutput | null): void => {
  lastWorkspacesOutput = output;
  if (!pickerItem) {
    return;
  }
  if (shouldShowWorkspacePicker(output)) {
    pickerItem.show();
  } else {
    pickerItem.hide();
  }
};

/** Re-render the picker status-bar item from the current resolved scope. */
export const refreshWorkspacePicker = (context: vscode.ExtensionContext): void => {
  if (!pickerItem) {
    return;
  }
  const active = resolveActiveWorkspaceScope(context);
  pickerItem.text = renderWorkspaceStatusBarText(active);
  pickerItem.tooltip = renderWorkspaceStatusBarTooltip(active);
  // Re-apply visibility so a re-render does not resurrect a hidden picker on a
  // single-package repo.
  if (shouldShowWorkspacePicker(lastWorkspacesOutput)) {
    pickerItem.show();
  } else {
    pickerItem.hide();
  }
};

export const disposeWorkspacePicker = (): void => {
  if (pickerItem) {
    pickerItem.dispose();
    pickerItem = null;
  }
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
 * re-render the picker and re-run analysis. Returns the chosen scope, or
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

    // On clear, report the ACTUAL residual scope: a pinned `fallow.workspace`
    // setting still scopes the analysis, so "whole project" would be false.
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
 * Clear the per-folder scope override back to whole-project. Returns true when
 * a change was made (so the caller can skip a no-op re-analysis).
 */
export const clearWorkspaceScope = async (context: vscode.ExtensionContext): Promise<boolean> => {
  const previous = getWorkspaceStateOverride(context);
  const residual = resolveResidualScope();
  if (previous === CLEAR_WORKSPACE_SCOPE) {
    // No picker override to clear. Report the residual scope honestly: a pinned
    // `fallow.workspace` setting still scopes the analysis.
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
