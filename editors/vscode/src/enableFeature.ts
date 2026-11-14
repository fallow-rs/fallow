import * as vscode from "vscode";

/** Fallow settings that a sidebar welcome link can turn on. */
export type FeatureToggleKey = "security.enabled" | "health.enabled";

const FEATURE_LABELS: Readonly<Record<FeatureToggleKey, string>> = {
  "security.enabled": "security candidate scanning",
  "health.enabled": "the Health view",
};

/**
 * Sets a Fallow feature toggle to `true` without the Settings editor.
 *
 * The sidebar welcome links use this command instead of
 * `workbench.action.openSettings` with a query. In Cursor, that command call
 * with a query can freeze the Settings editor: the reply to the extension host
 * is the Settings editor pane, and Cursor serializes that object graph (#4553).
 *
 * The value goes to workspace settings when a workspace is open, because a
 * workspace value overrides a user value. Without a workspace, it goes to
 * user settings. When the write fails (for example, a settings file with
 * invalid JSON), an error message names the setting to change by hand.
 */
export const enableFeature = async (key: FeatureToggleKey): Promise<void> => {
  const hasWorkspace = (vscode.workspace.workspaceFolders?.length ?? 0) > 0;
  const target = hasWorkspace
    ? vscode.ConfigurationTarget.Workspace
    : vscode.ConfigurationTarget.Global;
  const where = hasWorkspace ? "workspace" : "user";
  try {
    await vscode.workspace.getConfiguration("fallow").update(key, true, target);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(
      `Fallow: could not turn on ${FEATURE_LABELS[key]}. Set "fallow.${key}": true in your ${where} settings.json. ${reason}`,
    );
    return;
  }
  void vscode.window.showInformationMessage(
    `Fallow: turned on ${FEATURE_LABELS[key]} (fallow.${key} in ${where} settings).`,
  );
};
