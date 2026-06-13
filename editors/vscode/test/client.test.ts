import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let mockIssueTypes = {};
let mockChangedSince = "";
let mockConfigPath = "";
let mockProductionOverride: boolean | undefined;
let mockDuplicationMode = "mild";
let mockDuplicationThreshold = 0;
let mockDuplicationMinTokens = 50;
let mockDuplicationMinLines = 5;
let mockDuplicationMinOccurrences = 2;
let mockDuplicationSkipLocal = false;
let mockDuplicationCrossLanguage = false;
let mockDuplicationIgnoreImports = false;
let mockHealthInlineComplexity = false;
let mockMutedDiagnosticCategories = new Set<string>();
let mockIssueTypesResponse: unknown = [];

const mockBinaryResolution = vi.hoisted(() => ({
  localBinary: "/mock/fallow-lsp" as string | null,
  pathBinary: null as string | null,
  installedBinary: null as string | null,
}));

const mockWorkspace = vi.hoisted(() => ({
  textDocuments: [] as Array<{ uri: { scheme: string }; languageId: string }>,
}));

const mockLanguageClient = vi.hoisted(() => ({
  instances: [] as Array<{
    start: ReturnType<typeof vi.fn>;
    stop: ReturnType<typeof vi.fn>;
    setTrace: ReturnType<typeof vi.fn>;
    sendRequest: ReturnType<typeof vi.fn>;
    onNotification: ReturnType<typeof vi.fn>;
    state: number;
    onDidChangeState: ReturnType<typeof vi.fn>;
    emitState: (newState: number) => void;
  }>,
}));

vi.mock("vscode", () => ({
  extensions: {
    getExtension: vi.fn(),
  },
  window: {
    showErrorMessage: vi.fn(),
    showWarningMessage: vi.fn(),
  },
  workspace: {
    get textDocuments() {
      return mockWorkspace.textDocuments;
    },
  },
}));

vi.mock("vscode-languageclient/node.js", () => ({
  LanguageClient: class {
    state = 2;
    private stateListeners: Array<(event: { newState: number }) => void> = [];
    readonly start = vi.fn(async () => undefined);
    readonly stop = vi.fn(async () => undefined);
    readonly setTrace = vi.fn(async () => undefined);
    readonly sendRequest = vi.fn(async () => mockIssueTypesResponse);
    readonly onNotification = vi.fn(() => ({ dispose: vi.fn() }));
    readonly onDidChangeState = vi.fn((listener: (event: { newState: number }) => void) => {
      this.stateListeners.push(listener);
      return {
        dispose: () => {
          this.stateListeners = this.stateListeners.filter((item) => item !== listener);
        },
      };
    });

    emitState(newState: number) {
      this.state = newState;
      for (const listener of this.stateListeners) {
        listener({ newState });
      }
    }

    constructor() {
      mockLanguageClient.instances.push(this);
    }
  },
  State: {
    Stopped: 1,
    Running: 2,
    Starting: 3,
  },
  TransportKind: {
    stdio: 0,
  },
}));

vi.mock("../src/binary-utils.js", () => ({
  findLocalBinary: () => mockBinaryResolution.localBinary,
  findBinaryInPath: () => mockBinaryResolution.pathBinary,
}));

vi.mock("../src/download.js", () => ({
  downloadBinary: vi.fn(async () => null),
  getBinaryVersion: vi.fn(() => null),
  getInstalledBinaryPath: vi.fn(() => mockBinaryResolution.installedBinary),
}));

vi.mock("../src/config.js", () => ({
  getLspPath: () => "",
  getTraceLevel: () => "off",
  getAutoDownload: () => false,
  getIssueTypes: () => mockIssueTypes,
  getChangedSince: () => mockChangedSince,
  getResolvedConfigPath: () => mockConfigPath,
  getProductionOverride: () => mockProductionOverride,
  getDuplicationModeOverride: () => mockDuplicationMode,
  getDuplicationThresholdOverride: () => mockDuplicationThreshold,
  getDuplicationMinTokensOverride: () => mockDuplicationMinTokens,
  getDuplicationMinLinesOverride: () => mockDuplicationMinLines,
  getDuplicationMinOccurrencesOverride: () => mockDuplicationMinOccurrences,
  getDuplicationSkipLocalOverride: () => mockDuplicationSkipLocal,
  getDuplicationCrossLanguageOverride: () => mockDuplicationCrossLanguage,
  getDuplicationIgnoreImportsOverride: () => mockDuplicationIgnoreImports,
  getHealthInlineComplexity: () => mockHealthInlineComplexity,
  getMutedDiagnosticCategories: () => mockMutedDiagnosticCategories,
}));

