/**
 * Pure helpers for the monorepo workspace picker. No `vscode` import, so the
 * parse / partition / argv / label rules can be unit-tested in isolation
 * (mirrors the `statusBar-utils.ts` / `analysis-utils.ts` split).
 */
import type { WorkspaceInfo, WorkspacesOutput } from "./workspace-types.js";

/**
 * The synthetic name persisted to `workspaceState` / read from the
 * `fallow.workspace` setting that represents "analyze the whole project".
 * Empty string is the inert default, identical to today's behavior.
 */
export const CLEAR_WORKSPACE_SCOPE = "";

/** A real package and a generated/platform package, split for display. */
export interface PartitionedWorkspaces {
  readonly real: ReadonlyArray<WorkspaceInfo>;
  readonly internal: ReadonlyArray<WorkspaceInfo>;
}

/**
 * Parse `fallow workspaces --format json` stdout into the typed envelope.
 * Returns null on empty input, invalid JSON, or a payload missing the
 * `workspaces` array, so the caller can show an actionable message rather
 * than throw. Malformed individual entries are dropped, not fatal.
 */
export const parseWorkspacesOutput = (stdout: string): WorkspacesOutput | null => {
  const trimmed = stdout.trim();
  if (trimmed.length === 0) {
    return null;
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return null;
  }

  if (typeof parsed !== "object" || parsed === null) {
    return null;
  }

  const candidate = parsed as { workspaces?: unknown; workspace_count?: unknown };
  if (!Array.isArray(candidate.workspaces)) {
    return null;
  }

  const workspaces: WorkspaceInfo[] = [];
  for (const entry of candidate.workspaces) {
    if (typeof entry !== "object" || entry === null) {
      continue;
    }
    const record = entry as Record<string, unknown>;
    if (typeof record.name !== "string" || record.name.length === 0) {
      continue;
    }
    workspaces.push({
      name: record.name,
      path: typeof record.path === "string" ? record.path : "",
      is_internal_dependency: record.is_internal_dependency === true,
    });
  }

  return {
    workspace_count:
      typeof candidate.workspace_count === "number" ? candidate.workspace_count : workspaces.length,
    workspaces,
  };
};

/**
 * Split workspaces into real (hand-authored) packages and internal
 * (generated / platform) packages, each sorted by name. The picker lists
 * real packages first; internal ones are demoted under a separator.
 */
export const partitionWorkspaces = (
  workspaces: ReadonlyArray<WorkspaceInfo>,
): PartitionedWorkspaces => {
  const byName = (a: WorkspaceInfo, b: WorkspaceInfo): number => a.name.localeCompare(b.name);
  const real = workspaces.filter((w) => !w.is_internal_dependency).toSorted(byName);
  const internal = workspaces.filter((w) => w.is_internal_dependency).toSorted(byName);
  return { real, internal };
};

/**
 * Whether the workspace picker status-bar item is worth showing for a given
 * resolved workspaces list. A single-package repo (or one with no listable
 * workspaces) can never use scoping, so the picker is hidden there to avoid a
 * dead status-bar control. `null` (the list was never probed / unavailable)
 * keeps the item shown, so the picker stays reachable on an older CLI that
 * cannot list workspaces. Pure so the rule is unit-tested without a status bar.
 */
export const shouldShowWorkspacePicker = (output: WorkspacesOutput | null): boolean => {
  if (output === null) {
    return true;
  }
  return output.workspaces.length > 1;
};

/** Kinds of entries the picker renders, so the UI layer needs no `vscode` enum here. */
export type WorkspaceQuickPickItemKind = "clear" | "package" | "separator" | "refresh";

/**
 * A vscode-agnostic description of one QuickPick row. The picker maps these to
 * real `vscode.QuickPickItem`s (separators get `QuickPickItemKind.Separator`).
 * `name` carries the value to persist for `package`/`clear` rows.
 */
export interface WorkspaceQuickPickItem {
  readonly kind: WorkspaceQuickPickItemKind;
  readonly label: string;
  readonly description?: string;
  /** The `--workspace` value for `clear` (empty) and `package` rows. */
  readonly name?: string;
}

const REFRESH_LABEL = "$(refresh) Refresh workspace list";

