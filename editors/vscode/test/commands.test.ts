import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
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
    showErrorMessage: vi.fn(async () => undefined),
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
  getProductionOverride: () => undefined,
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
  getWorkspaceScope: () => "",
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

import { window as mockWindow, workspace as mockWorkspace } from "vscode";
import { downloadCliBinary, getInstalledCliPath } from "../src/download.js";
import {
  execFallow,
  FallowExecError,
  findCliBinary,
  resolveCliBinary,
  resolveCliForRun,
  runAnalysis,
  runHealthAnalysis,
  resetHealthNoWorkspaceWarning,
} from "../src/commands.js";
import { AnalysisFailureBackoff } from "../src/analysisBackoff.js";

const context = {} as unknown as vscode.ExtensionContext;
const workspaceContext = {
  workspaceState: {
    get: () => "",
  },
} as unknown as vscode.ExtensionContext;

const emptyCheck = {
  schema_version: 7,
  version: "0.0.0-test",
  elapsed_ms: 0,
  total_issues: 0,
  unused_files: [],
  unused_exports: [],
  unused_types: [],
  private_type_leaks: [],
  unused_dependencies: [],
  unused_dev_dependencies: [],
  unused_optional_dependencies: [],
  unused_enum_members: [],
  unused_class_members: [],
  unresolved_imports: [],
  unlisted_dependencies: [],
  duplicate_exports: [],
  type_only_dependencies: [],
  test_only_dependencies: [],
  circular_dependencies: [],
  re_export_cycles: [],
  boundary_violations: [],
  stale_suppressions: [],
  unused_catalog_entries: [],
  empty_catalog_groups: [],
  unresolved_catalog_references: [],
  unused_dependency_overrides: [],
  misconfigured_dependency_overrides: [],
  summary: {
    total_issues: 0,
    unused_files: 0,
    unused_exports: 0,
    unused_types: 0,
    private_type_leaks: 0,
    unused_dependencies: 0,
    unused_enum_members: 0,
    unused_class_members: 0,
    unresolved_imports: 0,
    unlisted_dependencies: 0,
    duplicate_exports: 0,
    type_only_dependencies: 0,
    test_only_dependencies: 0,
    circular_dependencies: 0,
    re_export_cycles: 0,
    boundary_violations: 0,
    stale_suppressions: 0,
    unused_catalog_entries: 0,
    empty_catalog_groups: 0,
    unresolved_catalog_references: 0,
    unused_dependency_overrides: 0,
    misconfigured_dependency_overrides: 0,
  },
};

const emptyDupes = {
  clone_groups: [],
  clone_families: [],
  stats: {
    total_files: 1,
    files_with_clones: 0,
    total_lines: 1,
    duplicated_lines: 0,
    total_tokens: 1,
    duplicated_tokens: 0,
    clone_groups: 0,
    clone_instances: 0,
    duplication_percentage: 0,
    clone_groups_below_min_occurrences: 0,
  },
};

const setWorkspaceRoot = (root: string | null): void => {
  const workspace = mockWorkspace as {
    workspaceFolders: ReadonlyArray<{ readonly uri: { readonly fsPath: string } }> | undefined;
  };
  workspace.workspaceFolders = root === null ? undefined : [{ uri: { fsPath: root } }];
};

const restoreMaxFileSizeEnv = (value: string | undefined): void => {
  if (value === undefined) {
    delete process.env.FALLOW_MAX_FILE_SIZE;
    return;
  }
  process.env.FALLOW_MAX_FILE_SIZE = value;
};

const readSpawnLog = async (
  logPath: string,
): Promise<Array<{ readonly env: string | undefined; readonly args: readonly string[] }>> => {
  const raw = await readFile(logPath, "utf8");
  return raw
    .trim()
    .split("\n")
    .filter((line) => line.length > 0)
    .map(
      (line) =>
        JSON.parse(line) as {
          readonly env: string | undefined;
          readonly args: readonly string[];
        },
    );
};

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

describe("runAnalysis retry backoff", () => {
  beforeEach(() => {
    mockFiles = new Set();
    mockLspPath = "";
    mockAutoDownload = true;
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = null;
    mockBinaryVersions = {};
    setWorkspaceRoot(null);
    vi.clearAllMocks();
  });

  it("runs analysis with the default max-file-size ceiling", async () => {
    const originalLimit = process.env.FALLOW_MAX_FILE_SIZE;
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-analysis-env-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");
    const output = JSON.stringify({ check: emptyCheck, dupes: emptyDupes });

    try {
      delete process.env.FALLOW_MAX_FILE_SIZE;
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ env: process.env.FALLOW_MAX_FILE_SIZE, args: process.argv.slice(2) }) + "\\n");`,
          `process.stdout.write(${JSON.stringify(output)});`,
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);

      const result = await runAnalysis(workspaceContext, undefined, {
        backoff: new AnalysisFailureBackoff(),
      });
      const calls = await readSpawnLog(logPath);

      expect(result.check).not.toBeNull();
      expect(calls).toHaveLength(1);
      expect(calls[0]?.env).toBe("5");
      expect(calls[0]?.args).toEqual(["--format", "json", "--quiet", "--skip", "health"]);
    } finally {
      restoreMaxFileSizeEnv(originalLimit);
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("stops automatic reruns after repeated analysis failures", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-analysis-backoff-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");
    const backoff = new AnalysisFailureBackoff();

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ env: process.env.FALLOW_MAX_FILE_SIZE, args: process.argv.slice(2) }) + "\\n");`,
          'process.stderr.write("boom\\n");',
          "process.exit(2);",
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);

      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow(
        "automatic analysis is paused",
      );

      let calls = await readSpawnLog(logPath);
      expect(calls).toHaveLength(3);
      expect(mockWindow.showErrorMessage).toHaveBeenCalledWith(
        expect.stringContaining("Fallow analysis paused after 3 failed attempts"),
        "Retry now",
      );

      await expect(
        runAnalysis(workspaceContext, undefined, { backoff, force: true }),
      ).rejects.toThrow("boom");
      calls = await readSpawnLog(logPath);
      expect(calls).toHaveLength(4);
    } finally {
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("clears previous failures after a successful empty analysis run", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-analysis-reset-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");
    const modePath = join(dir, "mode.txt");
    const backoff = new AnalysisFailureBackoff();

    try {
      await writeFile(modePath, "fail", "utf8");
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ env: process.env.FALLOW_MAX_FILE_SIZE, args: process.argv.slice(2) }) + "\\n");`,
          `if (fs.readFileSync(${JSON.stringify(modePath)}, "utf8").trim() === "fail") {`,
          '  process.stderr.write("boom\\n");',
          "  process.exit(2);",
          "}",
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);

      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");

      await writeFile(modePath, "empty", "utf8");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).resolves.toEqual({
        check: null,
        dupes: null,
      });

      await writeFile(modePath, "fail", "utf8");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow("boom");
      await expect(runAnalysis(workspaceContext, undefined, { backoff })).rejects.toThrow(
        "automatic analysis is paused",
      );

      const calls = await readSpawnLog(logPath);
      expect(calls).toHaveLength(6);
    } finally {
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("runHealthAnalysis no-workspace gate (#902)", () => {
  beforeEach(() => {
    setWorkspaceRoot(null);
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
