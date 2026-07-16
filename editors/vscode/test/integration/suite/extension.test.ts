import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type { FallowCheckResult, FallowDupesResult, FallowFixResult } from "../../../src/types.js";

interface ExtensionApi {
  readonly runAnalysis: (context: vscode.ExtensionContext) => Promise<{
    check: FallowCheckResult | null;
    dupes: FallowDupesResult | null;
  }>;
  readonly runFix: (
    context: vscode.ExtensionContext,
    dryRun: boolean,
  ) => Promise<FallowFixResult | null>;
}

const defaultIssueTypes = {
  "unused-files": true,
  "unused-exports": true,
  "unused-types": true,
  "unused-dependencies": true,
  "unused-dev-dependencies": true,
  "unused-optional-dependencies": true,
  "unused-enum-members": true,
  "unused-class-members": true,
  "unresolved-imports": true,
  "unlisted-dependencies": true,
  "duplicate-exports": true,
  "type-only-dependencies": true,
  "circular-dependencies": true,
};

const workspaceFolder = (): vscode.WorkspaceFolder => {
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, "workspace folder should exist");
  return folder;
};

/**
 * Minimal in-memory `Memento` so `runAnalysis` can resolve the workspace-scope
 * override (`context.workspaceState`) the way the real extension context does.
 */
const inMemoryMemento = (): vscode.Memento => {
  const store = new Map<string, unknown>();
  return {
    keys: () => [...store.keys()],
    get: <T>(key: string, defaultValue?: T): T | undefined =>
      store.has(key) ? (store.get(key) as T) : defaultValue,
    update: (key: string, value: unknown): Thenable<void> => {
      if (value === undefined) {
        store.delete(key);
      } else {
        store.set(key, value);
      }
      return Promise.resolve();
    },
  };
};

const testContext = (): vscode.ExtensionContext =>
  ({
    globalStorageUri: vscode.Uri.file(path.join(workspaceFolder().uri.fsPath, ".global-storage")),
    workspaceState: inMemoryMemento(),
  }) as vscode.ExtensionContext;

const cliLogPath = (): string => path.join(workspaceFolder().uri.fsPath, ".fallow-cli-log.jsonl");

const readCliLog = (): Array<{ command: string; args: string[] }> => {
  const logPath = cliLogPath();
  if (!fs.existsSync(logPath)) {
    return [];
  }

  return fs
    .readFileSync(logPath, "utf8")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as { command: string; args: string[] });
};

const readFixCommands = (): Array<{ command: string; args: string[] }> =>
  readCliLog().filter((entry) => entry.command === "fix");

/**
 * CLI commands the extension spawns that are NOT the sidebar dead-code +
 * duplication analysis: monorepo discovery for the workspace picker, and the
 * lazily-spawned health / audit / security / coverage / fix surfaces. The
 * combined-mode assertions filter these out so they only inspect the analysis
 * itself.
 */
const NON_ANALYSIS_COMMANDS = new Set([
  "workspaces",
  "health",
  "audit",
  "security",
  "coverage",
  "fix",
]);

const runAnalysisAndReadCliLog = async (
  api: ExtensionApi,
): Promise<Array<{ command: string; args: string[] }>> => {
  const result = await api.runAnalysis(testContext());

  assert.ok(result.check, "check result should be available");
  assert.ok(result.dupes, "duplication result should be available");

  return readCliLog();
};

