import * as fs from "node:fs";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  type DiagnosticProviderShape,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
  TransportKind,
} from "vscode-languageclient/node.js";
import { DocumentDiagnosticRequest, Trace } from "vscode-languageserver-protocol";
import {
  getLspPath,
  getTraceLevel,
  getAutoDownload,
  getIssueTypes,
  getChangedSince,
  getResolvedConfigPath,
  getProductionOverride,
  getDuplicationCrossLanguageOverride,
  getDuplicationIgnoreImportsOverride,
  getDuplicationMinLinesOverride,
  getDuplicationMinOccurrencesOverride,
  getDuplicationMinTokensOverride,
  getDuplicationModeOverride,
  getDuplicationSkipLocalOverride,
  getDuplicationThresholdOverride,
  getMutedDiagnosticCategories,
} from "./config.js";
import { showBinarySkewToastOnce } from "./binary-skew.js";
import { findBinaryInPath, findLocalBinary } from "./binary-utils.js";
import type { DiagnosticFilter } from "./diagnosticFilter.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";
import type { DuplicationMode, IssueTypeConfig } from "./types.js";
import {
  parseDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "./diagnosticFilter.js";
import { downloadBinary, getBinaryVersion, getInstalledBinaryPath } from "./download.js";

let client: LanguageClient | null = null;

// Serializes restarts. Two config changes firing in quick succession would
// otherwise each pass `stopClient`'s `if (!current)` guard and each spawn a
// `startClient`, racing two server processes (and double-stopping one client).
// Chaining every restart onto this queue makes them strictly sequential.
let restartQueue: Promise<LanguageClient | null> = Promise.resolve(null);

export interface LspInitializationOptions {
  readonly issueTypes: IssueTypeConfig;
  readonly changedSince: string;
  readonly configPath: string;
  /**
   * Production-mode override forwarded so the LSP diagnostics match the
   * CLI-driven sidebar. `true`/`false` force production on/off; `undefined`
   * (the `"auto"` setting) is dropped by `JSON.stringify`, so the LSP sees no
   * `production` key and defers to the project config (issue #1055).
   */
  readonly production: boolean | undefined;
  readonly duplication: {
    readonly mode: DuplicationMode | undefined;
    readonly threshold: number | undefined;
    readonly minTokens: number | undefined;
    readonly minLines: number | undefined;
    readonly minOccurrences: number | undefined;
    readonly skipLocal: boolean | undefined;
    readonly crossLanguage: boolean | undefined;
    readonly ignoreImports: boolean | undefined;
  };
}

export const createInitializationOptions = (): LspInitializationOptions => ({
  issueTypes: getIssueTypes(),
  changedSince: getChangedSince(),
  configPath: getResolvedConfigPath(),
  production: getProductionOverride(),
  // `fallow.health.inlineComplexity` is rendered by the extension's own
  // ComplexityLensProvider (so the lens can toggle the per-line breakdown), so
  // it is NOT forwarded to the LSP. The LSP complexity lens stays opt-in for
  // other editors (Neovim/Zed/Helix) via their own initializationOptions; this
  // avoids a double lens in VS Code without removing the editor-agnostic path.
  duplication: {
    mode: getDuplicationModeOverride(),
    threshold: getDuplicationThresholdOverride(),
    minTokens: getDuplicationMinTokensOverride(),
    minLines: getDuplicationMinLinesOverride(),
    minOccurrences: getDuplicationMinOccurrencesOverride(),
    skipLocal: getDuplicationSkipLocalOverride(),
    crossLanguage: getDuplicationCrossLanguageOverride(),
    ignoreImports: getDuplicationIgnoreImportsOverride(),
  },
});

const warnIfVersionMismatch = async (
  binaryPath: string,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  const extensionVersion = vscode.extensions.getExtension("fallow-rs.fallow-vscode")?.packageJSON
    ?.version as string | undefined;
  if (!extensionVersion) return;

  const binaryVersion = await getBinaryVersion(binaryPath);
  if (binaryVersion && binaryVersion !== extensionVersion) {
    const msg = `Fallow: binary in PATH is v${binaryVersion}, extension is v${extensionVersion}. Update the binary or remove it from PATH to use the managed auto-download.`;
    outputChannel?.appendLine(msg);
    // Shared once-per-session guard so the LSP-skew and CLI-skew toasts (same
    // root cause) don't stack into two dismissible warnings.
    showBinarySkewToastOnce(msg);
  }
};

const resolveBinaryPath = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<string | null> => {
  const configPath = getLspPath();
  if (configPath) {
    if (fs.existsSync(configPath)) {
      outputChannel?.appendLine(`Binary resolution: using fallow.lspPath setting: ${configPath}`);
      return configPath;
    }
    void vscode.window.showWarningMessage(
      `Fallow: configured LSP path "${configPath}" does not exist.`,
    );
    return null;
  }

  const local = findLocalBinary("fallow-lsp");
  if (local) {
    outputChannel?.appendLine(`Binary resolution: using local node_modules/.bin: ${local}`);
    return local;
  }
  outputChannel?.appendLine("Binary resolution: no local node_modules/.bin/fallow-lsp found");

  const inPath = findBinaryInPath("fallow-lsp");
  if (inPath) {
    outputChannel?.appendLine(`Binary resolution: using system PATH: ${inPath}`);
    // Fire-and-forget: the skew toast must not block binary resolution on the
    // up-to-5s `--version` spawn (the reason getBinaryVersion is async).
    void warnIfVersionMismatch(inPath, outputChannel);
    return inPath;
  }
  outputChannel?.appendLine("Binary resolution: fallow-lsp not found in PATH");

  const installed = await getInstalledBinaryPath(context, outputChannel);
  if (installed) {
    outputChannel?.appendLine(
      `Binary resolution: using previously downloaded binary: ${installed}`,
    );
    return installed;
  }

  if (getAutoDownload()) {
    return downloadBinary(context);
  }

  const choice = await vscode.window.showErrorMessage(
    "Fallow: fallow-lsp binary not found. Would you like to download it?",
    "Download",
    "Set Path",
    "Cancel",
  );

  if (choice === "Download") {
    return downloadBinary(context);
  }

  if (choice === "Set Path") {
    void vscode.commands.executeCommand("workbench.action.openSettings", "fallow.lspPath");
  }

  return null;
};

export const loadDiagnosticCategories = async (
  lspClient: LanguageClient,
  outputChannel: vscode.OutputChannel,
): Promise<void> => {
  try {
    const response = await lspClient.sendRequest<unknown>("fallow/issueTypes");
    const categories = parseDiagnosticCategories(response);
    if (!categories) {
      resetDiagnosticCategories();
      outputChannel.appendLine(
        "fallow/issueTypes returned an invalid response; using bundled diagnostic categories.",
      );
      return;
    }
    setDiagnosticCategories(categories);
    outputChannel.appendLine(`Loaded ${categories.length} diagnostic categories from fallow-lsp.`);
  } catch (err) {
    resetDiagnosticCategories();
    const message = err instanceof Error ? err.message : String(err);
    outputChannel.appendLine(
      `fallow/issueTypes unavailable (${message}); using bundled diagnostic categories.`,
    );
  }
};

/**
 * Force VS Code to re-pull `textDocument/diagnostic` for every open document.
 *
 * Firing a pull provider's `onDidChangeDiagnosticsEmitter` re-pulls all open
 * documents the provider matches, the same path the server's
 * `workspace/diagnostic/refresh` drives. The `DiagnosticFilter` needs this on
 * every mute toggle because a toggle changes only the client-side filter
 * (no server round-trip), and open-file diagnostics arrive via the LSP 3.17
 * pull path, owned by vscode-languageclient rather than the push
 * `DiagnosticCollection` the filter re-publishes directly. Without the re-pull,
 * undoing "hide all findings" leaves open files stuck hidden until the next
 * edit (regression from enabling pull diagnostics in VS Code).
 *
 * No-op when the pull feature is not registered (push-only server, or pull not
 * yet initialized) or when no open document matches the fallow selector.
 */
export const triggerPullDiagnosticRefresh = (lspClient: LanguageClient): void => {
  const feature = lspClient.getFeature(DocumentDiagnosticRequest.method);
  if (!feature) {
    return;
  }
  // `getProvider(document)` returns the same provider instance for every
  // matching document, and one `fire()` re-pulls all of them; dedupe so we
  // fire each unique provider exactly once.
  const fired = new Set<DiagnosticProviderShape>();
  for (const document of vscode.workspace.textDocuments) {
    if (document.uri.scheme !== "file") {
      continue;
    }
    const provider = feature.getProvider(document);
    if (provider && !fired.has(provider)) {
      fired.add(provider);
      provider.onDidChangeDiagnosticsEmitter.fire();
    }
  }
};

export const startClient = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
  diagnosticFilter?: DiagnosticFilter,
  onAnalysisComplete?: (params: AnalysisCompleteParams) => void,
): Promise<LanguageClient | null> => {
  const binaryPath = await resolveBinaryPath(context, outputChannel);
  if (!binaryPath) {
    return null;
  }

  outputChannel.appendLine(`Using fallow-lsp binary: ${binaryPath}`);

  const serverOptions: ServerOptions = {
    command: binaryPath,
    transport: TransportKind.stdio,
  };

  const traceLevel = getTraceLevel();

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "javascriptreact" },
      { scheme: "file", language: "typescript" },
      { scheme: "file", language: "typescriptreact" },
      { scheme: "file", language: "vue" },
      { scheme: "file", language: "svelte" },
      { scheme: "file", language: "astro" },
      { scheme: "file", language: "mdx" },
      { scheme: "file", language: "json" },
    ],
    outputChannel,
    traceOutputChannel: outputChannel,
    initializationOptions: createInitializationOptions(),
    // VS Code may receive fallow diagnostics via push and LSP 3.17 pull. The
    // middleware keeps diagnostic muting applied before VS Code stores either.
    middleware: diagnosticFilter
      ? {
          handleDiagnostics: (uri, diagnostics, next) =>
            diagnosticFilter.handleDiagnostics(uri, diagnostics, next),
          provideDiagnostics: (document, previousResultId, token, next) =>
            diagnosticFilter.provideDiagnostics(document, previousResultId, token, next),
        }
      : undefined,
  };

  const nextClient = new LanguageClient(
    "fallow",
    "Fallow Language Server",
    serverOptions,
    clientOptions,
  );
  client = nextClient;

  if (traceLevel !== "off") {
    void nextClient.setTrace(traceLevel === "verbose" ? Trace.Verbose : Trace.Messages);
  }

  try {
    await nextClient.start();
    if (client !== nextClient) {
      if (nextClient.state === State.Running) {
        await nextClient.stop();
      }
      return null;
    }
    outputChannel.appendLine("Fallow language server started.");
    await loadDiagnosticCategories(nextClient, outputChannel);
    diagnosticFilter?.updateBaselineMutedCategories(getMutedDiagnosticCategories());
    // Register the analysis-complete notification handler on THIS client, not
    // once in activate(). A restart builds a fresh client, so a handler bound to
    // the old client would silently stop firing after the first config-change
    // restart and freeze the status bar. The disposable is bounded by the
    // client's own lifetime (it is torn down when the client stops).
    if (onAnalysisComplete) {
      nextClient.onNotification("fallow/analysisComplete", onAnalysisComplete);
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    outputChannel.appendLine(`Failed to start language server: ${message}`);
    void vscode.window.showErrorMessage(
      `Fallow: failed to start language server. Check the output channel for details.`,
    );
    if (client === nextClient) {
      client = null;
    }
    return null;
  }

  diagnosticFilter?.attachClient({
    // Lazy getter: `LanguageClient.diagnostics` (the push collection) may not
    // exist until the server pushes its first diagnostics, so read it on each
    // refresh rather than snapshotting it here.
    get diagnostics() {
      return nextClient.diagnostics;
    },
    refreshPullDiagnostics: () => triggerPullDiagnosticRefresh(nextClient),
  });

  return nextClient;
};

