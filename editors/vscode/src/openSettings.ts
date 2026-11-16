import * as vscode from "vscode";

/**
 * Opens the Settings editor filtered to `query`.
 *
 * In Cursor, `workbench.action.openSettings` with a query can freeze the
 * Settings editor. The command returns the Settings editor pane, and Cursor
 * serializes that object graph to send it to the extension host, also when
 * the extension does not await the result (#4553, #4595).
 *
 * `runCommands` runs `workbench.action.openSettings` in the workbench and
 * returns no value, so the pane never goes to the extension host. The query
 * and the Settings editor stay the same in VS Code.
 *
 * A failure of the command call is not an error for the caller: the user can
 * still open Settings by hand.
 */
export const openSettingsSearch = async (query: string): Promise<void> => {
  try {
    await vscode.commands.executeCommand("runCommands", {
      commands: [{ command: "workbench.action.openSettings", args: query }],
    });
  } catch {
    // The Settings editor is a convenience. The setting key is in the message
    // that offered this action.
  }
};
