import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BASELINE_TAG,
  runBaselineCommand,
  runGitCommand,
  type BaselineCommandHost,
  type GitCommandResult,
  type GitRunner,
} from "../src/baselineCommand.js";

interface MutableHost {
  roots: string[];
  currentChangedSince: string;
  confirmResult: boolean;
  warningActionResult: boolean;
  updateError: Error | null;
  refreshError: Error | null;
}

const hostState: MutableHost = {
  roots: ["/repo"],
  currentChangedSince: "",
  confirmResult: true,
  warningActionResult: false,
  updateError: null,
  refreshError: null,
};

const confirm = vi.fn(async () => hostState.confirmResult);
const showWarning = vi.fn(async () => hostState.warningActionResult);
const showError = vi.fn(async () => undefined);
const showInformation = vi.fn(async () => undefined);
const openChangedSinceSetting = vi.fn(async () => undefined);
const updateChangedSince = vi.fn(async () => {
  if (hostState.updateError) {
    throw hostState.updateError;
  }
  hostState.currentChangedSince = BASELINE_TAG;
});
const refreshAnalysis = vi.fn(async () => {
  if (hostState.refreshError) {
    throw hostState.refreshError;
  }
});
const appendOutput = vi.fn();

const host: BaselineCommandHost = {
  workspaceRoots: () => hostState.roots,
  changedSince: () => hostState.currentChangedSince,
  confirm,
  showWarning,
  showError,
  showInformation,
  openChangedSinceSetting,
  updateChangedSince,
  refreshAnalysis,
  appendOutput,
};

const result = (code: number, stdout = "", stderr = ""): GitCommandResult => ({
  code,
  stdout,
  stderr,
});

const gitRunner = (responses: ReadonlyArray<GitCommandResult>): GitRunner => {
  let index = 0;
  return vi.fn(async () => {
    const response = responses[index];
    index += 1;
    if (!response) {
      throw new Error(`unexpected git call ${index}`);
    }
    return response;
  });
};

const missingTag = (): GitRunner =>
  gitRunner([result(0, "/repo\n"), result(0, "head-sha\n"), result(1), result(0)]);

const tagAtHead = (): GitRunner =>
  gitRunner([result(0, "/repo\n"), result(0, "head-sha\n"), result(0), result(0, "head-sha\n")]);