export const stopClient = async (outputChannel?: vscode.OutputChannel): Promise<void> => {
  const current = client;
  if (!current) {
    return;
  }

  try {
    if (current.state === State.Starting) {
      // Wait for the in-flight start to settle before stopping: the library
      // throws "Client is not running" if stop() is called while Starting.
      // 10s (raised from 2s) covers a slow first parse on a large monorepo; a
      // start hung past that is a bigger problem than a leaked process.
      let disposable: vscode.Disposable | undefined;
      try {
        await Promise.race([
          new Promise<void>((resolve) => {
            disposable = current.onDidChangeState((event) => {
              if (event.newState !== State.Starting) {
                disposable?.dispose();
                disposable = undefined;
                resolve();
              }
            });
          }),
          new Promise<void>((resolve) => setTimeout(resolve, 10_000)),
        ]);
      } finally {
        disposable?.dispose();
      }
    }

    if (current.state === State.Running) {
      await current.stop();
    }
  } catch (err) {
    // The library's own shutdown can reject (e.g. "Stopping the server timed
    // out") when the process already died but onConnectionClosed has not fired.
    // Swallow it: an uncaught rejection here propagates through restartClient,
    // skips the subsequent startClient, and leaves a stale non-null `client`
    // (LSP permanently dead, silently). The finally below always clears it.
    const message = err instanceof Error ? err.message : String(err);
    outputChannel?.appendLine(`Fallow: error stopping language server: ${message}`);
  } finally {
    if (client === current) {
      client = null;
    }
  }
};

export const restartClient = (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
  diagnosticFilter?: DiagnosticFilter,
  onAnalysisComplete?: (params: AnalysisCompleteParams) => void,
): Promise<LanguageClient | null> => {
  const doRestart = async (): Promise<LanguageClient | null> => {
    // Detach BEFORE stop so a user toggle that fires during the gap can't
    // call refresh() against a disposed DiagnosticCollection. startClient
    // re-attaches once the new client is up.
    diagnosticFilter?.detachClient();
    await stopClient(outputChannel);
    return startClient(context, outputChannel, diagnosticFilter, onAnalysisComplete);
  };
  // Chain onto the queue on BOTH the fulfilled and rejected paths so a prior
  // restart that somehow rejected cannot deadlock all future restarts.
  restartQueue = restartQueue.then(doRestart, doRestart);
  return restartQueue;
};
