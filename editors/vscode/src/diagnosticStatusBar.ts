// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type { DiagnosticFilter } from "./diagnosticFilter.js";

let item: vscode.StatusBarItem | null = null;
let changeSub: vscode.Disposable | null = null;

const CI_SAFE = "CI and `fallow check` are unaffected.";

export interface DiagnosticToggleStatusPresentation {
  readonly text: string;
  readonly tooltip: string;
}

/**
 * Pure presentation for the diagnostics on/off status-bar item, derived from
 * the shared {@link DiagnosticFilter} state. The icon alone carries the state
 * (no background color): a deliberate, persistent mute is not an error
 * condition, so it must not borrow the audit item's warning tint.
 */
export const diagnosticTogglePresentation = (
  mutedAll: boolean,
  mutedCategoryCount: number,
): DiagnosticToggleStatusPresentation => {
  if (mutedAll) {
    return {
      text: "$(eye-closed) Fallow: hidden",
      tooltip:
        "All Fallow findings are hidden in this editor. Click to show. " +
        "CI and `fallow check` still report every finding.",
    };
  }
  if (mutedCategoryCount > 0) {
    const noun = mutedCategoryCount === 1 ? "category" : "categories";
    return {
      text: "$(eye) Fallow",
      tooltip:
        `Fallow findings are visible (${mutedCategoryCount} ${noun} hidden via Manage). ` +
        `Click to hide all in this editor; the per-category hides are kept when you show again. ${CI_SAFE}`,
    };
  }
  return {
    text: "$(eye) Fallow",
    tooltip: `Fallow findings are visible. Click to hide all in this editor. ${CI_SAFE}`,
  };
};

const applyPresentation = (target: vscode.StatusBarItem, filter: DiagnosticFilter): void => {
  const presentation = diagnosticTogglePresentation(
    filter.isMutedAll(),
    filter.mutedCategoriesSnapshot().size,
  );
  target.text = presentation.text;
  // Non-trusted: no command links, no user input, no codicons in the body, so
  // no escaping is needed; backticks just render `fallow check` as inline code.
  target.tooltip = new vscode.MarkdownString(presentation.tooltip);
};

/**
 * Create the always-visible diagnostics on/off status-bar item, just right of
 * the audit item (priority 48 vs 49 vs the main item's 50). Reuses the existing
 * `fallow.toggleAllDiagnostics` command and the shared filter, so it stays in
 * sync with the right-gutter Language Status item, the Manage QuickPick, and
 * code-action mutes. Left uncolored, mirroring the main item.
 */
export const createDiagnosticStatusBar = (filter: DiagnosticFilter): vscode.StatusBarItem => {
  // Idempotent: dispose any prior item + its filter subscription first so a
  // double-create (e.g. a stray call outside the sync guard) cannot leak the
  // earlier onDidChange listener.
  disposeDiagnosticStatusBar();
  item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 48);
  item.command = "fallow.toggleAllDiagnostics";
  applyPresentation(item, filter);
  changeSub = filter.onDidChange(() => {
    if (item) {
      applyPresentation(item, filter);
    }
  });
  item.show();
  return item;
};

/** Whether the diagnostics toggle status-bar item currently exists. */
export const hasDiagnosticStatusBar = (): boolean => item !== null;

export const disposeDiagnosticStatusBar = (): void => {
  if (changeSub) {
    changeSub.dispose();
    changeSub = null;
  }
  if (item) {
    item.dispose();
    item = null;
  }
};
