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
  LanguageClient: class {},
  State: {
    Stopped: 1,
    Running: 2,
    Starting: 3,
  },
  TransportKind: {
    stdio: 0,
  },
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
}));

import { createInitializationOptions, loadDiagnosticCategories } from "../src/client.js";
import {
  DIAGNOSTIC_CATEGORIES,
  getDiagnosticCategories,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";

afterEach(() => {
  resetDiagnosticCategories();
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
