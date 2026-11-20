// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  buildCleanAnalysisSummary,
  countCheckIssues,
  countDuplicationGroups,
} from "./analysis-utils.js";
import { runBaselineCommand } from "./baselineCommand.js";
import { shouldAcceptLspAnalysisComplete } from "./analysisNotification.js";
import { startClient, stopClient, restartClient } from "./client.js";
import { cancelReleaseRetry } from "./download.js";
import { enableFeature } from "./enableFeature.js";
import { openSettingsSearch } from "./openSettings.js";
import { createSingleFlight } from "./analysis-single-flight.js";
import { createLazyView, type LazyRunOutcome } from "./lazyView.js";
import {
  getHealthEnabled,
  getSecurityEnabled,
  getLicenseRefreshOnStartup,
  getCoveragePath,
  getAuditEnabled,
  getAuditRunOnSave,
  getDiagnosticSeverity,
  getMutedDiagnosticCategories,
  getComplexityBreakdownEnabled,
  getComplexityAfterText,
  onConfigChange,
} from "./config.js";
import { ComplexityDecorationController } from "./complexityDecorations.js";
import {
  ComplexityLensProvider,
  TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND,
  type ComplexityToggleTarget,
} from "./complexityLens.js";
import {
  runAnalysis,
  runAudit,
  runFix,
  healthInputKey,
  runHealthAnalysis,
  runInspectActiveFile,
  runInspectActiveSymbol,
  runSecurityAnalysis,
  runWorkspaces,
  securityInputKey,
} from "./commands.js";
import {
  DIAGNOSTIC_RENDER_CONFIG_KEYS,
  HEALTH_CONFIG_KEYS,
  REANALYSIS_CONFIG_KEYS,
  RESTART_CONFIG_KEYS,
  affectsAnyConfiguration,
} from "./configKeys.js";
import { countSecurityFindings } from "./security-utils.js";
import { SecurityTreeProvider } from "./securityTreeView.js";
import {
  activateLicenseCommand,
  deactivateLicenseCommand,
  initLicenseIndicator,
  licenseStatusCommand,
  refreshLicenseCommand,
  refreshLicenseStatus,
} from "./license.js";
import { DiagnosticFilter } from "./diagnosticFilter.js";
import { registerDiagnosticMuteUi } from "./diagnosticMute.js";
import { registerFindingIdUi } from "./findingId.js";
import { HealthTreeProvider, complexityTargetOf } from "./healthTreeView.js";
import {
  createStatusBar,
  updateStatusBar,
  updateStatusBarFromLsp,
  updateStatusBarHealth,
  setStatusBarAnalyzing,
  setStatusBarError,
  setStatusBarAudit,
  setStatusBarFindings,
  refreshStatusBar,
  showStatusBarMenu,
  disposeStatusBar,
} from "./statusBar.js";
import { STATUS_MENU_COMMAND } from "./statusBar-model.js";
import { auditScopeSummary, gatingCount } from "./audit-utils.js";
import { OPEN_FILE_COMMAND, openFileCommandHandler } from "./openFileCommand.js";
import type { AnalysisCompleteParams } from "./statusBar.js";
import { DeadCodeTreeProvider, DuplicatesTreeProvider } from "./treeView.js";
import {
  applyWorkspaceVisibility,
  clearWorkspaceScope,
  refreshWorkspacePicker,
  showWorkspacePicker,
} from "./workspacePicker.js";
import { RuntimeCoverageTreeProvider } from "./coverageView.js";
import { runCoverageAnalysis } from "./coverageCommand.js";
import { coverageWatermarkMessage } from "./coverage-utils.js";
import { configureBundledTypeAwareCompanion } from "./typeAwareCompanion.js";
import { killActiveChildren } from "./process-registry.js";
import type {
  AuditOutput,
  FallowAnalysisScope,
  FallowCheckResult,
  FallowDupesResult,
  HealthOutput,
  RuntimeCoverageReport,
  SecurityOutput,
} from "./types.js";

/** Languages whose saves trigger an opt-in audit re-run (`audit.runOnSave`). */
const AUDIT_SAVE_LANGUAGES: ReadonlySet<string> = new Set([
  "javascript",
  "javascriptreact",
  "typescript",
  "typescriptreact",
  "vue",
  "svelte",
  "astro",
  "mdx",
]);

/** Debounce window (ms) for coalescing rapid saves into a single audit run. */
const AUDIT_SAVE_DEBOUNCE_MS = 600;

/**
 * workspaceState flag: the "all findings are hidden" startup nudge was already
 * shown for this workspace. Set when the prompt appears, cleared whenever the
 * filter leaves the fully-muted state, so each distinct hide-all episode is
 * nudged exactly once instead of on every window reload.
 */
const MUTED_ALL_NUDGE_KEY = "fallow.mutedAllNudgeShown.v1";

