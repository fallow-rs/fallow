// The VS Code row of the run parity matrix. Each case in
// tests/fixtures/run-parity/cases gives VS Code settings and the keys of the
// initialization options to compare. The row builds the initialization
// options with the real createInitializationOptions and the real setting
// getters, and compares them on those keys with the `lsp` input of the case,
// which the language server row resolves.
import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const SETTING_PREFIX = "fallow.";

const settings = vi.hoisted(() => ({ values: {} as Record<string, unknown> }));

vi.mock("vscode", () => ({
  extensions: { getExtension: vi.fn() },
  window: { showErrorMessage: vi.fn(), showWarningMessage: vi.fn() },
  workspace: {
    textDocuments: [],
    workspaceFolders: [{ uri: { fsPath: "/workspace" } }],
    getConfiguration: () => ({
      get: <T>(key: string, fallback: T): T => (settings.values[key] as T | undefined) ?? fallback,
      inspect: <T>(key: string): { workspaceValue?: T } | undefined =>
        key in settings.values ? { workspaceValue: settings.values[key] as T } : undefined,
    }),
  },
  Uri: { file: (fsPath: string) => ({ fsPath }) },
}));

vi.mock("vscode-languageclient/node", () => ({
  LanguageClient: class {},
  State: { Stopped: 1, Running: 2, Starting: 3 },
  TransportKind: { stdio: 0 },
}));

vi.mock("../src/binary-utils.js", () => ({
  findLocalLspBinary: () => null,
  findBinaryInPath: () => null,
  resolveConfiguredBinaryPath: (configured: string) => configured,
}));

vi.mock("../src/download.js", () => ({
  downloadBinary: vi.fn(async () => null),
  getBinaryVersion: vi.fn(() => null),
  getInstalledBinaryPath: vi.fn(() => null),
}));

import { createInitializationOptions } from "../src/client.js";

interface VscodeInput {
  readonly settings?: Record<string, unknown>;
  readonly compare_keys?: readonly string[];
  readonly not_expressible?: string;
}

interface LspInput {
  readonly initializationOptions?: Record<string, unknown>;
  readonly not_expressible?: string;
}

interface ParityCase {
  readonly name: string;
  readonly inputs: { readonly vscode?: VscodeInput; readonly lsp?: LspInput };
}

const casesDir = resolve(__dirname, "../../../tests/fixtures/run-parity/cases");

const loadCases = (): ParityCase[] =>
  readdirSync(casesDir)
    .filter((file) => file.endsWith(".json"))
    .sort()
    .map((file) => JSON.parse(readFileSync(join(casesDir, file), "utf8")) as ParityCase);

const settingValues = (input: VscodeInput): Record<string, unknown> =>
  Object.fromEntries(
    Object.entries(input.settings ?? {}).map(([name, value]) => {
      if (!name.startsWith(SETTING_PREFIX)) {
        throw new Error(`the setting ${name} has no ${SETTING_PREFIX} prefix`);
      }
      return [name.slice(SETTING_PREFIX.length), value];
    }),
  );

const projection = (
  options: Record<string, unknown>,
  keys: readonly string[],
): Record<string, unknown> => Object.fromEntries(keys.map((key) => [key, options[key]]));

const cases = loadCases();

describe("run parity: the VS Code row", () => {
  beforeEach(() => {
    settings.values = {};
  });

  it("reads the cases of the matrix", () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  it.each(cases.map((parityCase) => [parityCase.name, parityCase] as const))(
    "%s",
    (_name, parityCase) => {
      const vscode = parityCase.inputs.vscode;
      expect(vscode, "the case has a vscode input").toBeDefined();
      if (vscode?.not_expressible !== undefined) {
        expect(vscode.not_expressible.trim(), "a not_expressible row needs a reason").not.toBe("");
        return;
      }
      const keys = vscode?.compare_keys ?? [];
      expect(keys.length, "the case names the keys to compare").toBeGreaterThan(0);
      const lsp = parityCase.inputs.lsp?.initializationOptions;
      expect(lsp, "the case has an lsp input to compare with").toBeDefined();

      settings.values = settingValues(vscode ?? {});
      const sent = createInitializationOptions() as unknown as Record<string, unknown>;

      expect(projection(sent, keys)).toEqual(projection(lsp ?? {}, keys));
    },
  );
});