describe("Fallow VS Code extension", () => {
  let api: ExtensionApi;
  const windowApi = vscode.window as any;
  const originalShowQuickPick = vscode.window.showQuickPick;
  const originalShowTextDocument = vscode.window.showTextDocument;
  const originalShowWarningMessage = vscode.window.showWarningMessage;
  const originalShowInformationMessage = vscode.window.showInformationMessage;

  before(async () => {
    const extension = vscode.extensions.getExtension("fallow-rs.fallow-vscode");
    assert.ok(extension, "extension should be discoverable");
    api = (await extension.activate()) as ExtensionApi;
  });

  beforeEach(() => {
    windowApi.showInformationMessage = async () => undefined;
  });

  afterEach(async () => {
    if (fs.existsSync(cliLogPath())) {
      fs.rmSync(cliLogPath(), { force: true });
    }

    await vscode.workspace
      .getConfiguration("fallow")
      .update("issueTypes", defaultIssueTypes, vscode.ConfigurationTarget.Workspace);
    await vscode.workspace
      .getConfiguration("fallow")
      .update("changedSince", "", vscode.ConfigurationTarget.Workspace);
    const config = vscode.workspace.getConfiguration("fallow");
    for (const key of [
      "duplication.mode",
      "duplication.threshold",
      "duplication.minTokens",
      "duplication.minLines",
      "duplication.minOccurrences",
      "duplication.skipLocal",
      "duplication.crossLanguage",
      "duplication.ignoreImports",
    ]) {
      await config.update(key, undefined, vscode.ConfigurationTarget.Workspace);
    }

    windowApi.showQuickPick = originalShowQuickPick;
    windowApi.showTextDocument = originalShowTextDocument;
    windowApi.showWarningMessage = originalShowWarningMessage;
    windowApi.showInformationMessage = originalShowInformationMessage;
  });

  it("registers the expected commands", async () => {
    const commands = await vscode.commands.getCommands(true);

    assert.ok(commands.includes("fallow.analyze"));
    assert.ok(commands.includes("fallow.reloadAnalysis"));
    assert.ok(commands.includes("fallow.health.reload"));
    assert.ok(commands.includes("fallow.audit"));
    assert.ok(commands.includes("fallow.setBaselineAtHead"));
    assert.ok(commands.includes("fallow.fix"));
    assert.ok(commands.includes("fallow.fixDryRun"));
    assert.ok(commands.includes("fallow.restart"));
    // Escape hatch for a stuck-hidden workspace (discussion #287); if it is
    // unregistered the startup nudge's "Show all findings" action throws.
    assert.ok(commands.includes("fallow.resetDiagnosticFilters"));
    // The "N references" Code Lens routes here; if it is unregistered, clicking a
    // lens throws "command 'fallow.showReferences' not found".
    assert.ok(commands.includes("fallow.showReferences"));
  });

  it("runs analysis against the configured CLI and filters disabled issue types", async () => {
    await vscode.workspace.getConfiguration("fallow").update(
      "issueTypes",
      {
        ...defaultIssueTypes,
        "unused-exports": false,
      },
      vscode.ConfigurationTarget.Workspace,
    );

    const result = await api.runAnalysis(testContext());

    assert.ok(result.check, "check result should be available");
    assert.ok(result.dupes, "duplication result should be available");
    assert.equal(result.check.unused_files.length, 1);
    assert.equal(result.check.unused_exports.length, 0);
    assert.equal(result.check.unused_optional_dependencies?.length, 1);
    assert.equal(result.dupes.clone_groups.length, 1);

    // The sidebar's dead-code + duplication analysis must be ONE combined call,
    // never split into per-issue-type calls. Calls that are not the sidebar
    // analysis are excluded: `fallow workspaces` (monorepo discovery for the
    // workspace picker, #906) and the lazily-spawned health / audit / security /
    // coverage surfaces are orthogonal commands, not the dead-code analysis.
    const sidebarAnalysisCalls = readCliLog().filter(
      (entry) => !NON_ANALYSIS_COMMANDS.has(entry.command),
    );
    assert.ok(sidebarAnalysisCalls.length >= 1, "expected at least one CLI analysis call");
    assert.ok(
      sidebarAnalysisCalls.every((entry) => entry.command === "combined"),
      "sidebar analysis should use combined mode only (not split per issue type)",
    );
    // `.some`, not `.every`: a config change in a prior test's afterEach fires a
    // background `triggerCliAnalysis()` whose log entry can race into this test's
    // log. The awaited direct call is what we assert produced the expected argv.
    assert.ok(
      sidebarAnalysisCalls.some(
        (entry) => entry.args.join(" ") === "--format json --quiet --skip health",
      ),
      "combined analysis should not pass package default duplication settings as overrides",
    );
  });

  it("forwards duplication settings to the CLI analysis path", async () => {
    const config = vscode.workspace.getConfiguration("fallow");
    await config.update("duplication.mode", "mild", vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.threshold", 0, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.minTokens", 80, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.minLines", 8, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.minOccurrences", 3, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.skipLocal", true, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.crossLanguage", true, vscode.ConfigurationTarget.Workspace);
    await config.update("duplication.ignoreImports", true, vscode.ConfigurationTarget.Workspace);

    const analysisCalls = await runAnalysisAndReadCliLog(api);
    assert.ok(
      analysisCalls.some(
        (entry) =>
          entry.args.join(" ") ===
          "--format json --quiet --skip health --dupes-mode mild --dupes-threshold 0 --dupes-min-tokens 80 --dupes-min-lines 8 --dupes-min-occurrences 3 --dupes-skip-local --dupes-cross-language --dupes-ignore-imports",
      ),
      "combined analysis should include configured duplication settings",
    );
  });

  it("forwards changedSince to the CLI analysis path", async () => {
    await vscode.workspace
      .getConfiguration("fallow")
      .update("changedSince", "origin/main", vscode.ConfigurationTarget.Workspace);

    const analysisCalls = await runAnalysisAndReadCliLog(api);
    assert.ok(analysisCalls.length >= 1, "expected at least one CLI analysis call");
    // `.some` for the same reason as above: assert the awaited direct call's
    // argv is present, tolerating a stray background analysis from a prior
    // afterEach config reset.
    assert.ok(
      analysisCalls.some(
        (entry) =>
          entry.args.join(" ") ===
          "--format json --quiet --skip health --changed-since origin/main",
      ),
      "combined analysis should include --changed-since without package default duplication overrides",
    );
  });

  it("forwards --workspace to the CLI analysis path when a workspace is selected", async () => {
    // The picker persists its choice under this workspaceState key; the analysis
    // path reads it via resolveActiveWorkspaceScope and appends --workspace.
    const context = testContext();
    await context.workspaceState.update("fallow.workspaceScope", "pkg-a");

    const result = await api.runAnalysis(context);
    assert.ok(result.check, "check result should be available");

    const analysisCalls = readCliLog();
    // `.some` (not `.every`): a stray background analysis from a prior test's
    // afterEach config reset can race into this log. Assert the awaited direct
    // call's argv includes the scoped --workspace flag.
    assert.ok(
      analysisCalls.some(
        (entry) =>
          entry.command === "combined" &&
          entry.args.join(" ") === "--format json --quiet --skip health --workspace pkg-a",
      ),
      "combined analysis should forward --workspace <name> for the selected workspace",
    );
  });

  it("navigates to the selected dry-run fix even when labels collide", async () => {
    let openedPath = "";
    let openedLine = -1;

    windowApi.showQuickPick = async (items: readonly vscode.QuickPickItem[]) => items[1];
    windowApi.showTextDocument = async (
      uri: vscode.Uri,
      options?: vscode.TextDocumentShowOptions,
    ) => {
      openedPath = uri.fsPath;
      openedLine = options?.selection?.start.line ?? -1;
      return {} as vscode.TextEditor;
    };

    const result = await api.runFix(testContext(), true);

    assert.ok(result, "dry-run result should be returned");
    assert.equal(result.fixes.length, 2);
    assert.equal(openedPath, path.join(workspaceFolder().uri.fsPath, "src/second.ts"));
    assert.equal(openedLine, 6);
    assert.deepEqual(readFixCommands(), [
      {
        command: "fix",
        args: ["fix", "--dry-run", "--format", "json", "--quiet"],
      },
    ]);
  });

  it("cancels apply mode before invoking the CLI", async () => {
    windowApi.showWarningMessage = async () => "No";

    const result = await api.runFix(testContext(), false);

    assert.equal(result, null);
    assert.deepEqual(readFixCommands(), []);
  });

  it("applies fixes after confirmation and reports the result", async () => {
    let infoMessage = "";

    windowApi.showWarningMessage = async () => "Yes";
    windowApi.showInformationMessage = async (message: string) => {
      infoMessage = message;
      return undefined;
    };

    const result = await api.runFix(testContext(), false);

    assert.ok(result, "apply result should be returned");
    assert.equal(result.fixes.length, 1);
    assert.equal(infoMessage, "Fallow: applied 1 fix.");
    assert.deepEqual(readFixCommands(), [
      {
        command: "fix",
        args: ["fix", "--yes", "--format", "json", "--quiet"],
      },
    ]);
  });
});
