// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countCheckIssues } from "./analysis-utils.js";
import { startClient, stopClient, restartClient } from "./client.js";
import { onConfigChange } from "./config.js";
import { runAnalysis, runFix } from "./commands.js";
import { DiagnosticFilter } from "./diagnosticFilter.js";
import { registerDiagnosticMuteUi } from "./diagnosticMute.js";
import {
  createStatusBar,
  updateStatusBar,
  updateStatusBarFromLsp,
  setStatusBarAnalyzing,
  setStatusBarError,
  disposeStatusBar,
} from "./statusBar.js";
import type { AnalysisCompleteParams } from "./statusBar.js";
import { DeadCodeTreeProvider, DuplicatesTreeProvider } from "./treeView.js";
import type { FallowCheckResult, FallowDupesResult } from "./types.js";

let outputChannel: vscode.OutputChannel;
let lastCheckResult: FallowCheckResult | null = null;
let lastDupesResult: FallowDupesResult | null = null;

const RESTART_CONFIG_KEYS = [
  "fallow.lspPath",
  "fallow.configPath",
  "fallow.trace.server",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.autoDownload",
] as const;

const REANALYSIS_CONFIG_KEYS = [
  "fallow.configPath",
  "fallow.production",
  "fallow.duplication",
  "fallow.issueTypes",
  "fallow.changedSince",
] as const;

export interface ExtensionApi {
  readonly runAnalysis: typeof runAnalysis;
  readonly runFix: typeof runFix;
}

const affectsAnyConfiguration = (
  event: vscode.ConfigurationChangeEvent,
  keys: readonly string[],
): boolean => keys.some((key) => event.affectsConfiguration(key));