describe("runBaselineCommand", () => {
  beforeEach(() => {
    hostState.roots = ["/repo"];
    hostState.currentChangedSince = "";
    hostState.confirmResult = true;
    hostState.warningActionResult = false;
    hostState.updateError = null;
    hostState.refreshError = null;
    vi.clearAllMocks();
  });

  it("refuses to run without a workspace", async () => {
    hostState.roots = [];

    await expect(runBaselineCommand(host, gitRunner([]))).resolves.toBe("no-workspace");

    expect(showWarning).toHaveBeenCalledWith(
      "Fallow: open a workspace folder before setting a baseline.",
    );
    expect(confirm).not.toHaveBeenCalled();
  });

  it("refuses multi-root workspaces without mutation", async () => {
    hostState.roots = ["/repo-a", "/repo-b"];

    await expect(runBaselineCommand(host, gitRunner([]))).resolves.toBe("multi-root");

    expect(confirm).not.toHaveBeenCalled();
    expect(updateChangedSince).not.toHaveBeenCalled();
  });

  it("reports a non-Git workspace", async () => {
    const runGit = gitRunner([result(128, "", "not a git repository")]);

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("not-repository");

    expect(appendOutput).toHaveBeenCalledWith("Set Baseline at HEAD: not a git repository");
  });

  it("reports a repository without a commit", async () => {
    const runGit = gitRunner([result(0, "/repo\n"), result(128, "", "unknown revision")]);

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("unborn-head");

    expect(updateChangedSince).not.toHaveBeenCalled();
  });

  it("creates the tag, persists the setting, and refreshes", async () => {
    const runGit = missingTag();

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("configured");

    expect(runGit).toHaveBeenLastCalledWith(["tag", "--no-sign", BASELINE_TAG, "HEAD"], "/repo");
    expect(updateChangedSince).toHaveBeenCalledWith(BASELINE_TAG);
    expect(refreshAnalysis).toHaveBeenCalledOnce();
    expect(showInformation).toHaveBeenCalledWith(
      `Fallow: baseline set at HEAD. New findings are scoped from \`${BASELINE_TAG}\`.`,
    );
  });

  it("does not mutate after cancellation", async () => {
    hostState.confirmResult = false;
    const runGit = missingTag();

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("cancelled");

    expect(runGit).toHaveBeenCalledTimes(3);
    expect(updateChangedSince).not.toHaveBeenCalled();
  });

  it("names the existing setting before replacing it", async () => {
    hostState.currentChangedSince = "main";
    const runGit = missingTag();

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("configured");

    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining("This replaces the current effective value `main`."),
    );
    expect(confirm).toHaveBeenCalledWith(
      expect.stringContaining("This does not push anything to a remote."),
    );
  });

  it("treats a tag already at HEAD as idempotent", async () => {
    hostState.currentChangedSince = BASELINE_TAG;
    const runGit = tagAtHead();

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("already-configured");

    expect(confirm).not.toHaveBeenCalled();
    expect(updateChangedSince).not.toHaveBeenCalled();
    expect(refreshAnalysis).toHaveBeenCalledOnce();
  });

  it("updates only the setting when the tag already points to HEAD", async () => {
    hostState.currentChangedSince = "main";
    const runGit = tagAtHead();

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("configured");

    expect(runGit).toHaveBeenCalledTimes(4);
    expect(updateChangedSince).toHaveBeenCalledWith(BASELINE_TAG);
  });

  it("refuses to move an existing tag and can open the setting", async () => {
    hostState.warningActionResult = true;
    const runGit = gitRunner([
      result(0, "/repo\n"),
      result(0, "head-sha\n"),
      result(0),
      result(0, "other-sha\n"),
    ]);

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("tag-conflict");

    expect(confirm).not.toHaveBeenCalled();
    expect(openChangedSinceSetting).toHaveBeenCalledOnce();
    expect(updateChangedSince).not.toHaveBeenCalled();
  });

  it("reports Git failures without changing settings", async () => {
    const runGit = gitRunner([
      result(0, "/repo\n"),
      result(0, "head-sha\n"),
      result(2, "", "corrupt ref"),
    ]);

    await expect(runBaselineCommand(host, runGit)).resolves.toBe("git-error");

    expect(showError).toHaveBeenCalled();
    expect(updateChangedSince).not.toHaveBeenCalled();
  });

  it("reports partial state when settings persistence fails", async () => {
    hostState.updateError = new Error("settings are read-only");

    await expect(runBaselineCommand(host, missingTag())).resolves.toBe("settings-error");

    expect(showError).toHaveBeenCalledWith(
      expect.stringContaining(`The local tag \`${BASELINE_TAG}\` was created`),
    );
    expect(refreshAnalysis).not.toHaveBeenCalled();
  });

  it("reports a refresh failure after configuring the baseline", async () => {
    hostState.refreshError = new Error("analysis failed");

    await expect(runBaselineCommand(host, missingTag())).resolves.toBe("refresh-error");

    expect(updateChangedSince).toHaveBeenCalledWith(BASELINE_TAG);
    expect(showError).toHaveBeenCalledWith(expect.stringContaining("baseline is configured"));
  });
});

describe("runGitCommand", () => {
  let root = "";

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "fallow-vscode-baseline-"));
    expect((await runGitCommand(["init"], root)).code).toBe(0);
    expect((await runGitCommand(["config", "user.email", "test@example.com"], root)).code).toBe(0);
    expect((await runGitCommand(["config", "user.name", "Fallow Test"], root)).code).toBe(0);
    await writeFile(join(root, "index.ts"), "export const value = 1;\n");
    expect((await runGitCommand(["add", "index.ts"], root)).code).toBe(0);
    expect(
      (await runGitCommand(["-c", "commit.gpgsign=false", "commit", "-m", "initial"], root)).code,
    ).toBe(0);
    expect((await runGitCommand(["config", "tag.gpgSign", "true"], root)).code).toBe(0);
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("runs the command end to end and creates a lightweight tag at HEAD", async () => {
    const realHost: BaselineCommandHost = {
      ...host,
      workspaceRoots: () => [root],
      changedSince: () => "",
      confirm: async () => true,
      updateChangedSince: async () => undefined,
      refreshAnalysis: async () => undefined,
    };

    await expect(runBaselineCommand(realHost, runGitCommand)).resolves.toBe("configured");

    const tag = await runGitCommand(["rev-parse", `refs/tags/${BASELINE_TAG}`], root);
    const head = await runGitCommand(["rev-parse", "HEAD"], root);

    expect(tag.code).toBe(0);
    expect(tag.stdout.trim()).toBe(head.stdout.trim());
  });
});
