import { beforeEach, describe, expect, it, vi } from "vitest";

const vscodeMocks = vi.hoisted(() => ({
  writeText: vi.fn(async () => undefined),
  setStatusBarMessage: vi.fn(),
}));

vi.mock("vscode", () => ({
  CodeActionKind: {
    QuickFix: {
      append: (value: string) => `quickfix.${value}`,
    },
  },
  CodeAction: class {
    public command: unknown;
    public diagnostics: unknown;
    public constructor(
      public readonly title: string,
      public readonly kind: string,
    ) {}
  },
  DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
  env: { clipboard: { writeText: vscodeMocks.writeText } },
  window: { setStatusBarMessage: vscodeMocks.setStatusBarMessage },
  commands: { registerCommand: vi.fn() },
  languages: { registerCodeActionsProvider: vi.fn() },
}));

import type * as vscode from "vscode";
import { __testHelpers, findingIdOf } from "../src/findingId.js";

const ID = "dc1:unused-export:0123456789abcdef";

const diagnostic = (source: string, data: unknown): vscode.Diagnostic =>
  ({ source, message: "Export 'x' is unused", code: "unused-export", data }) as never;

const actionsFor = (diagnostics: vscode.Diagnostic[]): vscode.CodeAction[] =>
  new __testHelpers.FallowFindingIdCodeActions().provideCodeActions(
    {} as vscode.TextDocument,
    {} as vscode.Range,
    { diagnostics } as never,
  );

describe("findingIdOf", () => {
  it("reads data.findingId from a fallow diagnostic", () => {
    expect(findingIdOf(diagnostic("fallow", { findingId: ID, changedSince: "main" }))).toBe(ID);
  });

  it("returns null without a usable id", () => {
    expect(findingIdOf(diagnostic("fallow", undefined))).toBeNull();
    expect(findingIdOf(diagnostic("fallow", "token"))).toBeNull();
    expect(findingIdOf(diagnostic("fallow", { findingId: 7 }))).toBeNull();
    expect(findingIdOf(diagnostic("fallow", { findingId: "" }))).toBeNull();
    expect(findingIdOf(diagnostic("eslint", { findingId: ID }))).toBeNull();
  });
});

describe("copy finding id code action", () => {
  it("offers one action per distinct finding id", () => {
    const first = diagnostic("fallow", { findingId: ID });
    const actions = actionsFor([
      first,
      diagnostic("fallow", { findingId: ID }),
      diagnostic("fallow", { security: {} }),
      diagnostic("eslint", { findingId: "dc1:unused-file:fedcba9876543210" }),
    ]);
    expect(actions).toHaveLength(1);
    expect(actions[0].title).toBe(`Copy Fallow finding id (${ID})`);
    expect(actions[0].kind).toBe("quickfix.fallow.copyFindingId");
    expect(actions[0].command).toEqual({
      command: "fallow.copyFindingId",
      title: "Copy Fallow finding id",
      arguments: [ID],
    });
    expect(actions[0].diagnostics).toEqual([first]);
  });
});

describe("copy finding id command", () => {
  beforeEach(() => {
    vscodeMocks.writeText.mockClear();
    vscodeMocks.setStatusBarMessage.mockClear();
  });

  it("writes the id to the clipboard", async () => {
    await __testHelpers.copyFindingId(ID);
    expect(vscodeMocks.writeText).toHaveBeenCalledWith(ID);
    expect(vscodeMocks.setStatusBarMessage).toHaveBeenCalledOnce();
  });

  it("ignores a missing or non-string argument", async () => {
    await __testHelpers.copyFindingId(undefined);
    await __testHelpers.copyFindingId(42);
    expect(vscodeMocks.writeText).not.toHaveBeenCalled();
  });
});
