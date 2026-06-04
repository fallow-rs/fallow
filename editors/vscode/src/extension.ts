// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countCheckIssues } from "./analysis-utils.js";
import { startClient, stopClient, restartClient } from "./client.js";
import {
  getHealthEnabled,
  getSecurityEnabled,
  getLicenseRefreshOnStartup,
  getCoveragePath,
  getAuditEnabled,
  getAuditRunOnSave,
  onConfigChange,
} from "./config.js";
import {
  runAnalysis,
  runAudit,
  runFix,
  runHealthAnalysis,
  runSecurityAnalysis,
  runWorkspaces,
} from "./commands.js";
import {
  HEALTH_CONFIG_KEYS,
  REANALYSIS_CONFIG_KEYS,
  RESTART_CONFIG_KEYS,
  affectsAnyConfiguration,
} from "./configKeys.js";
import { countSecurityFindings } from "./security-utils.js";
import { SecurityTreeProvider } from "./securityTreeView.js";
import {
  activateLicenseCommand,
  createLicenseStatusBar,
  deactivateLicenseCommand,
  disposeLicenseStatusBar,
  licenseStatusCommand,
  refreshLicenseCommand,
  refreshLicenseStatus,
} from "./license.js";
import { DiagnosticFilter } from "./diagnosticFilter.js";
import { registerDiagnosticMuteUi } from "./diagnosticMute.js";
import { HealthTreeProvider } from "./healthTreeView.js";
import {
  createStatusBar,
  updateStatusBar,
  updateStatusBarFromLsp,
  updateStatusBarHealth,
  setStatusBarAnalyzing,
  setStatusBarError,
  disposeStatusBar,
} from "./statusBar.js";
import { gatingCount } from "./audit-utils.js";
import {
  createAuditStatusBar,
  disposeAuditStatusBar,
  setAuditAnalyzing,
  setAuditError,
  updateAuditStatusBar,
} from "./auditStatusBar.js";
import type { AnalysisCompleteParams } from "./statusBar.js";
import { DeadCodeTreeProvider, DuplicatesTreeProvider } from "./treeView.js";
import {
  clearWorkspaceScope,
  createWorkspacePicker,
  disposeWorkspacePicker,
  refreshWorkspacePicker,
  showWorkspacePicker,
} from "./workspacePicker.js";
import { RuntimeCoverageTreeProvider } from "./coverageView.js";
import { runCoverageAnalysis } from "./coverageCommand.js";
import type {
  AuditOutput,
  FallowCheckResult,
  FallowDupesResult,
  HealthReport,
  RuntimeCoverageReport,
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

let outputChannel: vscode.OutputChannel;
let lastCheckResult: FallowCheckResult | null = null;
let lastDupesResult: FallowDupesResult | null = null;
let lastHealthResult: HealthReport | null = null;
let lastCoverageReport: RuntimeCoverageReport | null = null;
let lastAuditResult: AuditOutput | null = null;

// The security run is a separate, view-gated process with disjoint config keys
// from the dead-code analysis: toggling security never re-runs the main
// analysis, and dead-code config changes never trigger a security re-run (#902).
const SECURITY_CONFIG_KEYS = [
  "fallow.security.enabled",
  "fallow.configPath",
  "fallow.changedSince",
  "fallow.workspace",
] as const;

export interface ExtensionApi {
  readonly runAnalysis: typeof runAnalysis;
  readonly runAudit: typeof runAudit;
  readonly runFix: typeof runFix;
  readonly runSecurityAnalysis: typeof runSecurityAnalysis;
}

export const activate = async (context: vscode.ExtensionContext): Promise<ExtensionApi> => {
  outputChannel = vscode.window.createOutputChannel("Fallow");
  context.subscriptions.push(outputChannel);

  const statusBar = createStatusBar();
  context.subscriptions.push(statusBar);

  // License indicator: a second status-bar item, created only when enabled
  // (`fallow.license.showStatusBar`). Decoupled from the analysis path, so it
  // adds no latency to sidebar reveal or `runAnalysis` (#902).
  const licenseStatusBar = createLicenseStatusBar();
  if (licenseStatusBar) {
    context.subscriptions.push(licenseStatusBar);
  }
  context.subscriptions.push({ dispose: () => disposeLicenseStatusBar() });

  const workspacePicker = createWorkspacePicker(context);
  context.subscriptions.push(workspacePicker);
  context.subscriptions.push({ dispose: () => disposeWorkspacePicker() });

  // Audit verdict status bar. Gated on the setting (default on); creating an
  // idle item runs no analysis, so it never touches the startup/visibility hot
  // path (#902). When disabled, the command still runs and reports its verdict
  // via an information message instead of the status bar.
  const auditStatusBarEnabled = getAuditEnabled();
  if (auditStatusBarEnabled) {
    const auditStatusBar = createAuditStatusBar();
    context.subscriptions.push(auditStatusBar);
  }

  const diagnosticFilter = new DiagnosticFilter(context.workspaceState);
  context.subscriptions.push({ dispose: () => diagnosticFilter.dispose() });
  registerDiagnosticMuteUi(context, diagnosticFilter);

  const deadCodeProvider = new DeadCodeTreeProvider();
  const duplicatesProvider = new DuplicatesTreeProvider();
  const healthProvider = new HealthTreeProvider();

  // Expose the health-enabled state to `viewsWelcome` / `menus` `when` clauses.
  const syncHealthEnabledContext = (): void => {
    void vscode.commands.executeCommand(
      "setContext",
      "fallow.health.enabled",
      getHealthEnabled(),
    );
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

  // Use createTreeView to get visibility events. Defer CLI analysis until the
  // tree view is first shown, avoiding a double analysis on activation (the LSP
  // runs its own analysis for diagnostics).
  let cliAnalysisRan = false;
  // The health spawn has its own latch and visibility trigger, fully
  // independent of the combined run, so opening the editor or the existing two
  // views never triggers any health work (#902 latency isolation).
  let healthAnalysisRan = false;
  // The security run is decoupled from the dead-code run: it fires only on first
  // visibility of the Security Candidates view, behind its own flag, so a user
  // who never opens that view pays nothing even with the feature enabled (#902).
  let securityAnalysisRan = false;

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

  // Lazy, opt-out health spawn. Separate from the combined run so the
  // latency-critical sidebar is never coupled to complexity scoring or the
  // git-churn hotspot walk.
  //
  // Returns whether the run COMPLETED, not whether it produced data. A null
  // report from a non-retryable outcome (no workspace, empty output, older CLI)
  // still counts as completed, so the latch holds and a re-reveal does not
  // re-spawn or repeat the no-workspace / update-CLI toast (#902). Only a
  // genuine transient failure (rethrown by runHealthAnalysis) returns false so
  // the caller resets the latch and a later reveal retries. Mirrors Security's
  // unconditional-true completion contract.
  const triggerHealthAnalysis = async (): Promise<boolean> => {
    if (!getHealthEnabled()) {
      lastHealthResult = null;
      healthProvider.update(null);
      updateStatusBarHealth(null);
      return true;
    }
    try {
      const report = await runHealthAnalysis(context, outputChannel);
      lastHealthResult = report;
      healthProvider.update(report);
      updateStatusBarHealth(report);
      return true;
    } catch {
      return false;
    }
  };

  // Run `fallow security` and update the Security Candidates view. Findings are
  // UNVERIFIED candidates (#903), so the toast says so explicitly and never uses
  // "vulnerability"/"confirmed".
  //
  // `fallow.hasAnalyzedSecurity` (which paints the "No security candidates
  // found" all-clear) is set ONLY after a genuinely completed scan, never after
  // a failed or older-CLI run: a false clean bill on a security surface is the
  // worst failure mode here, so the actionable enable/scan welcome stays in
  // place instead. Returns whether the run COMPLETED (a non-retryable failure
  // still counts as completed so the latch holds and a re-reveal does not
  // re-warn); only a transient failure returns false so the caller resets the
  // latch and a later reveal retries.
  const triggerSecurityAnalysis = async (): Promise<boolean> => {
    return await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Fallow: Scanning for security candidates...",
        cancellable: false,
      },
      async () => {
        const result = await runSecurityAnalysis(context, outputChannel);
        if (!result.ok) {
          // Leave the existing view/welcome untouched on failure: do NOT flip
          // `fallow.hasAnalyzedSecurity`, so a failed/unsupported scan never
          // paints a false "No security candidates found" all-clear.
          return !result.retryable;
        }

        securityProvider.update(result.data);
        void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzedSecurity", true);

        const count = countSecurityFindings(result.data);
        if (count > 0) {
          void vscode.window.showInformationMessage(
            `Fallow: found ${count} security candidate${count === 1 ? "" : "s"}. These are NOT verified vulnerabilities; verify each before acting.`,
          );
        }
        return true;
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
  const healthView = vscode.window.createTreeView("fallow.health", {
    treeDataProvider: healthProvider,
  });
  healthProvider.setView(healthView);
  const securityView = vscode.window.createTreeView("fallow.security", {
    treeDataProvider: securityProvider,
  });
  securityProvider.setView(securityView);
  const coverageView = vscode.window.createTreeView("fallow.runtimeCoverage", {
    treeDataProvider: coverageProvider,
  });
  coverageProvider.setView(coverageView);
  context.subscriptions.push(
    deadCodeView,
    duplicatesView,
    healthView,
    securityView,
    coverageView,
  );

  const onHealthViewVisible = (): void => {
    if (healthAnalysisRan) {
      return;
    }
    healthAnalysisRan = true;
    void (async (): Promise<void> => {
      const completed = await triggerHealthAnalysis();
      if (!completed) {
        healthAnalysisRan = false;
      }
    })();
  };

  context.subscriptions.push(
    healthView.onDidChangeVisibility((e) => {
      if (e.visible) {
        onHealthViewVisible();
      }
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

  // Lazily run the security scan on first visibility of its own view, behind a
  // separate flag and gated on the opt-in setting. This is the #902 protection:
  // the run never touches the dead-code / duplicates sidebar latency path.
  const onSecurityViewVisible = (): void => {
    if (securityAnalysisRan || !getSecurityEnabled()) {
      return;
    }
    securityAnalysisRan = true;
    void (async (): Promise<void> => {
      const completed = await triggerSecurityAnalysis();
      if (!completed) {
        securityAnalysisRan = false;
      }
    })();
  };

  context.subscriptions.push(
    securityView.onDidChangeVisibility((e) => {
      if (e.visible) {
        onSecurityViewVisible();
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

  const runHealthAnalysisCommand = async (): Promise<void> => {
    healthAnalysisRan = await triggerHealthAnalysis();
  };

  const runSecurityAnalysisCommand = async (): Promise<void> => {
    if (!getSecurityEnabled()) {
      void vscode.window.showInformationMessage(
        "Fallow: enable `fallow.security.enabled` to scan for security candidates.",
      );
      return;
    }
    securityAnalysisRan = await triggerSecurityAnalysis();
  };

  // Runtime coverage is loaded ONLY on explicit command (or lazily on first
  // view visibility when a capture path is already configured). It is fully
  // decoupled from the always-on dead-code/dupes pipeline and the LSP (#902):
  // never wired into triggerCliAnalysis, runAnalysis, the analysisComplete
  // notification, or REANALYSIS_CONFIG_KEYS. A user who never touches coverage
  // pays zero cost.
  const loadCoverage = async (): Promise<void> => {
    // Intentionally leaves the shared status bar (dead-code/dupes counts)
    // untouched: runtime coverage is an independent surface (#902). The
    // progress notification is enough feedback for the load itself.
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Fallow: Loading runtime coverage...",
        cancellable: false,
      },
      async () => {
        const report = await runCoverageAnalysis(context, outputChannel);
        // A null report means cancelled / no path / failure; only surfacing a
        // loaded report flips the view out of its welcome state. A failed run
        // already showed its own toast inside runCoverageAnalysis.
        if (report) {
          lastCoverageReport = report;
          coverageProvider.update(report);
          void vscode.commands.executeCommand("setContext", "fallow.hasCoverage", true);
        }
      },
    );
  };

  let coverageLoadAttempted = false;
  const onCoverageViewVisible = (): void => {
    if (coverageLoadAttempted || lastCoverageReport) {
      return;
    }
    // Lazy auto-load only when the user already pointed at a capture; otherwise
    // the welcome view's call-to-action drives the first load.
    if (!getCoveragePath()) {
      return;
    }
    coverageLoadAttempted = true;
    void loadCoverage();
  };

  context.subscriptions.push(
    coverageView.onDidChangeVisibility((e) => {
      if (e.visible) {
        onCoverageViewVisible();
      }
    }),
  );

  // Register commands
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

  // Re-run the sidebar after a scope change so the tree views and status bar
  // reflect the newly selected workspace. The picker persists the choice to
  // workspaceState; resolveActiveWorkspaceScope picks it up on the next run.
  // Mark the analysis as run so a later first-open of the sidebar does not
  // trigger a redundant second pass.
  const onWorkspaceScopeChange = (): void => {
    refreshWorkspacePicker(context);
    void (async (): Promise<void> => {
      cliAnalysisRan = await triggerCliAnalysis();
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
    vscode.commands.registerCommand("fallow.loadCoverage", async () => {
      coverageLoadAttempted = true;
      await loadCoverage();
    }),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.reloadCoverage", async () => {
      coverageLoadAttempted = true;
      await loadCoverage();
    }),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.clearCoverage", () => {
      lastCoverageReport = null;
      coverageLoadAttempted = false;
      coverageProvider.update(null);
      void vscode.commands.executeCommand("setContext", "fallow.hasCoverage", false);
    }),
  );

  // On-demand audit verdict for the current change set. Uses a quiet
  // status-bar-area progress spinner (not a notification) so an audit is less
  // noisy than the full analysis. The audit run is single-flighted in
  // `runAudit`, so a click while one is in flight is a no-op.
  const reportAuditVerdict = (audit: AuditOutput): void => {
    lastAuditResult = audit;
    if (auditStatusBarEnabled) {
      updateAuditStatusBar(audit);
      return;
    }
    // Status bar surface disabled: still report the verdict so the command is
    // never a silent no-op, and offer the details breakdown.
    const count = gatingCount(audit);
    const suffix = count > 0 ? ` (${count} gating candidate${count === 1 ? "" : "s"})` : "";
    void vscode.window
      .showInformationMessage(`Fallow audit: ${audit.verdict}${suffix}`, "Details")
      .then((choice) => {
        if (choice === "Details") {
          outputChannel.show();
        }
        return undefined;
      });
  };

  const runAuditCommand = async (): Promise<void> => {
    setAuditAnalyzing();
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
          // flight or no workspace); leave the prior verdict in place rather
          // than flashing an error state.
          if (audit) {
            reportAuditVerdict(audit);
          } else if (lastAuditResult) {
            updateAuditStatusBar(lastAuditResult);
          } else {
            setAuditError();
          }
        } catch {
          setAuditError();
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
      if (!auditStatusBarEnabled || !getAuditRunOnSave()) {
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

  // Fallback command for Code Lens items with 0 references (display-only)
  context.subscriptions.push(vscode.commands.registerCommand("fallow.noop", () => {}));

  // Watch for config changes
  context.subscriptions.push(
    onConfigChange(async (e) => {
      const needsRestart = affectsAnyConfiguration(e, RESTART_CONFIG_KEYS);
      const needsReanalysis = affectsAnyConfiguration(e, REANALYSIS_CONFIG_KEYS);
      const needsHealthReanalysis = affectsAnyConfiguration(e, HEALTH_CONFIG_KEYS);
      const affectsSecurity = affectsAnyConfiguration(e, SECURITY_CONFIG_KEYS);

      if (e.affectsConfiguration("fallow.workspace")) {
        // Keep the picker label in sync with a pinned-default setting change.
        // The workspaceState override (if any) still wins inside the picker.
        refreshWorkspacePicker(context);
      }

      if (needsRestart) {
        outputChannel.appendLine("Configuration changed, restarting server...");
        await restartClient(context, outputChannel, diagnosticFilter);
      }

      if (needsReanalysis) {
        // Re-run CLI analysis for tree views and status bar
        // (sequenced after LSP restart if both apply)
        void triggerCliAnalysis();
      }

      if (needsHealthReanalysis) {
        // Health settings never restart the LSP nor re-run the combined
        // analysis. Toggling only the status-bar visibility re-renders from the
        // cached report (no respawn); the spawn-affecting settings (enabled,
        // hotspots, topFindings) re-run the standalone health spawn, but only
        // if the user already revealed the Health view (preserving the lazy
        // trigger when they have not).
        syncHealthEnabledContext();
        const onlyStatusBarChanged =
          e.affectsConfiguration("fallow.health.statusBar") &&
          !e.affectsConfiguration("fallow.health.enabled") &&
          !e.affectsConfiguration("fallow.health.hotspots") &&
          !e.affectsConfiguration("fallow.health.topFindings");
        if (onlyStatusBarChanged) {
          updateStatusBarHealth(lastHealthResult);
        } else if (healthAnalysisRan) {
          void triggerHealthAnalysis();
        }
      }

      if (affectsSecurity) {
        // Keep the enabled-context (welcome split + scan-button gate) in sync
        // when the opt-in toggles.
        if (e.affectsConfiguration("fallow.security.enabled")) {
          syncSecurityEnabledContext();
        }
        // Security keys are disjoint from REANALYSIS_CONFIG_KEYS, so this never
        // re-runs the dead-code analysis. When the feature is enabled and the
        // view is open, re-scan; otherwise clear the provider so a disabled view
        // shows nothing stale.
        if (getSecurityEnabled()) {
          if (securityView.visible) {
            securityAnalysisRan = await triggerSecurityAnalysis();
          }
        } else {
          securityAnalysisRan = false;
          securityProvider.update(null);
          void vscode.commands.executeCommand("setContext", "fallow.hasAnalyzedSecurity", false);
        }
      }

      // Scoped to the coverage view only, deliberately NOT in
      // REANALYSIS_CONFIG_KEYS (#902): re-run coverage when the capture path
      // changes, but only if a capture was already loaded, so changing the
      // setting never kicks off work for a user who has not opted in.
      if (e.affectsConfiguration("fallow.coverage.capturePath") && lastCoverageReport) {
        void loadCoverage();
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

  // Opt-in license probe on startup (`fallow.license.refreshOnStartup`,
  // default false). Fire-and-forget so it never blocks activation or sidebar
  // reveal (#902); the indicator updates asynchronously when it resolves.
  if (licenseStatusBar && getLicenseRefreshOnStartup()) {
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
  disposeStatusBar();
  disposeLicenseStatusBar();
  disposeWorkspacePicker();
  disposeAuditStatusBar();
  await stopClient();
};
