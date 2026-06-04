import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type * as vscode from "vscode";
import { beforeEach, describe, expect, it, vi } from "vitest";

let mockFiles: ReadonlySet<string> = new Set();
let mockLspPath = "";
let mockAutoDownload = true;
let mockLocalBinary: string | null = null;
let mockPathBinary: string | null = null;
let mockInstalledCli: string | null = null;
let mockDownloadedCli: string | null = null;
let mockExtensionVersion: string | null = null;
let mockBinaryVersions: Readonly<Record<string, string | null>> = {};

vi.mock("node:fs", () => ({
  existsSync: (p: string) => mockFiles.has(p),
}));

vi.mock("vscode", () => ({
  QuickPickItemKind: {
    Separator: -1,
  },
  window: {
    showWarningMessage: vi.fn(),
    showInformationMessage: vi.fn(),
    showErrorMessage: vi.fn(),
    showQuickPick: vi.fn(),
    showTextDocument: vi.fn(),
  },
  workspace: {
    workspaceFolders: undefined,
  },
  commands: {
    executeCommand: vi.fn(),
  },
  Uri: {
    file: (fsPath: string) => ({ fsPath }),
  },
  Range: class {
    constructor(
      readonly startLine: number,
      readonly startCharacter: number,
      readonly endLine: number,
      readonly endCharacter: number,
    ) {}
  },
}));

vi.mock("../src/config.js", () => ({
  getLspPath: () => mockLspPath,
  getAutoDownload: () => mockAutoDownload,
  getProduction: () => false,
  getAuditGate: () => "new-only",
  getDuplicationCrossLanguageOverride: () => undefined,
  getDuplicationIgnoreImportsOverride: () => undefined,
  getDuplicationMinLinesOverride: () => undefined,
  getDuplicationMinOccurrencesOverride: () => undefined,
  getDuplicationMinTokensOverride: () => undefined,
  getDuplicationModeOverride: () => undefined,
  getDuplicationSkipLocalOverride: () => undefined,
  getDuplicationThresholdOverride: () => undefined,
  getHealthHotspots: () => true,
  getHealthTopFindings: () => 20,
  getIssueTypes: () => ({}),
  getChangedSince: () => "",
  getResolvedConfigPath: () => "",
}));

vi.mock("../src/binary-utils.js", () => ({
  getExecutableExtension: () => "",
  findLocalBinary: (name: string) => (name === "fallow" ? mockLocalBinary : null),
  findBinaryInPath: (name: string) => (name === "fallow" ? mockPathBinary : null),
}));

vi.mock("../src/download.js", () => ({
  getInstalledCliPath: vi.fn(() => mockInstalledCli),
  downloadCliBinary: vi.fn(async () => mockDownloadedCli),
  getBinaryVersion: (binaryPath: string) => mockBinaryVersions[binaryPath] ?? null,
  getExtensionVersion: () => mockExtensionVersion,
}));

import { window as mockWindow } from "vscode";
import { downloadCliBinary, getInstalledCliPath } from "../src/download.js";
import {
  execFallow,
  FallowExecError,
  findCliBinary,
  resolveCliBinary,
  resolveCliForRun,
  runHealthAnalysis,
  resetHealthNoWorkspaceWarning,
} from "../src/commands.js";

const context = {} as unknown as vscode.ExtensionContext;

