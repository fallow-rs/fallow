// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { isFallowDiagnostic } from "./diagnosticFilter.js";
import { FALLOW_LANGUAGES } from "./diagnosticMute.js";

const COMMAND_ID = "fallow.copyFindingId";
const CODE_ACTION_KIND = vscode.CodeActionKind.QuickFix.append("fallow.copyFindingId");
const STATUS_MESSAGE_MS = 4000;

/**
 * The dead-code or security finding id that the language server puts in
 * `Diagnostic.data.findingId`, or `null` when the diagnostic has none.
 *
 * The language client keeps the LSP `data` value on the diagnostic object,
 * but the VS Code type does not declare it, so the value is read defensively.
 */
export const findingIdOf = (diag: vscode.Diagnostic): string | null => {
  if (!isFallowDiagnostic(diag)) {
    return null;
  }
  const data: unknown = (diag as vscode.Diagnostic & { readonly data?: unknown }).data;
  if (typeof data !== "object" || data === null) {
    return null;
  }
  const id: unknown = (data as { readonly findingId?: unknown }).findingId;
  return typeof id === "string" && id.length > 0 ? id : null;
};

class FallowFindingIdCodeActions implements vscode.CodeActionProvider {
  public static readonly providedKinds: ReadonlyArray<vscode.CodeActionKind> = [CODE_ACTION_KIND];

  public provideCodeActions(
    _document: vscode.TextDocument,
    _range: vscode.Range | vscode.Selection,
    context: vscode.CodeActionContext,
  ): vscode.CodeAction[] {
    const seen = new Set<string>();
    const actions: vscode.CodeAction[] = [];
    for (const diag of context.diagnostics) {
      const id = findingIdOf(diag);
      if (id === null || seen.has(id)) {
        continue;
      }
      seen.add(id);
      const action = new vscode.CodeAction(`Copy Fallow finding id (${id})`, CODE_ACTION_KIND);
      action.command = {
        command: COMMAND_ID,
        title: "Copy Fallow finding id",
        arguments: [id],
      };
      action.diagnostics = [diag];
      actions.push(action);
    }
    return actions;
  }
}

const copyFindingId = async (id: unknown): Promise<void> => {
  if (typeof id !== "string" || id.length === 0) {
    return;
  }
  await vscode.env.clipboard.writeText(id);
  void vscode.window.setStatusBarMessage(`Fallow: copied finding id ${id}`, STATUS_MESSAGE_MS);
};

/** Register the "Copy finding id" command and its code action. */
export const registerFindingIdUi = (context: vscode.ExtensionContext): void => {
  context.subscriptions.push(vscode.commands.registerCommand(COMMAND_ID, copyFindingId));
  for (const language of FALLOW_LANGUAGES) {
    context.subscriptions.push(
      vscode.languages.registerCodeActionsProvider(
        { scheme: "file", language },
        new FallowFindingIdCodeActions(),
        { providedCodeActionKinds: FallowFindingIdCodeActions.providedKinds },
      ),
    );
  }
};

export const __findingIdTestHelpers = {
  FallowFindingIdCodeActions,
  copyFindingId,
};
