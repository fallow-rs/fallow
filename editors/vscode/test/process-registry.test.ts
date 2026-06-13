import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// execFallow lives in commands.ts, which imports vscode, config, binary-utils,
// and download at module load. Stub them so the module imports under vitest;
// the registry test only exercises the spawn primitive, not CLI resolution.
vi.mock("vscode", () => ({
  QuickPickItemKind: { Separator: -1 },
  window: {
    showWarningMessage: vi.fn(),
    showInformationMessage: vi.fn(),
    showErrorMessage: vi.fn(async () => undefined),
    showQuickPick: vi.fn(),
    showTextDocument: vi.fn(),
  },
  workspace: { workspaceFolders: undefined },
  commands: { executeCommand: vi.fn() },
  Uri: { file: (fsPath: string) => ({ fsPath }) },
  Range: class {},
}));

vi.mock("node:fs", () => ({ existsSync: () => false }));

vi.mock("../src/config.js", () => ({
  getLspPath: () => "",
  getAutoDownload: () => false,
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
  findLocalBinary: () => null,
  findBinaryInPath: () => null,
}));

vi.mock("../src/download.js", () => ({
  getInstalledCliPath: vi.fn(() => null),
  downloadCliBinary: vi.fn(async () => null),
  getBinaryVersion: () => null,
  getExtensionVersion: () => null,
}));

import { execFallow } from "../src/commands.js";
import { activeChildCount, killActiveChildren } from "../src/process-registry.js";

describe("process-registry", () => {
  let dir = "";

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "fallow-vscode-registry-"));
  });

  afterEach(async () => {
    killActiveChildren();
    if (dir) {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("tracks an in-flight child and clears it once the process exits", async () => {
    const script = join(dir, "quick.js");
    await writeFile(
      script,
      ["#!/usr/bin/env node", 'process.stdout.write("done");'].join("\n"),
      "utf8",
    );
    await chmod(script, 0o755);

    await expect(execFallow(process.execPath, [script], dir)).resolves.toBe("done");
    // The close handler unregisters the child, so the registry is empty again.
    expect(activeChildCount()).toBe(0);
  });

  it("killActiveChildren terminates an in-flight child mid-run", async () => {
    // A child that would otherwise run far longer than the test: killing it via
    // the registry must make execFallow settle (close fires with a signal),
    // proving deactivate() can reap an orphaned analysis.
    const script = join(dir, "sleep.js");
    await writeFile(
      script,
      [
        "#!/usr/bin/env node",
        "setTimeout(() => process.exit(0), 60000);",
      ].join("\n"),
      "utf8",
    );
    await chmod(script, 0o755);

    const pending = execFallow(process.execPath, [script], dir);

    // Wait until the spawn has registered before reaping.
    await vi.waitFor(() => {
      expect(activeChildCount()).toBe(1);
    });

    killActiveChildren();
    expect(activeChildCount()).toBe(0);

    // The killed child closes via a signal, which execFallow surfaces as a
    // signal-exit rejection rather than hanging the promise.
    await expect(pending).rejects.toThrow(/signal/);
  });
});
