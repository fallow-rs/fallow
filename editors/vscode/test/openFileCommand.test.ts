import { beforeEach, describe, expect, it, vi } from "vitest";

const vscodeMocks = vi.hoisted(() => ({
  showTextDocument: vi.fn(),
  uriFile: vi.fn((fsPath: string) => ({ fsPath })),
}));

vi.mock("vscode", () => {
  class FakeRange {
    public constructor(
      public readonly startLine: number,
      public readonly startCharacter: number,
      public readonly endLine: number,
      public readonly endCharacter: number,
    ) {}
  }

  return {
    Range: FakeRange,
    Uri: {
      file: vscodeMocks.uriFile,
    },
    window: {
      showTextDocument: vscodeMocks.showTextDocument,
    },
  };
});

import {
  OPEN_FILE_COMMAND,
  openFileCommand,
  openFileCommandHandler,
  type OpenFileCommandArgs,
} from "../src/openFileCommand.js";

describe("openFileCommand", () => {
  beforeEach(() => {
    vscodeMocks.showTextDocument.mockReset();
    vscodeMocks.uriFile.mockClear();
  });

  it("stores decoded bracketed paths as command data", () => {
    const command = openFileCommand("C:\\repo\\src\\app\\[productId]\\page.tsx", 9, 3);
    const args = command.arguments?.[0] as OpenFileCommandArgs | undefined;

    expect(command.command).toBe(OPEN_FILE_COMMAND);
    expect(args?.absolutePath).toBe("C:\\repo\\src\\app\\[productId]\\page.tsx");
    expect(args?.absolutePath).not.toContain("%5B");
    expect(args?.absolutePath).not.toContain("%5D");
    expect(args).toMatchObject({ line: 9, col: 3, endLine: 9, endCol: 3 });
  });

  it("opens decoded filesystem paths with the requested selection", async () => {
    const absolutePath = "C:\\repo\\src\\app\\[productId]\\page.tsx";

    await openFileCommandHandler({ absolutePath, line: 9, col: 3 });

    expect(vscodeMocks.uriFile).toHaveBeenCalledWith(absolutePath);
    expect(vscodeMocks.showTextDocument).toHaveBeenCalledWith(
      { fsPath: absolutePath },
      {
        selection: {
          startLine: 8,
          startCharacter: 3,
          endLine: 8,
          endCharacter: 3,
        },
      },
    );
  });

  it("ignores malformed command payloads", async () => {
    await openFileCommandHandler({ absolutePath: "", line: 1, col: 0 });

    expect(vscodeMocks.uriFile).not.toHaveBeenCalled();
    expect(vscodeMocks.showTextDocument).not.toHaveBeenCalled();
  });
});
