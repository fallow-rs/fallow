// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

/**
 * Whether a binary-version-skew toast has already been shown this session. Both
 * the LSP path (`client.ts`, fallow-lsp older than the extension) and the CLI
 * analysis path (`commands.ts`, fallow CLI rejecting/omitting a flag) detect the
 * same root cause: a resolved binary older than the extension. Stacking two
 * toasts about one cause trains users to dismiss without reading, so the toast
 * is shown at most once per session here; per-event details still go to the
 * output channel on every occurrence. Reset implicitly on reactivation.
 */
let toastShown = false;

/**
 * Show at most one binary-version-skew toast per session, regardless of which
 * code path (LSP or CLI) detects the skew first.
 */
export const showBinarySkewToastOnce = (message: string): void => {
  if (toastShown) {
    return;
  }
  toastShown = true;
  void vscode.window.showWarningMessage(message);
};

/** Test-only: reset the once-per-session guard. */
export const resetBinarySkewToast = (): void => {
  toastShown = false;
};