export const activate = async (context: vscode.ExtensionContext): Promise<ExtensionApi> => {
  outputChannel = vscode.window.createOutputChannel("Fallow");
  context.subscriptions.push(outputChannel);

  const statusBar = createStatusBar();
  context.subscriptions.push(statusBar);

  const diagnosticFilter = new DiagnosticFilter(context.workspaceState);
  context.subscriptions.push({ dispose: () => diagnosticFilter.dispose() });
  registerDiagnosticMuteUi(context, diagnosticFilter);

  const deadCodeProvider = new DeadCodeTreeProvider();
  const duplicatesProvider = new DuplicatesTreeProvider();

  // Use createTreeView to get visibility events. Defer CLI analysis until the
  // tree view is first shown, avoiding a double analysis on activation (the LSP
  // runs its own analysis for diagnostics).
  let cliAnalysisRan = false;

  const triggerCliAnalysis = async (): Promise<boolean> => {
    setStatusBarAnalyzing();
    return await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Fallow: Analyzing...",
        cancellable: false,
      },
      async () => {
        try {
          const { check, dupes } = await runAnalysis(context, outputChannel);
          lastCheckResult = check;
          lastDupesResult = dupes;
          updateViews();
          void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzed", true);

          const issueCount = countCheckIssues(check);

          if (issueCount > 0) {
            void vscode.window
              .showInformationMessage(
                `Fallow: found ${issueCount} issue${issueCount === 1 ? "" : "s"}. Open the Fallow sidebar to explore.`,
                "Open Sidebar",
              )
              .then((choice) => {
                if (choice === "Open Sidebar") {
                  void vscode.commands.executeCommand("fallow.deadCode.focus");
                }
                return undefined;
              });
          } else {
            void vscode.window.showInformationMessage("Fallow: no issues found.");
          }
          return true;
        } catch {
          setStatusBarError();
          return false;
        }
      },
    );
  };

  const deadCodeView = vscode.window.createTreeView("fallow.deadCode", {
    treeDataProvider: deadCodeProvider,
  });
  deadCodeProvider.setView(deadCodeView);
  const duplicatesView = vscode.window.createTreeView("fallow.duplicates", {
    treeDataProvider: duplicatesProvider,
  });
  context.subscriptions.push(deadCodeView, duplicatesView);

  const onViewVisible = (): void => {
    if (cliAnalysisRan) {
      return;
    }
    cliAnalysisRan = true;
    void (async (): Promise<void> => {
      const completed = await triggerCliAnalysis();
      if (!completed) {
        cliAnalysisRan = false;
      }
    })();
  };

  context.subscriptions.push(
    deadCodeView.onDidChangeVisibility((e) => {
      if (e.visible) {
        onViewVisible();
      }
    }),
  );
  context.subscriptions.push(
    duplicatesView.onDidChangeVisibility((e) => {
      if (e.visible) {
        onViewVisible();
      }
    }),
  );

  const updateViews = (): void => {
    deadCodeProvider.update(lastCheckResult);
    duplicatesProvider.update(lastDupesResult);
    updateStatusBar(lastCheckResult, lastDupesResult);
  };

  const runCliAnalysisCommand = async (): Promise<void> => {
    cliAnalysisRan = await triggerCliAnalysis();
  };

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.analyze", runCliAnalysisCommand),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.reloadAnalysis", runCliAnalysisCommand),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.fix", async () => {
      // Save dirty editors first so the fix works on up-to-date content
      await vscode.workspace.saveAll(false);
      await runFix(context, false);
      // Restart LSP to force fresh analysis. The fix modified files on disk
      // bypassing VS Code's editor, so did_save never fires for those files
      await restartClient(context, outputChannel, diagnosticFilter);
      // Re-run CLI analysis for tree views
      cliAnalysisRan = await triggerCliAnalysis();
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.fixDryRun", async () => {
      await runFix(context, true);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.restart", async () => {
      outputChannel.appendLine("Restarting language server...");
      await restartClient(context, outputChannel, diagnosticFilter);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.showOutput", () => {
      outputChannel.show();
    }),
  );

  // Open the Fallow sidebar (used by walkthrough completion event)
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.openSidebar", () => {
      void vscode.commands.executeCommand("fallow.deadCode.focus");
    }),
  );

  // Open Fallow settings (used by walkthrough completion event)
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.openSettings", () => {
      void vscode.commands.executeCommand("workbench.action.openSettings", "fallow");
    }),
  );

  // Fallback command for Code Lens items with 0 references (display-only)
  context.subscriptions.push(vscode.commands.registerCommand("fallow.noop", () => {}));

  // Watch for config changes
  context.subscriptions.push(
    onConfigChange(async (e) => {
      const needsRestart = affectsAnyConfiguration(e, RESTART_CONFIG_KEYS);
      const needsReanalysis = affectsAnyConfiguration(e, REANALYSIS_CONFIG_KEYS);

      if (needsRestart) {
        outputChannel.appendLine("Configuration changed, restarting server...");
        await restartClient(context, outputChannel, diagnosticFilter);
      }

      if (needsReanalysis) {
        // Re-run CLI analysis for tree views and status bar
        // (sequenced after LSP restart if both apply)
        void triggerCliAnalysis();
      }
    }),
  );

  // Start LSP client
  const client = await startClient(context, outputChannel, diagnosticFilter);
  if (client) {
    context.subscriptions.push({ dispose: () => void stopClient() });

    // Handle custom LSP notification: update status bar from LSP data
    // so the extension shows results immediately without waiting for CLI
    const notificationDisposable = client.onNotification(
      "fallow/analysisComplete",
      (params: AnalysisCompleteParams) => {
        updateStatusBarFromLsp(params);
        void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzed", true);
      },
    );
    context.subscriptions.push(notificationDisposable);
  }

  // Show walkthrough on first install
  const walkthroughShown = context.globalState.get<boolean>("fallow.walkthroughShown");
  if (!walkthroughShown) {
    void context.globalState.update("fallow.walkthroughShown", true);
    void vscode.commands.executeCommand(
      "workbench.action.openWalkthrough",
      "fallow-rs.fallow-vscode#fallow.gettingStarted",
      false,
    );
  }

  return {
    runAnalysis,
    runFix,
  };
};

export const deactivate = async (): Promise<void> => {
  disposeStatusBar();
  await stopClient();
};