let outputChannel: vscode.LogOutputChannel;
let lastCheckResult: FallowCheckResult | null = null;
let lastDupesResult: FallowDupesResult | null = null;
let lastAnalysisScope: FallowAnalysisScope | null = null;
let lastHealthResult: HealthOutput | null = null;
let lastCoverageReport: RuntimeCoverageReport | null = null;
let lastAuditResult: AuditOutput | null = null;

// The diagnostic filter is activate-scoped, but `deactivate()` needs to flush
// its pending workspaceState write before the window closes (its `dispose()` is
// synchronous and cannot await the persist queue). A module-level handle bridges
// that gap so the last mute toggle is not dropped mid-write on reload/shutdown.
let activeDiagnosticFilter: DiagnosticFilter | null = null;

/** Settings that the status bar item reads on each render. */
const STATUS_BAR_CONFIG_KEYS = [
  "fallow.audit.statusBar.enabled",
  "fallow.license.showStatusBar",
  "fallow.changedSince",
] as const;

export interface ExtensionApi {
  readonly runAnalysis: typeof runAnalysis;
  readonly runAudit: typeof runAudit;
  readonly runFix: typeof runFix;
  readonly runSecurityAnalysis: typeof runSecurityAnalysis;
}

export const activate = async (context: vscode.ExtensionContext): Promise<ExtensionApi> => {
  configureBundledTypeAwareCompanion(context.extensionPath);
  outputChannel = vscode.window.createOutputChannel("Fallow", { log: true });
  context.subscriptions.push(outputChannel);

  // One status bar item carries the analysis, health, audit, license,
  // workspace and findings-visibility state. Its click menu runs the existing
  // commands. License and workspace probes update it later, off the activation
  // hot path (#902).
  const statusBar = createStatusBar();
  context.subscriptions.push(statusBar);
  const licenseIndicatorEnabled = initLicenseIndicator();
  refreshWorkspacePicker(context);
  context.subscriptions.push(vscode.workspace.onDidChangeWorkspaceFolders(refreshStatusBar));

  const diagnosticFilter = new DiagnosticFilter(
    context.workspaceState,
    getDiagnosticSeverity,
    getMutedDiagnosticCategories(),
  );
  activeDiagnosticFilter = diagnosticFilter;
  context.subscriptions.push({
    dispose: () => {
      activeDiagnosticFilter = null;
      diagnosticFilter.dispose();
    },
  });
  registerDiagnosticMuteUi(context, diagnosticFilter);
  registerFindingIdUi(context);

  // Once-ever nudge: when EVERY Fallow finding is hidden (the "Hide All"
  // toggle), users who reinstalled often don't realize the mute state persisted
  // in workspaceState (it survives uninstall and deleting the `.fallow` folder),
  // so it looks like findings vanished for good. Surface a single dismissible
  // prompt wired to the escape-hatch command so a stuck-muted workspace is
  // recoverable without knowing the command name. Gated by a persisted flag so
  // an intentional muter is told once, not nagged on every window reload.
  if (diagnosticFilter.isMutedAll() && !context.workspaceState.get(MUTED_ALL_NUDGE_KEY)) {
    void (async () => {
      await context.workspaceState.update(MUTED_ALL_NUDGE_KEY, true);
      const choice = await vscode.window.showInformationMessage(
        "Fallow findings are hidden in this workspace (Hide All is on). CI and the CLI still report everything.",
        "Show all findings",
      );
      if (choice === "Show all findings") {
        await vscode.commands.executeCommand("fallow.resetDiagnosticFilters");
      }
    })();
  }
  // Re-arm the nudge once findings are no longer fully hidden, so a future
  // hide-all episode prompts again rather than staying silent forever.
  context.subscriptions.push(
    diagnosticFilter.onDidChange((state) => {
      if (!state.mutedAll && context.workspaceState.get(MUTED_ALL_NUDGE_KEY)) {
        void context.workspaceState.update(MUTED_ALL_NUDGE_KEY, undefined);
      }
    }),
  );

  // Custom LSP notification handler: update the status bar from LSP data so
  // results show immediately without waiting for the CLI pass. Passed into
  // startClient / restartClient so it re-registers on every client instance
  // (a restart builds a fresh client; a handler bound only to the first client
  // would stop firing and freeze the status bar after the first restart).
  const onAnalysisComplete = (params: AnalysisCompleteParams): void => {
    if (!shouldAcceptLspAnalysisComplete(vscode.workspace.textDocuments)) {
      return;
    }
    updateStatusBarFromLsp(params);
    void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzed", true);
  };

  const syncFindingsVisibility = (): void => {
    setStatusBarFindings({
      mutedAll: diagnosticFilter.isMutedAll(),
      mutedCategoryCount: diagnosticFilter.mutedCategoriesSnapshot().size,
    });
  };
  syncFindingsVisibility();
  context.subscriptions.push(diagnosticFilter.onDidChange(syncFindingsVisibility));

  const deadCodeProvider = new DeadCodeTreeProvider();
  const duplicatesProvider = new DuplicatesTreeProvider();
  const healthProvider = new HealthTreeProvider();

  // Expose the health-enabled state to `viewsWelcome` / `menus` `when` clauses.
  const syncHealthEnabledContext = (): void => {
    void vscode.commands.executeCommand("setContext", "fallow.health.enabled", getHealthEnabled());
  };
  syncHealthEnabledContext();

  // Expose the security-enabled state to `viewsWelcome` / `menus` `when` clauses
  // (mirrors `syncHealthEnabledContext`). Lets the welcome split between a
  // "scanning is off, enable it" state and an "enabled, run the scan" state, and
  // hides the scan toolbar button while the feature is disabled.
  const syncSecurityEnabledContext = (): void => {
    void vscode.commands.executeCommand(
      "setContext",
      "fallow.security.enabled",
      getSecurityEnabled(),
    );
  };
  syncSecurityEnabledContext();
  const securityProvider = new SecurityTreeProvider();
  const coverageProvider = new RuntimeCoverageTreeProvider();

  // Tie each TreeDataProvider's lifetime to the extension: createTreeView
  // disposes the TreeView wrapper but NOT the provider, so its EventEmitter
  // would otherwise leak on deactivate/reload. (The TreeViews themselves are
  // pushed where they are created below.)
  context.subscriptions.push(
    deadCodeProvider,
    duplicatesProvider,
    healthProvider,
    securityProvider,
    coverageProvider,
  );

  // Use createTreeView to get visibility events. Defer CLI analysis until the
  // tree view is first shown, avoiding a double analysis on activation (the LSP
  // runs its own analysis for diagnostics).
  let cliAnalysisRan = false;

  // One-shot, lazy workspace-visibility probe. The status bar item offers the
  // picker by default; after the first sidebar analysis we list workspaces
  // (cheap, cached) and drop the workspace entry on single-package repos that
  // can never use scoping (n2). Probed
  // off the activation hot path so #902 latency isolation is preserved, and
  // fire-and-forget so it never blocks the analysis it follows.
  let workspaceVisibilityProbed = false;
  const probeWorkspaceVisibility = (): void => {
    if (workspaceVisibilityProbed) {
      return;
    }
    workspaceVisibilityProbed = true;
    void (async (): Promise<void> => {
      const output = await runWorkspaces(context, false, outputChannel, true);
      applyWorkspaceVisibility(context, output);
    })();
  };

  interface CliAnalysisTriggerOptions {
    readonly force?: boolean;
  }

  const runCliAnalysisOnce = async (options: CliAnalysisTriggerOptions = {}): Promise<boolean> => {
    setStatusBarAnalyzing();
    return await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Fallow: Analyzing...",
        cancellable: false,
      },
      async () => {
        try {
          const { check, dupes, scope } = await runAnalysis(context, outputChannel, {
            force: options.force === true,
          });
          lastCheckResult = check;
          lastDupesResult = dupes;
          lastAnalysisScope = scope;
          updateViews();
          probeWorkspaceVisibility();
          void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzed", true);

          const issueCount = countCheckIssues(check);
          const duplicateGroupCount = countDuplicationGroups(dupes);

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
          } else if (duplicateGroupCount > 0) {
            void vscode.window
              .showInformationMessage(
                `Fallow: found ${duplicateGroupCount} duplicate-code group${duplicateGroupCount === 1 ? "" : "s"}. Open the Fallow sidebar to explore.`,
                "Open Sidebar",
              )
              .then((choice) => {
                if (choice === "Open Sidebar") {
                  void vscode.commands.executeCommand("fallow.duplicates.focus");
                }
                return undefined;
              });
          } else {
            const summary = buildCleanAnalysisSummary(check, dupes);
            outputChannel.appendLine(summary.outputLines.join("\n"));
            void vscode.window
              .showInformationMessage(summary.notification, "Open Output")
              .then((choice) => {
                if (choice === "Open Output") {
                  outputChannel.show();
                }
                return undefined;
              });
          }
          return true;
        } catch {
          setStatusBarError();
          return false;
        }
      },
    );
  };

  // Concurrency guard: the lazy view-visibility trigger, workspace scope
  // changes, config-change re-analysis, and explicit re-analyze / post-fix
  // runs can all fire while another run is in flight. Without a guard they
  // race on `lastCheckResult` / `lastDupesResult` (last-writer-wins). The
  // view-visibility trigger dedups onto the in-flight run; a `force` run
  // (explicit, scope change, or config change) arriving mid-run re-runs once
  // afterward so the results reflect the latest scope and config.
  const cliAnalysisFlight = createSingleFlight((force) => runCliAnalysisOnce({ force }));
  const triggerCliAnalysis = async (options: CliAnalysisTriggerOptions = {}): Promise<boolean> =>
    cliAnalysisFlight.run(options.force === true);

  // Inline complexity breakdown: per-line editor decorations driven by the same
  // health findings the tree renders. Reads the workspace root live so it tracks
  // the active folder; rendering and staleness live in the controller.
  const complexityDecorations = new ComplexityDecorationController(
    getComplexityBreakdownEnabled,
    getComplexityAfterText,
    () => vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
  );
  context.subscriptions.push(complexityDecorations);
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      complexityDecorations.renderEditor(editor);
    }),
    vscode.workspace.onDidChangeTextDocument((event) => {
      complexityDecorations.handleDocumentChange(event.document);
    }),
    vscode.workspace.onDidCloseTextDocument((document) => {
      complexityDecorations.handleDocumentClose(document);
    }),
  );

  // The complexity lens (summary + show/hide-breakdown toggle), the hover that
  // peeks the breakdown without expanding it, and the command the lens fires.
  // Scoped to the JS/TS family + SFCs (complexity findings never target JSON).
  const complexityLanguages: vscode.DocumentSelector = [
    { scheme: "file", language: "javascript" },
    { scheme: "file", language: "javascriptreact" },
    { scheme: "file", language: "typescript" },
    { scheme: "file", language: "typescriptreact" },
    { scheme: "file", language: "vue" },
    { scheme: "file", language: "svelte" },
    { scheme: "file", language: "astro" },
  ];
  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider(
      complexityLanguages,
      new ComplexityLensProvider(complexityDecorations),
    ),
    vscode.languages.registerHoverProvider(complexityLanguages, {
      provideHover: (document, position) => complexityDecorations.provideHover(document, position),
    }),
    vscode.commands.registerCommand(
      TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND,
      (target: ComplexityToggleTarget) => {
        complexityDecorations.toggleExpanded(target.path, target.line);
      },
    ),
  );

  const applyHealth = (report: HealthOutput | null): void => {
    lastHealthResult = report;
    healthProvider.update(report);
    updateStatusBarHealth(report);
    complexityDecorations.setFindings(report?.findings ?? []);
  };

  // Lazy, opt-out health spawn. Separate from the combined run so the
  // latency-critical sidebar is never coupled to complexity scoring or the
  // git-churn hotspot walk, and opening the editor never starts health work
  // (#902).
  //
  // A null report is a completed run (no workspace, empty output, older CLI),
  // so a later reveal does not spawn again or repeat the toast. A transient
  // failure makes `runHealthAnalysis` throw, and a later reveal tries again.
  const healthLoader = createLazyView<HealthOutput | null>({
    enabled: getHealthEnabled,
    inputKey: () => healthInputKey(context),
    // Health data also feeds the status bar and the inline complexity
    // decorations, so a changed input reloads while the view is hidden.
    reloadStaleWhen: ({ revealed }) => revealed,
    run: async () => ({
      kind: "loaded",
      value: await runHealthAnalysis(context, outputChannel),
    }),
    apply: applyHealth,
    reset: () => applyHealth(null),
  });

  const deadCodeView = vscode.window.createTreeView("fallow.deadCode", {
    treeDataProvider: deadCodeProvider,
  });
  deadCodeProvider.setView(deadCodeView);
  const duplicatesView = vscode.window.createTreeView("fallow.duplicates", {
    treeDataProvider: duplicatesProvider,
  });
  const healthView = vscode.window.createTreeView("fallow.health", {
    treeDataProvider: healthProvider,
  });
  const securityView = vscode.window.createTreeView("fallow.security", {
    treeDataProvider: securityProvider,
  });
  const coverageView = vscode.window.createTreeView("fallow.runtimeCoverage", {
    treeDataProvider: coverageProvider,
  });
  context.subscriptions.push(deadCodeView, duplicatesView, healthView, securityView, coverageView);

  // Run `fallow security` and update the Security Candidates view. Findings are
  // UNVERIFIED candidates (#903), so the toast says so and never uses
  // "vulnerability" or "confirmed". The scan starts only on a reveal of its own
  // view or on the scan command, so a user who never opens the view pays
  // nothing and the dead-code sidebar latency path stays untouched (#902).
  //
  // `fallow.hasAnalyzedSecurity` paints the "No security candidates found"
  // all-clear. Only a completed scan sets it. A failed scan or an older CLI
  // leaves the view and the welcome as they are: a false clean bill is the
  // worst failure of a security surface. A non-retryable failure counts as a
  // completed attempt, so a later reveal does not warn again.
  const securityLoader = createLazyView<SecurityOutput | null>({
    enabled: getSecurityEnabled,
    inputKey: () => securityInputKey(context),
    // The scan is opt-in and never runs for a hidden view (#902).
    reloadStaleWhen: () => securityView.visible,
    run: async () =>
      await vscode.window.withProgress<LazyRunOutcome<SecurityOutput | null>>(
        {
          location: vscode.ProgressLocation.Notification,
          title: "Fallow: Scanning for security candidates...",
          cancellable: false,
        },
        async () => {
          const result = await runSecurityAnalysis(context, outputChannel);
          if (result.ok) {
            return { kind: "loaded", value: result.data };
          }
          return result.retryable ? { kind: "failed" } : { kind: "skipped" };
        },
      ),
    apply: (data) => {
      securityProvider.update(data);
      void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzedSecurity", true);
      const count = countSecurityFindings(data);
      if (count > 0) {
        void vscode.window.showInformationMessage(
          `Fallow: found ${count} security candidate${count === 1 ? "" : "s"}. These are NOT verified vulnerabilities; verify each before acting.`,
        );
      }
    },
    reset: () => {
      securityProvider.update(null);
      void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzedSecurity", false);
    },
  });

  context.subscriptions.push(
    healthView.onDidChangeVisibility((e) => {
      if (e.visible) {
        void healthLoader.reveal();
      } else {
        // Hiding the Health view drops the transient selection highlight.
        complexityDecorations.setSelectedFunction(undefined);
      }
    }),
    // Selecting a complexity finding expands that function's inline breakdown
    // while it stays selected; any other selection (or none) clears it.
    healthView.onDidChangeSelection((e) => {
      const target = e.selection.length > 0 ? complexityTargetOf(e.selection[0]) : undefined;
      complexityDecorations.setSelectedFunction(target);
    }),
  );

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

  context.subscriptions.push(
    securityView.onDidChangeVisibility((e) => {
      if (e.visible) {
        void securityLoader.reveal();
      }
    }),
  );

  const updateViews = (): void => {
    deadCodeProvider.update(lastCheckResult);
    duplicatesProvider.update(lastDupesResult);
    updateStatusBar(lastCheckResult, lastDupesResult, lastAnalysisScope);
  };

  const runCliAnalysisCommand = async (): Promise<void> => {
    cliAnalysisRan = await triggerCliAnalysis({ force: true });
  };

  const runSetBaselineAtHeadCommand = async (): Promise<void> => {
    await runBaselineCommand({
      workspaceRoots: () =>
        vscode.workspace.workspaceFolders?.map((folder) => folder.uri.fsPath) ?? [],
      changedSince: () =>
        vscode.workspace.getConfiguration("fallow").get<string>("changedSince", ""),
      hasPackageBaselines: () => (lastAnalysisScope?.package_baselines?.length ?? 0) > 0,
      confirm: async (message) =>
        (await vscode.window.showWarningMessage(message, { modal: true }, "Set Baseline")) ===
        "Set Baseline",
      showWarning: async (message, action) => {
        if (!action) {
          await vscode.window.showWarningMessage(message);
          return false;
        }
        return (await vscode.window.showWarningMessage(message, action)) === action;
      },
      showError: async (message) => {
        await vscode.window.showErrorMessage(message);
      },
      showInformation: async (message) => {
        await vscode.window.showInformationMessage(message);
      },
      openChangedSinceSetting: async () => {
        await openSettingsSearch("fallow.changedSince");
      },
      updateChangedSince: async (value) => {
        await vscode.workspace
          .getConfiguration("fallow")
          .update("changedSince", value, vscode.ConfigurationTarget.Workspace);
      },
      refreshAnalysis: async () => {
        cliAnalysisRan = await triggerCliAnalysis({ force: true });
        if (!cliAnalysisRan) {
          throw new Error("analysis did not complete");
        }
      },
      appendOutput: (line) => outputChannel.appendLine(line),
    });
  };

  const runHealthAnalysisCommand = (): Promise<void> => healthLoader.reload();

  const runSecurityAnalysisCommand = async (): Promise<void> => {
    if (!getSecurityEnabled()) {
      void vscode.window.showInformationMessage(
        "Fallow: enable `fallow.security.enabled` to scan for security candidates.",
      );
      return;
    }
    await securityLoader.reload();
  };

  // Runtime coverage is loaded ONLY on explicit command (or lazily on first
  // view visibility when a capture path is already configured). It is fully
  // decoupled from the always-on dead-code/dupes pipeline and the LSP (#902):
  // never wired into triggerCliAnalysis, runAnalysis, the analysisComplete
  // notification, or REANALYSIS_CONFIG_KEYS. A user who never touches coverage
  // pays zero cost.
  const coverageLoader = createLazyView<RuntimeCoverageReport>({
    enabled: () => true,
    // A reveal loads only when the user already pointed at a capture. The
    // welcome view starts the first load in the other case.
    autoLoad: () => lastCoverageReport === null && getCoveragePath() !== "",
    // Constant: the run can store `fallow.coverage.capturePath` itself, so a
    // key that holds the path would change during the first load.
    inputKey: () => "",
    reloadStaleWhen: () => false,
    // Leaves the shared status bar (dead-code/dupes counts) untouched:
    // runtime coverage is an independent surface (#902). The progress
    // notification is enough feedback for the load itself.
    run: async () => {
      const report = await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: "Fallow: Loading runtime coverage...",
          cancellable: false,
        },
        () => runCoverageAnalysis(context, outputChannel),
      );
      // A null report means cancelled, no path or failure. The run showed its
      // own toast, and the view keeps its content.
      return report ? { kind: "loaded", value: report } : { kind: "skipped" };
    },
    apply: (report) => {
      lastCoverageReport = report;
      coverageProvider.update(report);
      void vscode.commands.executeCommand("setContext", "fallow.hasCoverage", true);

      // A grace/trial watermark means these candidates were produced under a
      // stale or expired license: surface it once per load so "Safe to
      // Delete" rows are not mistaken for authoritative deletions.
      const watermark = coverageWatermarkMessage(report);
      if (watermark) {
        void vscode.window.showWarningMessage(`Fallow runtime coverage: ${watermark}`);
      }
    },
    reset: () => {
      lastCoverageReport = null;
      coverageProvider.update(null);
      void vscode.commands.executeCommand("setContext", "fallow.hasCoverage", false);
    },
  });

  context.subscriptions.push(
    coverageView.onDidChangeVisibility((e) => {
      if (e.visible) {
        void coverageLoader.reveal();
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.analyze", runCliAnalysisCommand),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.reloadAnalysis", runCliAnalysisCommand),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.health.reload", runHealthAnalysisCommand),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.analyzeSecurity", runSecurityAnalysisCommand),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.inspectActiveFile", () =>
      runInspectActiveFile(context, outputChannel),
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.inspectActiveSymbol", () =>
      runInspectActiveSymbol(context, outputChannel),
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.setBaselineAtHead", runSetBaselineAtHeadCommand),
  );

  // Re-run the sidebar after a scope change so the tree views and status bar
  // reflect the newly selected workspace. Forced: a non-force trigger would
  // dedup onto an in-flight run spawned with the OLD scope and latch its
  // results as current with no follow-up run. Mark the analysis as run so a
  // later first-open of the sidebar does not trigger a redundant second pass.
  // The picker stores the scope in workspace state, so no configuration event
  // tells the lazy views about the change.
  const onWorkspaceScopeChange = (): void => {
    refreshWorkspacePicker(context);
    void healthLoader.invalidate();
    void securityLoader.invalidate();
    void (async (): Promise<void> => {
      cliAnalysisRan = await triggerCliAnalysis({ force: true });
    })();
  };

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.selectWorkspace", async () => {
      await showWorkspacePicker(
        context,
        (forceRefresh) => runWorkspaces(context, forceRefresh, outputChannel),
        onWorkspaceScopeChange,
      );
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.clearWorkspace", async () => {
      const changed = await clearWorkspaceScope(context);
      if (changed) {
        onWorkspaceScopeChange();
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.loadCoverage", () => coverageLoader.reload()),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.reloadCoverage", () => coverageLoader.reload()),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.clearCoverage", () => {
      coverageLoader.clear();
    }),
  );

  // On-demand audit verdict for the current change set. Uses a quiet
  // status-bar-area progress spinner (not a notification) so an audit is less
  // noisy than the full analysis. The audit run is single-flighted in
  // `runAudit`, so a click while one is in flight is a no-op.
  const reportAuditVerdict = (audit: AuditOutput): void => {
    lastAuditResult = audit;
    setStatusBarAudit({ kind: "result", audit });
    if (getAuditEnabled()) {
      return;
    }
    // Audit verdict in the status bar disabled: still report the verdict so the command is
    // never a silent no-op, and offer the details breakdown. Include the
    // change-set scope (changed files vs base ref) so the verdict is not a
    // contextless word (#908 n3).
    const count = gatingCount(audit);
    const suffix = count > 0 ? ` (${count} gating candidate${count === 1 ? "" : "s"})` : "";
    const scope = auditScopeSummary(audit);
    void vscode.window
      .showInformationMessage(`Fallow audit: ${audit.verdict}${suffix} - ${scope}`, "Details")
      .then((choice) => {
        if (choice === "Details") {
          outputChannel.show();
        }
        return undefined;
      });
  };

  const runAuditCommand = async (): Promise<void> => {
    setStatusBarAudit({ kind: "running" });
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Window,
        title: "Fallow: Auditing changes...",
        cancellable: false,
      },
      async () => {
        try {
          const audit = await runAudit(context, outputChannel);
          // A null result with no error means the run was skipped (already in
          // flight or no workspace). Prefer restoring the prior verdict; with no
          // prior verdict, reset to the idle "click to audit" state rather than
          // flashing a misleading error state (#908 n4). `runAudit` already
          // surfaces the no-workspace warning toast, so this stays silent.
          if (audit) {
            reportAuditVerdict(audit);
          } else if (lastAuditResult) {
            setStatusBarAudit({ kind: "result", audit: lastAuditResult });
          } else {
            setStatusBarAudit({ kind: "idle" });
          }
        } catch {
          setStatusBarAudit({ kind: "error" });
        }
      },
    );
  };
  context.subscriptions.push(vscode.commands.registerCommand("fallow.audit", runAuditCommand));

  // Opt-in re-run on save (default off; #902). Debounced and scoped to
  // JS/TS-family saves so it cannot regress idle latency on unrelated files.
  // Only active when the status-bar surface exists: a passive verdict refresh
  // makes sense there, whereas firing an information message on every save would
  // be noisy.
  let auditSaveTimer: ReturnType<typeof setTimeout> | null = null;
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc) => {
      // Read both settings live so toggling either takes effect without a window
      // reload: the surface must exist and run-on-save must be on.
      if (!getAuditEnabled() || !getAuditRunOnSave()) {
        return;
      }
      if (!AUDIT_SAVE_LANGUAGES.has(doc.languageId)) {
        return;
      }
      if (auditSaveTimer) {
        clearTimeout(auditSaveTimer);
      }
      auditSaveTimer = setTimeout(() => {
        auditSaveTimer = null;
        void runAuditCommand();
      }, AUDIT_SAVE_DEBOUNCE_MS);
    }),
  );
  context.subscriptions.push({
    dispose: () => {
      if (auditSaveTimer) {
        clearTimeout(auditSaveTimer);
        auditSaveTimer = null;
      }
    },
  });

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.fix", async () => {
      // Save dirty editors first so the fix works on up-to-date content
      await vscode.workspace.saveAll(false);
      await runFix(context, false);
      // Restart LSP to force fresh analysis. The fix modified files on disk
      // bypassing VS Code's editor, so did_save never fires for those files
      await restartClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
      // Re-run CLI analysis for tree views
      cliAnalysisRan = await triggerCliAnalysis({ force: true });
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
      await restartClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.resetDiagnosticFilters", async () => {
      // Escape hatch for a stuck-hidden workspace. Mute state lives in
      // workspaceState, so it survives uninstall/reinstall and deleting the
      // `.fallow` folder; the only in-editor way out is to clear it here. The
      // restart then re-opens every document, reproducing the close-and-reopen
      // workaround for all open files at once, which is the path proven to
      // re-render hidden findings (discussion #287).
      diagnosticFilter.clearAllMutes();
      diagnosticFilter.setMutedAll(false);
      outputChannel.appendLine(
        "Cleared Fallow diagnostic filters; restarting language server to re-render findings...",
      );
      await restartClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
      void vscode.window.setStatusBarMessage("Fallow: showing all findings", 4000);
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
    vscode.commands.registerCommand("fallow.openSettings", () => openSettingsSearch("fallow")),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand(STATUS_MENU_COMMAND, showStatusBarMenu),
  );

  // Turn on an opt-in view from its sidebar welcome link (#4553).
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.enableSecurity", () =>
      enableFeature("security.enabled"),
    ),
    vscode.commands.registerCommand("fallow.enableHealth", () => enableFeature("health.enabled")),
  );

  // License management commands (activate / status / refresh / deactivate).
  // All are one-shot CLI invocations; none touch the analysis path.
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.license.activate", () =>
      activateLicenseCommand(context, outputChannel),
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.license.status", () =>
      licenseStatusCommand(context, outputChannel),
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.license.refresh", () =>
      refreshLicenseCommand(context, outputChannel),
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.license.deactivate", () =>
      deactivateLicenseCommand(context, outputChannel),
    ),
  );

  // Fallback command for display-only Code Lens items that have no click target
  context.subscriptions.push(vscode.commands.registerCommand("fallow.noop", () => {}));

  context.subscriptions.push(
    vscode.commands.registerCommand(OPEN_FILE_COMMAND, openFileCommandHandler),
  );

  // The "imported by N files" Code Lens routes here instead of calling the built-in
  // `editor.action.showReferences` directly: that built-in validates its args
  // with `instanceof URI / Position / Location`, which the LSP's JSON wire
  // payload (a string URI, a plain position, plain locations) fails with
  // "argument does not match one of these constraints". Convert to real vscode
  // types, then delegate to the built-in.
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "fallow.showReferences",
      (
        uri: string,
        position: { line: number; character: number },
        locations: ReadonlyArray<{
          uri: string;
          range: {
            start: { line: number; character: number };
            end: { line: number; character: number };
          };
        }>,
      ) => {
        const toPosition = (p: { line: number; character: number }): vscode.Position =>
          new vscode.Position(p.line, p.character);
        const refs = (locations ?? []).map(
          (loc) =>
            new vscode.Location(
              vscode.Uri.parse(loc.uri),
              new vscode.Range(toPosition(loc.range.start), toPosition(loc.range.end)),
            ),
        );
        void vscode.commands.executeCommand(
          "editor.action.showReferences",
          vscode.Uri.parse(uri),
          toPosition(position),
          refs,
        );
      },
    ),
  );

  // Watch for config changes
  context.subscriptions.push(
    onConfigChange(async (e) => {
      const needsRestart = affectsAnyConfiguration(e, RESTART_CONFIG_KEYS);
      const needsReanalysis = affectsAnyConfiguration(e, REANALYSIS_CONFIG_KEYS);
      const needsHealthReanalysis = affectsAnyConfiguration(e, HEALTH_CONFIG_KEYS);
      const needsDiagnosticRefresh = affectsAnyConfiguration(e, DIAGNOSTIC_RENDER_CONFIG_KEYS);

      if (e.affectsConfiguration("fallow.workspace")) {
        // Keep the workspace state of the status bar item in sync with a
        // pinned-default setting change.
        // The workspaceState override (if any) still wins inside the picker. The
        // dead-code/dupes sidebar + status bar re-run is handled by the
        // REANALYSIS_CONFIG_KEYS path below (`fallow.workspace` is a member), so
        // a workspace change both refreshes the label and re-analyzes.
        refreshWorkspacePicker(context);
      }

      if (affectsAnyConfiguration(e, STATUS_BAR_CONFIG_KEYS)) {
        // The status bar item reads these settings on each render.
        initLicenseIndicator();
        refreshStatusBar();
      }

      if (e.affectsConfiguration("fallow.diagnostics.mutedCategories")) {
        diagnosticFilter.updateBaselineMutedCategories(getMutedDiagnosticCategories());
      }

      if (needsDiagnosticRefresh) {
        diagnosticFilter.refresh();
      }

      if (needsRestart) {
        outputChannel.appendLine("Configuration changed, restarting server...");
        await restartClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
      }

      if (needsReanalysis) {
        // Re-run CLI analysis for tree views and status bar (sequenced after
        // LSP restart if both apply). Forced: a run already in flight was
        // spawned with the old config, so deduping onto it would latch stale
        // results with no follow-up run.
        void triggerCliAnalysis({ force: true });
      }

      if (needsHealthReanalysis) {
        // Render-only work for the health settings. The loader below decides
        // from the input key of the run whether the health spawn starts again.
        syncHealthEnabledContext();
        if (e.affectsConfiguration("fallow.health.statusBar")) {
          updateStatusBarHealth(lastHealthResult);
        }
        complexityDecorations.refresh();
      }
      void healthLoader.invalidate();

      // `complexity.afterText` is render-only (the inline tier on/off): re-render
      // from the cached findings without respawning health.
      if (e.affectsConfiguration("fallow.complexity.afterText")) {
        complexityDecorations.renderVisibleEditors();
      }

      // `health.inlineComplexity` toggles the extension's complexity lens (no
      // longer an LSP option), so refresh decorations + lenses live, no respawn.
      if (e.affectsConfiguration("fallow.health.inlineComplexity")) {
        complexityDecorations.refresh();
      }

      // Keep the enabled-context (welcome split + scan-button gate) in sync when
      // the opt-in toggles.
      if (e.affectsConfiguration("fallow.security.enabled")) {
        syncSecurityEnabledContext();
      }
      // The loader scans again when the feature switch or an input of the scan
      // changed and the view is visible. A disabled feature empties the view.
      // This never runs the dead-code analysis.
      void securityLoader.invalidate();

      // Scoped to the coverage view only, deliberately NOT in
      // REANALYSIS_CONFIG_KEYS (#902): re-run coverage when the capture path
      // changes, but only if a capture was already loaded, so changing the
      // setting never kicks off work for a user who has not opted in.
      if (e.affectsConfiguration("fallow.coverage.capturePath") && lastCoverageReport) {
        void coverageLoader.reload();
      }
    }),
  );

  // Start LSP client. The analysis-complete handler is passed INTO startClient
  // so it is registered on every client instance (including post-restart ones),
  // not once on the initial client; see client.ts. Otherwise a config-change
  // restart would freeze the status bar.
  const client = await startClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
  if (client) {
    context.subscriptions.push({ dispose: () => void stopClient(outputChannel) });
  }

  // Opt-in license probe on startup (`fallow.license.refreshOnStartup`,
  // default false). Fire-and-forget so it never blocks activation or sidebar
  // reveal (#902); the indicator updates asynchronously when it resolves.
  if (licenseIndicatorEnabled && getLicenseRefreshOnStartup()) {
    void refreshLicenseStatus(context, outputChannel);
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
    runAudit,
    runFix,
    runSecurityAnalysis,
  };
};

export const deactivate = async (): Promise<void> => {
  cancelReleaseRetry();
  disposeStatusBar();
  // Kill any in-flight CLI children (analysis, audit, health, security, fix,
  // license) before stopping the LSP, so a window reload mid-analysis does not
  // orphan a process that can hold file handles on the project directory.
  killActiveChildren();
  // Flush any in-flight mute-state write before tearing down, so the last toggle
  // survives a window reload that closes mid-persist.
  await activeDiagnosticFilter?.flushPersist();
  await stopClient();
};
