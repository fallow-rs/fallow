import * as fs from "node:fs";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  State,
  TransportKind,
} from "vscode-languageclient/node.js";
import { Trace } from "vscode-languageserver-protocol";
import {
  getLspPath,
  getTraceLevel,
  getAutoDownload,
  getIssueTypes,
  getChangedSince,
  getResolvedConfigPath,
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
import type { DuplicationMode, IssueTypeConfig } from "./types.js";
import {
  parseDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "./diagnosticFilter.js";
import { downloadBinary, getBinaryVersion, getInstalledBinaryPath } from "./download.js";

let client: LanguageClient | null = null;

export interface LspInitializationOptions {
  readonly issueTypes: IssueTypeConfig;
  readonly changedSince: string;
  readonly configPath: string;
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

const warnIfVersionMismatch = (binaryPath: string, outputChannel?: vscode.OutputChannel): void => {
  const extensionVersion = vscode.extensions.getExtension("fallow-rs.fallow-vscode")?.packageJSON
    ?.version as string | undefined;
  if (!extensionVersion) return;

  const binaryVersion = getBinaryVersion(binaryPath);
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
    warnIfVersionMismatch(inPath, outputChannel);
    return inPath;
  }
  outputChannel?.appendLine("Binary resolution: fallow-lsp not found in PATH");

  const installed = getInstalledBinaryPath(context, outputChannel);
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

export const startClient = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
  diagnosticFilter?: DiagnosticFilter,
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

  diagnosticFilter?.attachClient(nextClient);

  return nextClient;
};

export const stopClient = async (): Promise<void> => {
  const current = client;
  if (!current) {
    return;
  }

  if (current.state === State.Starting) {
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
        new Promise<void>((resolve) => setTimeout(resolve, 2_000)),
      ]);
    } finally {
      disposable?.dispose();
    }
  }

  if (current.state === State.Running) {
    await current.stop();
  }

  if (client === current) {
    client = null;
  }
};

export const restartClient = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
  diagnosticFilter?: DiagnosticFilter,
): Promise<LanguageClient | null> => {
  // Detach BEFORE stop so a user toggle that fires during the gap can't
  // call refresh() against a disposed DiagnosticCollection. startClient
  // re-attaches once the new client is up.
  diagnosticFilter?.detachClient();
  await stopClient();
  return startClient(context, outputChannel, diagnosticFilter);
};
