// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getWorkspaceScope } from "./config.js";
import {
  CLEAR_WORKSPACE_SCOPE,
  buildWorkspaceQuickPickItems,
  partitionWorkspaces,
  renderWorkspaceStatusBarText,
  renderWorkspaceStatusBarTooltip,
  resolveWorkspaceScope,
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
  pickerItem.show();
  return pickerItem;
};

/** Re-render the picker status-bar item from the current resolved scope. */
export const refreshWorkspacePicker = (context: vscode.ExtensionContext): void => {
  if (!pickerItem) {
    return;
  }
  const active = resolveActiveWorkspaceScope(context);
  pickerItem.text = renderWorkspaceStatusBarText(active);
  pickerItem.tooltip = renderWorkspaceStatusBarTooltip(active);
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

    void vscode.window.showInformationMessage(
      next === CLEAR_WORKSPACE_SCOPE
        ? "Fallow: scope cleared. Analyzing the whole project."
        : `Fallow: scoped to ${next}.`,
    );
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
  if (previous === CLEAR_WORKSPACE_SCOPE) {
    void vscode.window.showInformationMessage("Fallow: already analyzing the whole project.");
    return false;
  }
  await context.workspaceState.update(WORKSPACE_STATE_KEY, CLEAR_WORKSPACE_SCOPE);
  void vscode.window.showInformationMessage("Fallow: scope cleared. Analyzing the whole project.");
  return true;
};