describe("execFallow", () => {
  it("preserves structured stdout on nonzero coverage gate exits", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-exec-"));
    const structuredError = {
      error: true,
      message: "license missing",
      exit_code: 3,
    };

    try {
      const script = join(dir, "gate-error.mjs");
      await writeFile(
        script,
        [
          `process.stdout.write(${JSON.stringify(JSON.stringify(structuredError))});`,
          'process.stderr.write("license gate failed\\n");',
          "process.exit(3);",
        ].join("\n"),
        "utf8",
      );

      let caught: unknown = null;
      try {
        await execFallow(process.execPath, [script], dir);
      } catch (err) {
        caught = err;
      }

      expect(caught).toBeInstanceOf(FallowExecError);
      const error = caught as FallowExecError;
      expect(error.exitCode).toBe(3);
      expect(error.stdout).toBe(JSON.stringify(structuredError));
      expect(error.message).toBe("license gate failed");
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("findCliBinary", () => {
  beforeEach(() => {
    mockFiles = new Set();
    mockLspPath = "";
    mockAutoDownload = true;
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    vi.clearAllMocks();
  });

  it("uses the CLI sibling of a configured LSP path first", () => {
    mockLspPath = "/tools/fallow-lsp";
    mockFiles = new Set(["/tools/fallow"]);
    mockLocalBinary = "/workspace/node_modules/.bin/fallow";
    mockPathBinary = "/usr/local/bin/fallow";
    mockInstalledCli = "/storage/bin/fallow";

    expect(findCliBinary(context)).toBe("/tools/fallow");
  });

  it("prefers the workspace CLI before PATH and managed storage", () => {
    mockLocalBinary = "/workspace/node_modules/.bin/fallow";
    mockPathBinary = "/usr/local/bin/fallow";
    mockInstalledCli = "/storage/bin/fallow";

    expect(findCliBinary(context)).toBe("/workspace/node_modules/.bin/fallow");
  });

  it("uses the managed CLI after configured, workspace, and PATH lookups miss", () => {
    mockInstalledCli = "/storage/bin/fallow";

    expect(findCliBinary(context)).toBe("/storage/bin/fallow");
  });
});

describe("resolveCliBinary", () => {
  beforeEach(() => {
    mockFiles = new Set();
    mockLspPath = "";
    mockAutoDownload = true;
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    vi.clearAllMocks();
  });

  it("downloads the managed CLI when every higher-priority location misses", async () => {
    mockDownloadedCli = "/storage/bin/fallow";

    await expect(resolveCliBinary(context)).resolves.toBe("/storage/bin/fallow");
    expect(downloadCliBinary).toHaveBeenCalledWith(context);
  });

  it("does not download the CLI when auto-download is disabled", async () => {
    mockAutoDownload = false;
    mockDownloadedCli = "/storage/bin/fallow";

    await expect(resolveCliBinary(context)).resolves.toBeNull();
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });
});

describe("resolveCliForRun", () => {
  beforeEach(() => {
    mockFiles = new Set();
    mockLspPath = "";
    mockAutoDownload = true;
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = "2.88.1";
    mockBinaryVersions = {};
    vi.clearAllMocks();
  });

  it("uses a resolved CLI at the extension version as-is, without downloading", async () => {
    mockPathBinary = "/usr/local/bin/ok-fallow";
    mockBinaryVersions = { "/usr/local/bin/ok-fallow": "2.88.1" };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/usr/local/bin/ok-fallow",
      version: "2.88.1",
    });
    expect(getInstalledCliPath).not.toHaveBeenCalled();
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });

  it("uses a newer resolved CLI as-is (never downgrades)", async () => {
    mockPathBinary = "/usr/local/bin/newer-fallow";
    mockBinaryVersions = { "/usr/local/bin/newer-fallow": "2.99.0" };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/usr/local/bin/newer-fallow",
      version: "2.99.0",
    });
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });

  it("switches a stale PATH CLI to the already-installed managed binary (no network)", async () => {
    mockPathBinary = "/usr/local/bin/old-fallow";
    mockInstalledCli = "/storage/bin/fallow";
    mockBinaryVersions = {
      "/usr/local/bin/old-fallow": "2.86.0",
      "/storage/bin/fallow": "2.88.1",
    };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/storage/bin/fallow",
      version: "2.88.1",
    });
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });

  it("downloads the managed binary once when a stale PATH CLI has no managed copy yet", async () => {
    mockPathBinary = "/usr/local/bin/stale-fallow";
    mockInstalledCli = null;
    mockDownloadedCli = "/storage/bin/fallow";
    mockBinaryVersions = {
      "/usr/local/bin/stale-fallow": "2.86.0",
      "/storage/bin/fallow": "2.88.1",
    };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/storage/bin/fallow",
      version: "2.88.1",
    });
    expect(downloadCliBinary).toHaveBeenCalledWith(context);
  });

  it("keeps a stale CLI (degraded) when auto-download is disabled", async () => {
    mockAutoDownload = false;
    mockPathBinary = "/usr/local/bin/pinned-fallow";
    mockBinaryVersions = { "/usr/local/bin/pinned-fallow": "2.86.0" };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/usr/local/bin/pinned-fallow",
      version: "2.86.0",
    });
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });

  it("does not force an upgrade when the resolved CLI version is unknown", async () => {
    mockPathBinary = "/usr/local/bin/unknown-fallow";
    mockBinaryVersions = { "/usr/local/bin/unknown-fallow": null };

    await expect(resolveCliForRun(context)).resolves.toEqual({
      binary: "/usr/local/bin/unknown-fallow",
      version: null,
    });
    expect(downloadCliBinary).not.toHaveBeenCalled();
  });
});

describe("runHealthAnalysis no-workspace gate (#902)", () => {
  beforeEach(() => {
    resetHealthNoWorkspaceWarning();
    vi.clearAllMocks();
  });

  it("returns null and warns exactly once across repeated reveals with no workspace folder", async () => {
    // The mocked vscode.workspace.workspaceFolders is undefined, so every call
    // hits the no-workspace path. The Health view re-spawns on every reveal
    // until it latches, so the warning must not repeat on each re-reveal.
    await expect(runHealthAnalysis(context)).resolves.toBeNull();
    await expect(runHealthAnalysis(context)).resolves.toBeNull();
    await expect(runHealthAnalysis(context)).resolves.toBeNull();

    expect(mockWindow.showWarningMessage).toHaveBeenCalledTimes(1);
    expect(mockWindow.showWarningMessage).toHaveBeenCalledWith("Fallow: no workspace folder open.");
  });

  it("warns again after the once-per-session gate is reset (reactivation)", async () => {
    await runHealthAnalysis(context);
    expect(mockWindow.showWarningMessage).toHaveBeenCalledTimes(1);

    resetHealthNoWorkspaceWarning();
    await runHealthAnalysis(context);
    expect(mockWindow.showWarningMessage).toHaveBeenCalledTimes(2);
  });
});
