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
let mockConfigPathSetting = "";
let mockResolvedConfigRoots: string[] = [];
let mockComplexityBreakdownEnabled = false;
let mockTypeAwareSettings: {
  enabled: boolean;
  projects: readonly string[];
  require: "best-effort" | "complete";
} = {
  enabled: false,
  projects: [],
  require: "best-effort",
};
let mockActiveTextEditor:
  | {
      readonly document: {
        readonly uri: {
          readonly scheme: string;
          readonly fsPath: string;
        };
        readonly isDirty: boolean;
        readonly save: () => Promise<boolean>;
        readonly getWordRangeAtPosition: () => object | undefined;
        readonly getText: () => string;
      };
      readonly selection: { readonly active: object };
    }
  | undefined;

vi.mock("node:fs", () => ({
  existsSync: (p: string) => mockFiles.has(p),
}));

vi.mock("vscode", () => ({
  QuickPickItemKind: {
    Separator: -1,
  },
  window: {
    get activeTextEditor() {
      return mockActiveTextEditor;
    },
    showWarningMessage: vi.fn(),
    showInformationMessage: vi.fn(),
    showErrorMessage: vi.fn(async () => undefined),
    showQuickPick: vi.fn(),
    showTextDocument: vi.fn(),
  },
  workspace: {
    workspaceFolders: undefined,
    getWorkspaceFolder: vi.fn((uri: { readonly fsPath: string }) => {
      const workspace = mockWorkspace as {
        readonly workspaceFolders:
          | ReadonlyArray<{ readonly uri: { readonly fsPath: string } }>
          | undefined;
      };
      return (
        workspace.workspaceFolders?.find((folder) => uri.fsPath.startsWith(folder.uri.fsPath)) ??
        undefined
      );
    }),
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
  getTypeAwareSettings: () => mockTypeAwareSettings,
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
  getComplexityBreakdownEnabled: () => mockComplexityBreakdownEnabled,
  getComplexityDecorationCap: () => 200,
  getIssueTypes: () => ({}),
  getChangedSince: () => "",
  getResolvedConfigPath: (workspaceRoot?: string) => {
    mockResolvedConfigRoots.push(workspaceRoot ?? "");
    return mockConfigPathSetting && workspaceRoot
      ? join(workspaceRoot, mockConfigPathSetting)
      : mockConfigPathSetting;
  },
  getWorkspaceScope: () => "",
}));

vi.mock("../src/binary-utils.js", () => ({
  getExecutableExtension: () => "",
  findLocalBinary: (name: string) => (name === "fallow" ? mockLocalBinary : null),
  findBinaryInPath: (name: string) => (name === "fallow" ? mockPathBinary : null),
  // Mirror the real sibling resolution against the mocked fs (mockFiles): the
  // `fallow` CLI sibling of a configured `fallow.lspPath` is `<dir>/fallow`.
  resolveConfiguredBinaryPath: (configured: string, name: string) => {
    const slash = configured.lastIndexOf("/");
    const dir = slash >= 0 ? configured.slice(0, slash) : ".";
    const sibling = `${dir}/${name}`;
    if (mockFiles.has(configured) && configured.endsWith(`/${name}`)) {
      return configured;
    }
    return mockFiles.has(sibling) ? sibling : null;
  },
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
  buildInspectArgs,
  runInspectActiveFile,
  runInspectActiveSymbol,
  resolveCliBinary,
  resolveCliForRun,
  runAnalysis,
  runAudit,
  runFix,
  runHealthAnalysis,
  resetHealthNoWorkspaceWarning,
} from "../src/commands.js";
import { AnalysisFailureBackoff } from "../src/analysisBackoff.js";
import { resetBinarySkewToast } from "../src/binary-skew.js";

const context = {} as unknown as vscode.ExtensionContext;
const workspaceContext = {
  workspaceState: {
    get: () => "",
  },
} as unknown as vscode.ExtensionContext;

beforeEach(() => {
  mockAutoDownload = true;
  mockConfigPathSetting = "";
  mockResolvedConfigRoots = [];
  mockTypeAwareSettings = { enabled: false, projects: [], require: "best-effort" };
});

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

interface ActiveEditorOptions {
  readonly isDirty?: boolean;
  readonly save?: () => Promise<boolean>;
  readonly symbol?: string;
}

const setActiveEditor = (fsPath: string | null, options: ActiveEditorOptions = {}): void => {
  mockActiveTextEditor =
    fsPath === null
      ? undefined
      : {
          document: {
            uri: { scheme: "file", fsPath },
            isDirty: options.isDirty ?? false,
            save: options.save ?? (() => Promise.resolve(true)),
            getWordRangeAtPosition: () => (options.symbol ? {} : undefined),
            getText: () => options.symbol ?? "",
          },
          selection: { active: {} },
        };
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

  it("rejects once with an actionable message when stdout exceeds the cap", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-exec-overflow-"));

    try {
      // Stream well past the 50MB cap in chunks so the overflow trips inside a
      // `data` handler before the child finishes; the kill+guard must yield a
      // single rejection, not a follow-up "exited via signal" reject on close.
      const script = join(dir, "flood.mjs");
      await writeFile(
        script,
        [
          'const chunk = "x".repeat(4 * 1024 * 1024);',
          "for (let i = 0; i < 16; i += 1) {",
          "  process.stdout.write(chunk);",
          "}",
        ].join("\n"),
        "utf8",
      );

      let caught: unknown = null;
      try {
        await execFallow(process.execPath, [script], dir);
      } catch (err) {
        caught = err;
      }

      expect(caught).toBeInstanceOf(Error);
      expect((caught as Error).message).toContain("output exceeded 50 MB");
      expect((caught as Error).message).toContain("ignorePatterns");
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

  it("uses the CLI sibling of a configured LSP path first", async () => {
    mockLspPath = "/tools/fallow-lsp";
    mockFiles = new Set(["/tools/fallow"]);
    mockLocalBinary = "/workspace/node_modules/.bin/fallow";
    mockPathBinary = "/usr/local/bin/fallow";
    mockInstalledCli = "/storage/bin/fallow";

    expect(await findCliBinary(context)).toBe("/tools/fallow");
  });

  it("prefers the workspace CLI before PATH and managed storage", async () => {
    mockLocalBinary = "/workspace/node_modules/.bin/fallow";
    mockPathBinary = "/usr/local/bin/fallow";
    mockInstalledCli = "/storage/bin/fallow";

    expect(await findCliBinary(context)).toBe("/workspace/node_modules/.bin/fallow");
  });

  it("uses the managed CLI after configured, workspace, and PATH lookups miss", async () => {
    mockInstalledCli = "/storage/bin/fallow";

    expect(await findCliBinary(context)).toBe("/storage/bin/fallow");
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
    mockComplexityBreakdownEnabled = false;
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

describe("runAudit type-aware compatibility", () => {
  beforeEach(() => {
    mockLspPath = "";
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = null;
    mockBinaryVersions = {};
    mockAutoDownload = false;
    setWorkspaceRoot(null);
    resetBinarySkewToast();
    vi.clearAllMocks();
  });

  it("retries once without the complete type-aware flag group when version probing is unavailable", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-audit-old-cli-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");
    const output = JSON.stringify({ command: "audit", verdict: "pass" });

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          "const fs = require('node:fs');",
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + '\\n');`,
          "if (process.argv.includes('--type-aware')) {",
          "  console.error(\"error: unexpected argument '--type-aware' found\");",
          "  process.exit(2);",
          "}",
          `process.stdout.write(${JSON.stringify(output)});`,
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      mockTypeAwareSettings = {
        enabled: true,
        projects: ["tsconfig.json"],
        require: "complete",
      };
      setWorkspaceRoot(dir);

      await expect(runAudit(workspaceContext)).resolves.toMatchObject({
        command: "audit",
        verdict: "pass",
      });
      const calls = await readSpawnLog(logPath);
      expect(calls).toHaveLength(2);
      expect(calls[0]?.args).toEqual(
        expect.arrayContaining([
          "--type-aware",
          "--type-aware-project",
          "tsconfig.json",
          "--type-aware-require",
          "complete",
        ]),
      );
      expect(calls[1]?.args).not.toContain("--type-aware");
      expect(calls[1]?.args).not.toContain("--type-aware-project");
      expect(calls[1]?.args).not.toContain("--type-aware-require");
      expect(mockWindow.showWarningMessage).toHaveBeenCalledTimes(1);
      expect(mockWindow.showWarningMessage).toHaveBeenCalledWith(
        expect.stringContaining("enable fallow.autoDownload"),
      );
    } finally {
      mockAutoDownload = true;
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("runFix type-aware integration", () => {
  beforeEach(() => {
    mockLspPath = "";
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = null;
    mockBinaryVersions = {};
    mockAutoDownload = false;
    setWorkspaceRoot(null);
    resetBinarySkewToast();
    vi.clearAllMocks();
  });

  it("forwards guarded type-aware settings to fix preview", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-fix-type-aware-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          "process.stdout.write(JSON.stringify({ fixes: [] }));",
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      mockBinaryVersions = { [script]: "3.8.2" };
      mockTypeAwareSettings = {
        enabled: true,
        projects: ["tsconfig.app.json"],
        require: "complete",
      };
      setWorkspaceRoot(dir);

      await runFix(workspaceContext, true);

      const calls = await readSpawnLog(logPath);
      expect(calls).toHaveLength(1);
      expect(calls[0]?.args).toEqual([
        "fix",
        "--dry-run",
        "--format",
        "json",
        "--quiet",
        "--type-aware",
        "--type-aware-project",
        "tsconfig.app.json",
        "--type-aware-require",
        "complete",
      ]);
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

describe("runInspectActiveFile", () => {
  beforeEach(() => {
    mockLspPath = "";
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = null;
    mockBinaryVersions = {};
    setWorkspaceRoot(null);
    setActiveEditor(null);
    vi.clearAllMocks();
  });

  it("runs inspect for the active file and writes the JSON bundle to the output channel", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-inspect-"));
    const script = join(dir, "fallow-cli.js");
    const filePath = join(dir, "src", "extension.ts");
    const logPath = join(dir, "spawn.log");
    const output = JSON.stringify({
      kind: "inspect_target",
      target: { type: "file", file: "src/extension.ts" },
      identity: { file: "src/extension.ts" },
      evidence: {},
      warnings: [],
    });
    const outputChannel = {
      appendLine: vi.fn(),
      show: vi.fn(),
    } as unknown as vscode.OutputChannel;

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          `process.stdout.write(${JSON.stringify(output)});`,
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);
      setActiveEditor(filePath);

      const result = await runInspectActiveFile(workspaceContext, outputChannel);
      const calls = await readSpawnLog(logPath);

      expect(result?.kind).toBe("inspect_target");
      expect(calls[0]?.args).toEqual([
        "inspect",
        "--file",
        "src/extension.ts",
        "--format",
        "json",
        "--quiet",
      ]);
      expect(outputChannel.appendLine).toHaveBeenCalledWith("Fallow inspect: src/extension.ts");
      expect(outputChannel.show).toHaveBeenCalled();
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("forwards inspect production false as --no-production", () => {
    expect(
      buildInspectArgs({
        filePath: "src/extension.ts",
        production: false,
        workspace: "app",
        configPath: "/repo/.fallowrc.json",
      }),
    ).toEqual([
      "inspect",
      "--file",
      "src/extension.ts",
      "--format",
      "json",
      "--quiet",
      "--workspace",
      "app",
      "--no-production",
      "--config",
      "/repo/.fallowrc.json",
    ]);
  });

  it("forwards type-aware settings for exact-symbol inspect", () => {
    expect(
      buildInspectArgs({
        filePath: "src/extension.ts",
        symbol: "activate",
        production: undefined,
        workspace: "",
        configPath: "",
        typeAware: {
          enabled: true,
          projects: ["tsconfig.json", "packages/app/tsconfig.json"],
          require: "complete",
        },
      }),
    ).toEqual([
      "inspect",
      "--symbol",
      "src/extension.ts:activate",
      "--format",
      "json",
      "--quiet",
      "--type-aware",
      "--type-aware-project",
      "tsconfig.json",
      "--type-aware-project",
      "packages/app/tsconfig.json",
      "--type-aware-require",
      "complete",
    ]);
  });

  it("inspects the exported symbol at the cursor with semantic settings", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-inspect-symbol-"));
    const script = join(dir, "fallow-cli.js");
    const filePath = join(dir, "src", "extension.ts");
    const logPath = join(dir, "spawn.log");

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          'process.stdout.write(\'{"kind":"inspect_target","warnings":[]}\');',
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      mockTypeAwareSettings = {
        enabled: true,
        projects: ["tsconfig.json"],
        require: "best-effort",
      };
      setWorkspaceRoot(dir);
      setActiveEditor(filePath, { symbol: "activate" });

      await expect(runInspectActiveSymbol(workspaceContext)).resolves.not.toBeNull();
      const calls = await readSpawnLog(logPath);
      expect(calls[0]?.args).toContain("src/extension.ts:activate");
      expect(calls[0]?.args).toContain("--type-aware");
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("resolves relative inspect config paths from the active editor workspace root", async () => {
    const firstRoot = await mkdtemp(join(tmpdir(), "fallow-vscode-root-a-"));
    const secondRoot = await mkdtemp(join(tmpdir(), "fallow-vscode-root-b-"));
    const script = join(secondRoot, "fallow-cli.js");
    const filePath = join(secondRoot, "src", "extension.ts");
    const logPath = join(secondRoot, "spawn.log");

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          'process.stdout.write(\'{"kind":"inspect_target","warnings":[]}\');',
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      mockConfigPathSetting = ".config/fallow.json";
      const workspace = mockWorkspace as {
        workspaceFolders: ReadonlyArray<{ readonly uri: { readonly fsPath: string } }>;
      };
      workspace.workspaceFolders = [
        { uri: { fsPath: firstRoot } },
        { uri: { fsPath: secondRoot } },
      ];
      setActiveEditor(filePath);

      await expect(runInspectActiveFile(workspaceContext)).resolves.not.toBeNull();
      const calls = await readSpawnLog(logPath);

      expect(mockResolvedConfigRoots).toContain(secondRoot);
      expect(calls[0]?.args).toContain(join(secondRoot, ".config/fallow.json"));
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(firstRoot, { recursive: true, force: true });
      await rm(secondRoot, { recursive: true, force: true });
    }
  });

  it("retries inspect with the managed CLI when the resolved CLI rejects the subcommand", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-inspect-managed-"));
    const staleScript = join(dir, "stale-fallow.js");
    const managedScript = join(dir, "managed-fallow.js");
    const filePath = join(dir, "src", "extension.ts");
    const logPath = join(dir, "spawn.log");
    const outputChannel = {
      appendLine: vi.fn(),
      show: vi.fn(),
    } as unknown as vscode.OutputChannel;

    try {
      await writeFile(
        staleScript,
        [
          "#!/usr/bin/env node",
          'process.stderr.write("error: unrecognized subcommand \\"inspect\\"\\n");',
          "process.exit(2);",
        ].join("\n"),
        "utf8",
      );
      await writeFile(
        managedScript,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          'process.stdout.write(\'{"kind":"inspect_target","warnings":[]}\');',
        ].join("\n"),
        "utf8",
      );
      await chmod(staleScript, 0o755);
      await chmod(managedScript, 0o755);

      mockPathBinary = staleScript;
      mockInstalledCli = managedScript;
      setWorkspaceRoot(dir);
      setActiveEditor(filePath);

      const result = await runInspectActiveFile(workspaceContext, outputChannel);
      const calls = await readSpawnLog(logPath);

      expect(result?.kind).toBe("inspect_target");
      expect(calls).toHaveLength(1);
      expect(outputChannel.appendLine).toHaveBeenCalledWith(
        "Fallow: resolved CLI does not support inspect; switched to the managed CLI.",
      );
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("saves a dirty active file before running inspect", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-inspect-save-"));
    const script = join(dir, "fallow-cli.js");
    const filePath = join(dir, "src", "extension.ts");
    const logPath = join(dir, "spawn.log");
    const save = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          'const fs = require("node:fs");',
          `fs.appendFileSync(${JSON.stringify(logPath)}, JSON.stringify({ args: process.argv.slice(2) }) + "\\n");`,
          'process.stdout.write(\'{"kind":"inspect_target","warnings":[]}\');',
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);
      setActiveEditor(filePath, { isDirty: true, save });

      const result = await runInspectActiveFile(workspaceContext);
      const calls = await readSpawnLog(logPath);

      expect(save).toHaveBeenCalledOnce();
      expect(result?.kind).toBe("inspect_target");
      expect(calls[0]?.args).toContain("src/extension.ts");
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("does not run inspect when saving a dirty active file fails", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-inspect-save-fail-"));
    const script = join(dir, "fallow-cli.js");
    const filePath = join(dir, "src", "extension.ts");
    const logPath = join(dir, "spawn.log");
    const save = vi.fn<() => Promise<boolean>>().mockResolvedValue(false);

    try {
      await writeFile(script, "#!/usr/bin/env node\n", "utf8");
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);
      setActiveEditor(filePath, { isDirty: true, save });

      const result = await runInspectActiveFile(workspaceContext);

      expect(result).toBeNull();
      expect(save).toHaveBeenCalledOnce();
      expect(mockWindow.showWarningMessage).toHaveBeenCalledWith(
        "Fallow inspect cancelled because src/extension.ts could not be saved.",
      );
      await expect(readSpawnLog(logPath)).rejects.toThrow();
    } finally {
      setWorkspaceRoot(null);
      setActiveEditor(null);
      await rm(dir, { recursive: true, force: true });
    }
  });
});

describe("runHealthAnalysis return type (envelope reachable)", () => {
  beforeEach(() => {
    mockLspPath = "";
    mockLocalBinary = null;
    mockPathBinary = null;
    mockInstalledCli = null;
    mockDownloadedCli = null;
    mockExtensionVersion = null;
    mockBinaryVersions = {};
    mockComplexityBreakdownEnabled = false;
    setWorkspaceRoot(null);
    resetHealthNoWorkspaceWarning();
    vi.clearAllMocks();
  });

  // The declared return type is `HealthOutput | null`, not `HealthReport | null`.
  // `HealthOutput` is the envelope flattened over the report body, so the
  // envelope fields (`schema_version`, `version`, `next_steps`) must be
  // reachable on the resolved value WITHOUT a cast. Reading them here would not
  // type-check if the declaration narrowed back to the report body.
  it("resolves a value whose envelope fields are typed and present", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-health-envelope-"));
    const script = join(dir, "fallow-cli.js");
    const output = JSON.stringify({
      schema_version: 7,
      version: "9.9.9-test",
      elapsed_ms: 12,
      findings: [],
      summary: {},
      next_steps: [{ id: "health-clean", title: "Project is healthy", command: "fallow health" }],
    });

    try {
      await writeFile(
        script,
        ["#!/usr/bin/env node", `process.stdout.write(${JSON.stringify(output)});`].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      setWorkspaceRoot(dir);

      const report = await runHealthAnalysis(workspaceContext);
      expect(report).not.toBeNull();
      // Envelope-level access only compiles when the return type is HealthOutput.
      expect(report?.schema_version).toBe(7);
      expect(report?.version).toBe("9.9.9-test");
      expect(report?.next_steps?.[0]?.id).toBe("health-clean");
    } finally {
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("retries without complexity breakdown when an older CLI rejects the flag", async () => {
    const dir = await mkdtemp(join(tmpdir(), "fallow-vscode-health-old-cli-"));
    const script = join(dir, "fallow-cli.js");
    const logPath = join(dir, "spawn.log");
    const output = JSON.stringify({
      schema_version: 7,
      version: "9.9.9-test",
      elapsed_ms: 12,
      findings: [],
      summary: {},
      next_steps: [],
    });

    try {
      await writeFile(
        script,
        [
          "#!/usr/bin/env node",
          "const fs = require('node:fs');",
          `fs.appendFileSync(${JSON.stringify(logPath)}, process.argv.slice(2).join(' ') + '\\n');`,
          "if (process.argv.includes('--complexity-breakdown')) {",
          "  console.error(\"error: unexpected argument '--complexity-breakdown' found\");",
          "  process.exit(2);",
          "}",
          `process.stdout.write(${JSON.stringify(output)});`,
        ].join("\n"),
        "utf8",
      );
      await chmod(script, 0o755);

      mockPathBinary = script;
      mockComplexityBreakdownEnabled = true;
      setWorkspaceRoot(dir);

      const report = await runHealthAnalysis(workspaceContext);
      expect(report?.version).toBe("9.9.9-test");

      const calls = (await readFile(logPath, "utf8")).trim().split("\n");
      expect(calls).toHaveLength(2);
      expect(calls[0]).toContain("--complexity-breakdown");
      expect(calls[1]).not.toContain("--complexity-breakdown");
    } finally {
      mockComplexityBreakdownEnabled = false;
      setWorkspaceRoot(null);
      await rm(dir, { recursive: true, force: true });
    }
  });
});
