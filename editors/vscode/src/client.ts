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
} from "vscode-languageclient/node";
import { DocumentDiagnosticRequest, Trace } from "vscode-languageserver-protocol";
import {
  getLspPath,
  getTraceLevel,
  getAutoDownload,
  getIssueTypes,
  getChangedSince,
  getResolvedConfigPath,
  getAllowRemoteExtends,
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
import {
  findBinaryInPath,
  findLocalLspBinary,
  resolveConfiguredBinaryPath,
  type ResolvedLspBinary,
} from "./binary-utils.js";
import type { DiagnosticFilter } from "./diagnosticFilter.js";
// fallow-ignore-next-line unresolved-import
import type { LspInitializationOptions } from "./generated/lsp-initialization-options.js";
import type { AnalysisCompleteParams } from "./statusBar-utils.js";
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

export const createInitializationOptions = (): LspInitializationOptions => ({
  issueTypes: getIssueTypes(),
  changedSince: getChangedSince(),
  configPath: getResolvedConfigPath(),
  allowRemoteExtends: getAllowRemoteExtends(),
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

/** Wrap a bare, directly-spawnable binary path as a zero-arg LSP invocation. */
const bareLsp = (command: string): ResolvedLspBinary => ({ command, args: [] });

const resolveBinaryPath = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<ResolvedLspBinary | null> => {
  const configPath = getLspPath();
  if (configPath) {
    // Honor `fallow.lspPath` as the user typed it, tolerating a missing Windows
    // extension and a directory pointing at the install folder, so a manually
    // set path actually resolves instead of silently failing existsSync.
    const resolved = resolveConfiguredBinaryPath(configPath, "fallow-lsp");
    if (resolved) {
      outputChannel?.appendLine(`Binary resolution: using fallow.lspPath setting: ${resolved}`);
      return bareLsp(resolved);
    }
    void vscode.window.showWarningMessage(
      `Fallow: configured LSP path "${configPath}" does not point to a fallow-lsp binary.`,
    );
    return null;
  }

  // The only branch that can produce the multicall `fallow lsp-server` form,
  // when the workspace platform package ships the single bundled binary.
  const local = findLocalLspBinary();
  if (local) {
    const suffix = local.args.length > 0 ? ` ${local.args.join(" ")}` : "";
    outputChannel?.appendLine(
      `Binary resolution: using local node_modules: ${local.command}${suffix}`,
    );
    return local;
  }
  outputChannel?.appendLine("Binary resolution: no local node_modules fallow-lsp found");

  const inPath = findBinaryInPath("fallow-lsp");
  if (inPath) {
    outputChannel?.appendLine(`Binary resolution: using system PATH: ${inPath}`);
    // Fire-and-forget: the skew toast must not block binary resolution on the
    // up-to-5s `--version` spawn (the reason getBinaryVersion is async).
    void warnIfVersionMismatch(inPath, outputChannel);
    return bareLsp(inPath);
  }
  outputChannel?.appendLine("Binary resolution: fallow-lsp not found in PATH");

  const installed = await getInstalledBinaryPath(context, outputChannel);
  if (installed) {
    outputChannel?.appendLine(
      `Binary resolution: using previously downloaded binary: ${installed}`,
    );
    return bareLsp(installed);
  }

  if (getAutoDownload()) {
    const downloaded = await downloadBinary(context);
    return downloaded ? bareLsp(downloaded) : null;
  }

  const choice = await vscode.window.showErrorMessage(
    "Fallow: fallow-lsp binary not found. Would you like to download it?",
    "Download",
    "Set Path",
    "Cancel",
  );

  if (choice === "Download") {
    const downloaded = await downloadBinary(context);
    return downloaded ? bareLsp(downloaded) : null;
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

/** Custom request that asks fallow-lsp to re-drive `workspace/diagnostic/refresh`. */
const REFRESH_DIAGNOSTICS_METHOD = "fallow/refreshDiagnostics";

/**
 * Force VS Code to re-pull `textDocument/diagnostic` for every open document
 * by firing each open document's pull provider directly.
 *
 * This is the client-side fast path / fallback for older servers. It is gated
 * per document by `getProvider(document)`, which matches the document against
 * the pull registration's `documentSelector`; if that match returns nothing
 * (selector skew, a provider registered without our selector, timing) the fire
 * is silently a no-op and the un-hide does not re-render. `requestServerRefresh`
 * covers that gap by routing through the server's `getAllProviders()` path.
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

/**
 * Ask fallow-lsp to re-send `workspace/diagnostic/refresh`.
 *
 * The server handler fires EVERY registered pull provider via
 * `getAllProviders()`, the same mechanism it uses after analysis and on
 * `document open` (the latter is the close-and-reopen workaround users fall
 * back to). Unlike the client-side `triggerPullDiagnosticRefresh`, this is not
 * gated by a per-document `getProvider(document)` selector match, so it
 * re-renders open-file squiggles reliably when a mute toggle is undone
 * (discussion #287).
 *
 * Fire-and-forget: older fallow-lsp binaries do not implement the request and
 * reply `MethodNotFound`; the local re-pull above already ran, so the rejection
 * is swallowed.
 */
export const requestServerDiagnosticRefresh = async (lspClient: LanguageClient): Promise<void> => {
  try {
    await lspClient.sendRequest(REFRESH_DIAGNOSTICS_METHOD);
  } catch {
    // Older server without the handler (MethodNotFound), or a client that is
    // shutting down. The client-side re-pull is the fallback.
  }
};

export const startClient = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.LogOutputChannel,
  diagnosticFilter?: DiagnosticFilter,
  onAnalysisComplete?: (params: AnalysisCompleteParams) => void,
): Promise<LanguageClient | null> => {
  const resolved = await resolveBinaryPath(context, outputChannel);
  if (!resolved) {
    return null;
  }

  const invocation =
    resolved.args.length > 0 ? `${resolved.command} ${resolved.args.join(" ")}` : resolved.command;
  outputChannel.appendLine(`Using fallow-lsp binary: ${invocation}`);

  const serverOptions: ServerOptions = {
    command: resolved.command,
    args: [...resolved.args],
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
    refreshPullDiagnostics: () => {
      // Fire the local providers first (fast, covers older servers), then ask
      // the server to re-drive `workspace/diagnostic/refresh` so the un-hide
      // re-renders open files even when the per-document `getProvider` match
      // above fired nothing (discussion #287).
      triggerPullDiagnosticRefresh(nextClient);
      void requestServerDiagnosticRefresh(nextClient);
    },
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
  outputChannel: vscode.LogOutputChannel,
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
