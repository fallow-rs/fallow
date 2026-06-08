import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let mockIssueTypes = {};
let mockChangedSince = "";
let mockConfigPath = "";
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

const mockLanguageClient = vi.hoisted(() => ({
  instances: [] as Array<{
    start: ReturnType<typeof vi.fn>;
    stop: ReturnType<typeof vi.fn>;
    setTrace: ReturnType<typeof vi.fn>;
    sendRequest: ReturnType<typeof vi.fn>;
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
}));

vi.mock("vscode-languageclient/node.js", () => ({
  LanguageClient: class {
    state = 2;
    private stateListeners: Array<(event: { newState: number }) => void> = [];
    readonly start = vi.fn(async () => undefined);
    readonly stop = vi.fn(async () => undefined);
    readonly setTrace = vi.fn(async () => undefined);
    readonly sendRequest = vi.fn(async () => mockIssueTypesResponse);
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
  startClient,
  stopClient,
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
    expect(filter.attachClient).toHaveBeenCalledWith(client);
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
