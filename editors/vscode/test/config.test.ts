import { beforeEach, describe, expect, it, vi } from "vitest";

type InspectValue<T> = {
  readonly defaultValue?: T;
  readonly globalValue?: T;
  readonly workspaceValue?: T;
  readonly workspaceFolderValue?: T;
  readonly globalLanguageValue?: T;
  readonly workspaceLanguageValue?: T;
  readonly workspaceFolderLanguageValue?: T;
};

let inspected: Record<string, InspectValue<unknown> | undefined> = {};
let configured: Record<string, unknown> = {};

vi.mock("vscode", () => ({
  workspace: {
    workspaceFolders: undefined,
    getConfiguration: () => ({
      get: <T>(key: string, fallback: T): T => (configured[key] as T | undefined) ?? fallback,
      inspect: <T>(key: string): InspectValue<T> | undefined =>
        inspected[key] as InspectValue<T> | undefined,
    }),
  },
}));

import {
  getDuplicationCrossLanguageOverride,
  getDuplicationIgnoreImportsOverride,
  getDuplicationMinLinesOverride,
  getDuplicationMinOccurrencesOverride,
  getDuplicationMinTokensOverride,
  getDuplicationModeOverride,
  getDuplicationSkipLocalOverride,
  getDuplicationThresholdOverride,
  getDiagnosticSeverity,
  getMutedDiagnosticCategories,
  getHealthInlineComplexity,
} from "../src/config.js";

describe("duplication setting overrides", () => {
  beforeEach(() => {
    inspected = {};
    configured = {};
  });

  it("ignores package defaults so project config can win", () => {
    inspected = {
      "duplication.mode": { defaultValue: "mild" },
      "duplication.threshold": { defaultValue: 0 },
      "duplication.minTokens": { defaultValue: 50 },
      "duplication.minLines": { defaultValue: 5 },
      "duplication.minOccurrences": { defaultValue: 2 },
      "duplication.skipLocal": { defaultValue: false },
      "duplication.crossLanguage": { defaultValue: false },
      "duplication.ignoreImports": { defaultValue: false },
    };

    expect(getDuplicationModeOverride()).toBeUndefined();
    expect(getDuplicationThresholdOverride()).toBeUndefined();
    expect(getDuplicationMinTokensOverride()).toBeUndefined();
    expect(getDuplicationMinLinesOverride()).toBeUndefined();
    expect(getDuplicationMinOccurrencesOverride()).toBeUndefined();
    expect(getDuplicationSkipLocalOverride()).toBeUndefined();
    expect(getDuplicationCrossLanguageOverride()).toBeUndefined();
    expect(getDuplicationIgnoreImportsOverride()).toBeUndefined();
  });

  it("returns explicit configured values, including defaults used as overrides", () => {
    inspected = {
      "duplication.mode": { workspaceValue: "mild" },
      "duplication.threshold": { workspaceValue: 0 },
      "duplication.minTokens": { workspaceValue: 50 },
      "duplication.minLines": { workspaceValue: 5 },
      "duplication.minOccurrences": { workspaceValue: 2 },
      "duplication.skipLocal": { workspaceValue: false },
      "duplication.crossLanguage": { workspaceValue: false },
      "duplication.ignoreImports": { workspaceValue: false },
    };

    expect(getDuplicationModeOverride()).toBe("mild");
    expect(getDuplicationThresholdOverride()).toBe(0);
    expect(getDuplicationMinTokensOverride()).toBe(50);
    expect(getDuplicationMinLinesOverride()).toBe(5);
    expect(getDuplicationMinOccurrencesOverride()).toBe(2);
    expect(getDuplicationSkipLocalOverride()).toBe(false);
    expect(getDuplicationCrossLanguageOverride()).toBe(false);
    expect(getDuplicationIgnoreImportsOverride()).toBe(false);
  });

  it("clamps hand-edited numeric overrides before forwarding them", () => {
    inspected = {
      "duplication.minLines": { workspaceValue: 0 },
      "duplication.minOccurrences": { workspaceValue: 1 },
    };

    expect(getDuplicationMinLinesOverride()).toBe(1);
    expect(getDuplicationMinOccurrencesOverride()).toBe(2);
  });
});

describe("health inline complexity setting", () => {
  it("defaults on (the extension renders the complexity lens)", () => {
    expect(getHealthInlineComplexity()).toBe(true);
  });
});

describe("diagnostics severity setting", () => {
  beforeEach(() => {
    configured = {};
  });

  it("defaults to warning", () => {
    expect(getDiagnosticSeverity()).toBe("warning");
  });

  it("accepts information and hint", () => {
    configured = { "diagnostics.severity": "information" };
    expect(getDiagnosticSeverity()).toBe("information");
    configured = { "diagnostics.severity": "hint" };
    expect(getDiagnosticSeverity()).toBe("hint");
  });

  it("falls back to warning for unknown values", () => {
    configured = { "diagnostics.severity": "quiet" };
    expect(getDiagnosticSeverity()).toBe("warning");
  });
});

describe("diagnostic muted category setting", () => {
  beforeEach(() => {
    configured = {};
  });

  it("returns only known diagnostic category codes", () => {
    configured = {
      "diagnostics.mutedCategories": [
        "code-duplication",
        "future-unknown",
        42,
        "stale-suppression",
      ],
    };

    expect(Array.from(getMutedDiagnosticCategories()).toSorted()).toEqual([
      "code-duplication",
      "stale-suppression",
    ]);
  });

  it("ignores non-array values", () => {
    configured = {
      "diagnostics.mutedCategories": "code-duplication",
    };

    expect(getMutedDiagnosticCategories().size).toBe(0);
  });
});