import {
  createInitializationOptions,
  loadDiagnosticCategories,
  restartClient,
  startClient,
  stopClient,
  triggerPullDiagnosticRefresh,
} from "../src/client.js";
import {
  DIAGNOSTIC_CATEGORIES,
  getDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";

afterEach(async () => {
  resetDiagnosticCategories();
  await stopClient();
});

beforeEach(() => {
  mockIssueTypes = { "code-duplication": true };
  mockChangedSince = "origin/main";
  mockConfigPath = "/workspace/.fallowrc.jsonc";
  mockProductionOverride = undefined;
  mockDuplicationMode = "semantic";
  mockDuplicationThreshold = 8;
  mockDuplicationMinTokens = 80;
  mockDuplicationMinLines = 9;
  mockDuplicationMinOccurrences = 3;
  mockDuplicationSkipLocal = true;
  mockDuplicationCrossLanguage = true;
  mockDuplicationIgnoreImports = true;
  mockHealthInlineComplexity = false;
  mockMutedDiagnosticCategories = new Set();
  mockIssueTypesResponse = [];
  mockBinaryResolution.localBinary = "/mock/fallow-lsp";
  mockBinaryResolution.pathBinary = null;
  mockBinaryResolution.installedBinary = null;
  mockLanguageClient.instances = [];
  mockWorkspace.textDocuments = [];
});

const outputChannel = () => ({
  lines: [] as string[],
  appendLine(line: string) {
    this.lines.push(line);
  },
});

describe("createInitializationOptions", () => {
  it("forwards duplication settings to fallow-lsp", () => {
    expect(createInitializationOptions()).toEqual({
      issueTypes: { "code-duplication": true },
      changedSince: "origin/main",
      configPath: "/workspace/.fallowrc.jsonc",
      production: undefined,
      duplication: {
        mode: "semantic",
        threshold: 8,
        minTokens: 80,
        minLines: 9,
        minOccurrences: 3,
        skipLocal: true,
        crossLanguage: true,
        ignoreImports: true,
      },
    });
  });

  it("forwards the production override so the LSP matches the sidebar (#1055)", () => {
    mockProductionOverride = true;
    expect(createInitializationOptions().production).toBe(true);

    mockProductionOverride = false;
    expect(createInitializationOptions().production).toBe(false);
  });

  it("omits production when deferring to the project config (auto)", () => {
    mockProductionOverride = undefined;
    const options = createInitializationOptions();
    expect(options.production).toBeUndefined();
    // JSON.stringify drops `undefined`, so the LSP sees no `production` key and
    // reads the project config, matching the sidebar's no-flag behavior.
    expect("production" in JSON.parse(JSON.stringify(options))).toBe(false);
  });

  it("does not forward inline complexity to fallow-lsp (extension owns the lens)", () => {
    // The extension renders its own complexity lens (ComplexityLensProvider) so
    // it can toggle the per-line breakdown; forwarding the LSP option too would
    // double-render. The LSP lens stays opt-in for other editors.
    mockHealthInlineComplexity = true;

    expect("health" in createInitializationOptions()).toBe(false);
  });
});

describe("loadDiagnosticCategories", () => {
  it("loads categories from fallow/issueTypes", async () => {
    const out = outputChannel();
    const client = {
      sendRequest: vi.fn(async () => [{ code: "future-rule", label: "Future Rule" }]),
    };

    await loadDiagnosticCategories(client as never, out as never);

    expect(client.sendRequest).toHaveBeenCalledWith("fallow/issueTypes");
    expect(getDiagnosticCategories()).toEqual([{ code: "future-rule", label: "Future Rule" }]);
    expect(out.lines.at(-1)).toBe("Loaded 1 diagnostic categories from fallow-lsp.");
  });

  it("refreshes diagnostic mute baseline after loading live categories", async () => {
    mockIssueTypesResponse = [{ code: "future-rule", label: "Future Rule" }];
    mockMutedDiagnosticCategories = new Set(["future-rule"]);
    const filter = {
      attachClient: vi.fn(),
      updateBaselineMutedCategories: vi.fn(),
    };

    const client = await startClient({} as never, outputChannel() as never, filter as never);

    expect(client).not.toBeNull();
    expect(filter.updateBaselineMutedCategories).toHaveBeenCalledWith(
      new Set(["future-rule"]),
    );
    // attachClient receives an adapter (not the raw client): a lazy
    // `diagnostics` getter delegating to the push collection plus a
    // `refreshPullDiagnostics` hook that re-pulls open documents on a mute
    // toggle (the pull-mode fix).
    const attachArg = filter.attachClient.mock.calls[0]?.[0] as {
      diagnostics: unknown;
      refreshPullDiagnostics: () => void;
    };
    expect(attachArg.diagnostics).toBe(client!.diagnostics);
    expect(typeof attachArg.refreshPullDiagnostics).toBe("function");
  });

  it("falls back to bundled categories when the request fails", async () => {
    setDiagnosticCategories([{ code: "stale-rule", label: "Stale Rule" }]);
    const out = outputChannel();
    const client = {
      sendRequest: vi.fn(async () => {
        throw new Error("method not found");
      }),
    };

    await loadDiagnosticCategories(client as never, out as never);

    expect(getDiagnosticCategories()).toBe(DIAGNOSTIC_CATEGORIES);
    expect(out.lines.at(-1)).toContain("using bundled diagnostic categories");
  });
});

describe("stopClient", () => {
  it("waits for a starting client before stopping it", async () => {
    const out = outputChannel();
    const client = await startClient({} as never, out as never);

    expect(client).not.toBeNull();
    expect(mockLanguageClient.instances).toHaveLength(1);
    const instance = mockLanguageClient.instances[0];
    expect(instance).toBeDefined();

    instance!.state = 3;
    const stopped = stopClient();

    expect(instance!.stop).not.toHaveBeenCalled();
    instance!.emitState(2);

    await expect(stopped).resolves.toBeUndefined();
    expect(instance!.onDidChangeState).toHaveBeenCalledOnce();
    expect(instance!.stop).toHaveBeenCalledOnce();
  });
});

describe("restartClient lifecycle", () => {
  it("re-registers the analysisComplete handler on the new client", async () => {
    const out = outputChannel();
    await startClient({} as never, out as never);
    const onAnalysisComplete = vi.fn();

    await restartClient({} as never, out as never, undefined, onAnalysisComplete);

    // A restart builds a fresh client; the handler must be registered on it,
    // not left on the old (now-stopped) client, or the status bar freezes.
    const fresh = mockLanguageClient.instances[mockLanguageClient.instances.length - 1];
    expect(fresh!.onNotification).toHaveBeenCalledWith(
      "fallow/analysisComplete",
      onAnalysisComplete,
    );
  });

  it("recovers when the old client's stop() rejects (no permanently-dead LSP)", async () => {
    const out = outputChannel();
    await startClient({} as never, out as never);
    const first = mockLanguageClient.instances[0];
    // Simulate the library's shutdown timeout rejecting.
    first!.stop.mockRejectedValueOnce(new Error("Stopping the server timed out"));

    const restarted = await restartClient({} as never, out as never);

    // The rejection is swallowed, the stale client is cleared in `finally`, and
    // startClient still runs, so we end with a fresh live client (not a dead one).
    expect(restarted).not.toBeNull();
    expect(mockLanguageClient.instances).toHaveLength(2);
    expect(restarted).toBe(mockLanguageClient.instances[1]);
  });

  it("serializes concurrent restarts so the prior client is stopped exactly once", async () => {
    const out = outputChannel();
    await startClient({} as never, out as never);

    // Two restarts fired without awaiting the first. Without serialization both
    // would observe the same live client and each call stop() on it.
    const [a, b] = await Promise.all([
      restartClient({} as never, out as never),
      restartClient({} as never, out as never),
    ]);

    expect(a).not.toBeNull();
    expect(b).not.toBeNull();
    // 1 initial + 2 restarts, each stopping only the immediately-prior client once.
    expect(mockLanguageClient.instances).toHaveLength(3);
    expect(mockLanguageClient.instances[0]!.stop).toHaveBeenCalledOnce();
    expect(mockLanguageClient.instances[1]!.stop).toHaveBeenCalledOnce();
    expect(mockLanguageClient.instances[2]!.stop).not.toHaveBeenCalled();
  });
});

describe("triggerPullDiagnosticRefresh", () => {
  const fileDoc = (languageId: string) => ({
    uri: { scheme: "file" },
    languageId,
  });

  it("fires each unique provider once to re-pull open documents", () => {
    const provider = { onDidChangeDiagnosticsEmitter: { fire: vi.fn() } };
    const getProvider = vi.fn(() => provider);
    const getFeature = vi.fn(() => ({ getProvider }));
    mockWorkspace.textDocuments = [fileDoc("typescript"), fileDoc("javascript")];

    triggerPullDiagnosticRefresh({ getFeature } as never);

    expect(getFeature).toHaveBeenCalledWith("textDocument/diagnostic");
    // Both open docs share one provider instance, so it fires exactly once.
    expect(provider.onDidChangeDiagnosticsEmitter.fire).toHaveBeenCalledTimes(1);
  });

  it("is a no-op when the pull feature is not registered", () => {
    const getFeature = vi.fn(() => undefined);
    mockWorkspace.textDocuments = [fileDoc("typescript")];
    expect(() => triggerPullDiagnosticRefresh({ getFeature } as never)).not.toThrow();
    expect(getFeature).toHaveBeenCalledWith("textDocument/diagnostic");
  });

  it("skips non-file documents (output channels, git, etc.)", () => {
    const provider = { onDidChangeDiagnosticsEmitter: { fire: vi.fn() } };
    const getProvider = vi.fn(() => provider);
    const getFeature = vi.fn(() => ({ getProvider }));
    mockWorkspace.textDocuments = [{ uri: { scheme: "output" }, languageId: "log" }];

    triggerPullDiagnosticRefresh({ getFeature } as never);

    expect(getProvider).not.toHaveBeenCalled();
    expect(provider.onDidChangeDiagnosticsEmitter.fire).not.toHaveBeenCalled();
  });

  it("does not fire when getProvider returns undefined for the document", () => {
    const getProvider = vi.fn(() => undefined);
    const getFeature = vi.fn(() => ({ getProvider }));
    mockWorkspace.textDocuments = [fileDoc("typescript")];

    expect(() => triggerPullDiagnosticRefresh({ getFeature } as never)).not.toThrow();
    expect(getProvider).toHaveBeenCalledTimes(1);
  });
});
