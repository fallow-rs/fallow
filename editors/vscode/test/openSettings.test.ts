import * as fs from "node:fs";
import * as path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openSettingsSearch } from "../src/openSettings.js";

const mocks = vi.hoisted(() => ({
  executeCommand: vi.fn<(command: string, ...args: unknown[]) => Promise<unknown>>(),
  showWarningMessage: vi.fn<(message: string) => Promise<undefined>>(),
}));

vi.mock("vscode", () => ({
  commands: { executeCommand: mocks.executeCommand },
  window: { showWarningMessage: mocks.showWarningMessage },
}));

beforeEach(() => {
  mocks.executeCommand.mockReset().mockResolvedValue(undefined);
  mocks.showWarningMessage.mockReset().mockResolvedValue(undefined);
});

describe("openSettingsSearch", () => {
  it("opens the Settings editor with the query through runCommands", async () => {
    await openSettingsSearch("fallow.lspPath");

    expect(mocks.executeCommand).toHaveBeenCalledTimes(1);
    expect(mocks.executeCommand).toHaveBeenCalledWith("runCommands", {
      commands: [{ command: "workbench.action.openSettings", args: "fallow.lspPath" }],
    });
  });

  it("resolves when the command fails", async () => {
    mocks.executeCommand.mockRejectedValue(new Error("command not found"));

    await expect(openSettingsSearch("fallow")).resolves.toBeUndefined();
  });

  it("names the query in a warning when the command fails", async () => {
    mocks.executeCommand.mockRejectedValue(new Error("command not found"));

    await openSettingsSearch("fallow.changedSince");

    expect(mocks.showWarningMessage).toHaveBeenCalledTimes(1);
    expect(mocks.showWarningMessage.mock.calls[0]?.[0]).toContain("fallow.changedSince");
  });

  it("shows no warning when the command succeeds", async () => {
    await openSettingsSearch("fallow");

    expect(mocks.showWarningMessage).not.toHaveBeenCalled();
  });
});

// In Cursor, `workbench.action.openSettings` with a query can freeze the
// Settings editor when the extension host gets the Settings editor pane as
// the command result (#4553, #4595). Only `src/openSettings.ts` may call it.
describe("workbench.action.openSettings callers", () => {
  const srcDir = path.resolve(__dirname, "../src");
  const direct = /executeCommand\(\s*["']workbench\.action\.openSettings["']/;

  const sourceFiles = (dir: string): string[] =>
    fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        return sourceFiles(full);
      }
      return entry.name.endsWith(".ts") ? [full] : [];
    });

  it("has no direct executeCommand call outside openSettings.ts", () => {
    const offenders = sourceFiles(srcDir)
      .filter((file) => path.basename(file) !== "openSettings.ts")
      .filter((file) => direct.test(fs.readFileSync(file, "utf8")))
      .map((file) => path.relative(srcDir, file));

    expect(offenders).toEqual([]);
  });
});
