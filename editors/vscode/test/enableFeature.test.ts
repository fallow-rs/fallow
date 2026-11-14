import { beforeEach, describe, expect, it, vi } from "vitest";
import { enableFeature } from "../src/enableFeature.js";

const mocks = vi.hoisted(() => ({
  update: vi.fn<(key: string, value: unknown, target: number) => Promise<void>>(),
  executeCommand: vi.fn<(command: string, ...args: unknown[]) => Promise<unknown>>(),
  showInformationMessage: vi.fn<(message: string) => Promise<undefined>>(),
  showErrorMessage: vi.fn<(message: string) => Promise<undefined>>(),
  workspaceFolders: undefined as readonly object[] | undefined,
}));

vi.mock("vscode", () => ({
  ConfigurationTarget: { Global: 1, Workspace: 2, WorkspaceFolder: 3 },
  workspace: {
    get workspaceFolders() {
      return mocks.workspaceFolders;
    },
    getConfiguration: (section: string) => {
      expect(section).toBe("fallow");
      return { update: mocks.update };
    },
  },
  commands: { executeCommand: mocks.executeCommand },
  window: {
    showInformationMessage: mocks.showInformationMessage,
    showErrorMessage: mocks.showErrorMessage,
  },
}));

beforeEach(() => {
  mocks.update.mockReset().mockResolvedValue(undefined);
  mocks.executeCommand.mockReset();
  mocks.showInformationMessage.mockReset().mockResolvedValue(undefined);
  mocks.showErrorMessage.mockReset().mockResolvedValue(undefined);
  mocks.workspaceFolders = [{}];
});

describe("enableFeature", () => {
  it("writes the toggle to workspace settings when a workspace is open", async () => {
    await enableFeature("security.enabled");

    expect(mocks.update).toHaveBeenCalledWith("security.enabled", true, 2);
    expect(mocks.showInformationMessage).toHaveBeenCalledWith(
      expect.stringContaining("fallow.security.enabled in workspace settings"),
    );
  });

  it("writes the toggle to user settings when no workspace is open", async () => {
    mocks.workspaceFolders = undefined;

    await enableFeature("health.enabled");

    expect(mocks.update).toHaveBeenCalledWith("health.enabled", true, 1);
    expect(mocks.showInformationMessage).toHaveBeenCalledWith(
      expect.stringContaining("fallow.health.enabled in user settings"),
    );
  });

  it("never opens the Settings editor", async () => {
    await enableFeature("security.enabled");

    expect(mocks.executeCommand).not.toHaveBeenCalled();
  });

  it("names the manual setting when the settings write fails", async () => {
    mocks.update.mockRejectedValue(new Error("Unable to write into workspace settings."));

    await expect(enableFeature("security.enabled")).resolves.toBeUndefined();

    expect(mocks.showInformationMessage).not.toHaveBeenCalled();
    expect(mocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining('Set "fallow.security.enabled": true in your workspace settings.json'),
    );
    expect(mocks.showErrorMessage).toHaveBeenCalledWith(
      expect.stringContaining("Unable to write into workspace settings."),
    );
  });
});