/**
 * Build the ordered QuickPick rows: an "All workspaces" reset first, then the
 * real packages, then (if any) a separator and the internal packages, and
 * finally a refresh row. The `active` scope is annotated so the user sees the
 * current selection.
 */
export const buildWorkspaceQuickPickItems = (
  partitioned: PartitionedWorkspaces,
  active: string,
): ReadonlyArray<WorkspaceQuickPickItem> => {
  const items: WorkspaceQuickPickItem[] = [];

  items.push({
    kind: "clear",
    label: "$(layers) All workspaces",
    description: active === CLEAR_WORKSPACE_SCOPE ? "Current scope" : "Clear scope",
    name: CLEAR_WORKSPACE_SCOPE,
  });

  for (const ws of partitioned.real) {
    items.push({
      kind: "package",
      label: ws.name,
      description: active === ws.name ? `${ws.path} · Current scope` : ws.path,
      name: ws.name,
    });
  }

  if (partitioned.internal.length > 0) {
    items.push({ kind: "separator", label: "Generated packages" });
    for (const ws of partitioned.internal) {
      items.push({
        kind: "package",
        label: ws.name,
        description: active === ws.name ? `${ws.path} · Current scope` : ws.path,
        name: ws.name,
      });
    }
  }

  items.push({ kind: "separator", label: "" });
  items.push({ kind: "refresh", label: REFRESH_LABEL });

  return items;
};

/**
 * The status-bar label for the picker item. Unscoped reads
 * `$(layers) Fallow: All`; a scoped selection reads `$(layers) <pkg>`.
 * Pure so it can be unit-tested without a status-bar mock.
 */
export const renderWorkspaceStatusBarText = (active: string): string =>
  active === CLEAR_WORKSPACE_SCOPE ? "$(layers) Fallow: All" : `$(layers) ${active}`;

/**
 * Disclosure appended to the picker tooltip and select/clear toasts. The scope
 * drives the CLI-backed views (Unused Code, Duplicates, Health, Security); the
 * LSP is not yet workspace-scoped, so editor gutter diagnostics stay
 * project-wide. Stated plainly so a scoped developer is not surprised by
 * whole-project squiggles. Health and Security ARE scoped (see #906 C2).
 */
export const WORKSPACE_SCOPE_DISCLOSURE =
  "Scopes the Unused Code, Duplicates, Health, and Security views; editor diagnostics remain project-wide for now.";

/** Tooltip for the picker status-bar item. Neutral copy: it scopes, not judges. */
export const renderWorkspaceStatusBarTooltip = (active: string): string =>
  active === CLEAR_WORKSPACE_SCOPE
    ? `Fallow: analyzing the whole project. Click to scope to a single workspace. ${WORKSPACE_SCOPE_DISCLOSURE}`
    : `Fallow: scoped to ${active}. Click to change or clear the scope. ${WORKSPACE_SCOPE_DISCLOSURE}`;

/**
 * Resolve the effective workspace scope. A per-folder `workspaceState`
 * override (set via the picker) wins; otherwise the `fallow.workspace`
 * setting provides a pinned default; empty means whole-project. Mirrors the
 * `changedSince` precedent. `undefined` for either input is treated as unset.
 */
export const resolveWorkspaceScope = (
  override: string | undefined,
  setting: string | undefined,
): string => {
  if (override !== undefined && override.trim().length > 0) {
    return override.trim();
  }
  if (setting !== undefined && setting.trim().length > 0) {
    return setting.trim();
  }
  return CLEAR_WORKSPACE_SCOPE;
};

/**
 * The info-toast text shown after the user clears the picker override.
 *
 * Clearing the override does not always reach whole-project: a pinned
 * `fallow.workspace` setting still scopes the analysis. So the message reports
 * the ACTUAL residual scope. `residualScope` is the scope after the override is
 * cleared, i.e. `resolveWorkspaceScope("", settingScope)`: empty means
 * whole-project, a non-empty value is the still-pinned setting. Pure so the
 * branch is unit-tested without a `vscode` mock.
 */
export const clearedScopeToast = (residualScope: string): string =>
  residualScope === CLEAR_WORKSPACE_SCOPE
    ? "Fallow: scope cleared. Analyzing the whole project."
    : `Fallow: cleared the picker override; still scoped to ${residualScope} via the fallow.workspace setting.`;
