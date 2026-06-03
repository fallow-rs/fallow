import * as fs from "node:fs";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
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
} from "./config.js";
import { findBinaryInPath, findLocalBinary } from "./binary-utils.js";
import type { DiagnosticFilter } from "./diagnosticFilter.js";
import {
  parseDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "./diagnosticFilter.js";
import { downloadBinary, getBinaryVersion, getInstalledBinaryPath } from "./download.js";

let client: LanguageClient | null = null;

const warnIfVersionMismatch = (binaryPath: string, outputChannel?: vscode.OutputChannel): void => {
  const extensionVersion = vscode.extensions.getExtension("fallow-rs.fallow-vscode")?.packageJSON
    ?.version as string | undefined;
  if (!extensionVersion) return;

  const binaryVersion = getBinaryVersion(binaryPath);
  if (binaryVersion && binaryVersion !== extensionVersion) {
    const msg = `Fallow: binary in PATH is v${binaryVersion}, extension is v${extensionVersion}. Update the binary or remove it from PATH to use the managed auto-download.`;
    outputChannel?.appendLine(msg);
    void vscode.window.showWarningMessage(msg);
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
    initializationOptions: {
      issueTypes: getIssueTypes(),
      changedSince: getChangedSince(),
      configPath: getResolvedConfigPath(),
    },
    middleware: diagnosticFilter
      ? {
          handleDiagnostics: (uri, diagnostics, next) =>
            diagnosticFilter.handleDiagnostics(uri, diagnostics, next),
          provideDiagnostics: (document, previousResultId, token, next) =>
            diagnosticFilter.provideDiagnostics(document, previousResultId, token, next),
        }
      : undefined,
  };

  client = new LanguageClient("fallow", "Fallow Language Server", serverOptions, clientOptions);

  if (traceLevel !== "off") {
    void client.setTrace(traceLevel === "verbose" ? Trace.Verbose : Trace.Messages);
  }

  try {
    await client.start();
    outputChannel.appendLine("Fallow language server started.");
    await loadDiagnosticCategories(client, outputChannel);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    outputChannel.appendLine(`Failed to start language server: ${message}`);
    void vscode.window.showErrorMessage(
      `Fallow: failed to start language server. Check the output channel for details.`,
    );
    client = null;
    return null;
  }

  diagnosticFilter?.attachClient(client);

  return client;
};

export const stopClient = async (): Promise<void> => {
  if (client) {
    await client.stop();
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
