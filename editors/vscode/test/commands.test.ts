import type * as vscode from "vscode";
import { beforeEach, describe, expect, it, vi } from "vitest";

let mockFiles: ReadonlySet<string> = new Set();
let mockLspPath = "";
let mockAutoDownload = true;
let mockLocalBinary: string | null = null;
let mockPathBinary: string | null = null;
let mockInstalledCli: string | null = null;
let mockDownloadedCli: string | null = null;

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
  getDuplicationMinOccurrences: () => 2,
  getDuplicationMode: () => "mild",
  getDuplicationThreshold: () => 0,
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
  getInstalledCliPath: () => mockInstalledCli,
  downloadCliBinary: vi.fn(async () => mockDownloadedCli),
}));

import { downloadCliBinary } from "../src/download.js";
import { findCliBinary, resolveCliBinary } from "../src/commands.js";

const context = {} as unknown as vscode.ExtensionContext;

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
