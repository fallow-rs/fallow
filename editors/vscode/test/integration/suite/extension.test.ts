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
    dryRun: boolean
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

const testContext = (): vscode.ExtensionContext =>
  ({
    globalStorageUri: vscode.Uri.file(
      path.join(workspaceFolder().uri.fsPath, ".global-storage")
    ),
  }) as vscode.ExtensionContext;

const cliLogPath = (): string =>
  path.join(workspaceFolder().uri.fsPath, ".fallow-cli-log.jsonl");

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

  afterEach(async () => {
    if (fs.existsSync(cliLogPath())) {
      fs.rmSync(cliLogPath(), { force: true });
    }

    await vscode.workspace
      .getConfiguration("fallow")
      .update(
        "issueTypes",
        defaultIssueTypes,
        vscode.ConfigurationTarget.Workspace
      );
    await vscode.workspace
      .getConfiguration("fallow")
      .update("changedSince", "", vscode.ConfigurationTarget.Workspace);

    windowApi.showQuickPick = originalShowQuickPick;
    windowApi.showTextDocument = originalShowTextDocument;
    windowApi.showWarningMessage = originalShowWarningMessage;
    windowApi.showInformationMessage = originalShowInformationMessage;
  });

  it("registers the expected commands", async () => {
    const commands = await vscode.commands.getCommands(true);

    assert.ok(commands.includes("fallow.analyze"));
    assert.ok(commands.includes("fallow.reloadAnalysis"));
    assert.ok(commands.includes("fallow.fix"));
    assert.ok(commands.includes("fallow.fixDryRun"));
    assert.ok(commands.includes("fallow.restart"));
  });

  it("runs analysis against the configured CLI and filters disabled issue types", async () => {
    await vscode.workspace
      .getConfiguration("fallow")
      .update(
        "issueTypes",
        {
          ...defaultIssueTypes,
          "unused-exports": false,
        },
        vscode.ConfigurationTarget.Workspace
      );

    const result = await api.runAnalysis(testContext());

    assert.ok(result.check, "check result should be available");
    assert.ok(result.dupes, "duplication result should be available");
    assert.equal(result.check.unused_files.length, 1);
    assert.equal(result.check.unused_exports.length, 0);
    assert.equal(result.check.unused_optional_dependencies?.length, 1);
    assert.equal(result.dupes.clone_groups.length, 1);

    const analysisCalls = readCliLog();
    assert.ok(analysisCalls.length >= 1, "expected at least one CLI analysis call");
    assert.ok(
      analysisCalls.every((entry) => entry.command === "combined"),
      "analysis should use combined mode only"
    );
    // `.some`, not `.every`: a config change in a prior test's afterEach fires a
    // background `triggerCliAnalysis()` whose log entry can race into this test's
    // log. The awaited direct call is what we assert produced the expected argv.
    assert.ok(
      analysisCalls.some((entry) =>
        entry.args.join(" ") ===
        "--format json --quiet --skip health --dupes-mode mild --dupes-threshold 0"
      ),
      "combined analysis should pass the expected arguments (no --dupes-min-occurrences at the floor)"
    );
  });

  it("forwards changedSince to the CLI analysis path", async () => {
    await vscode.workspace
      .getConfiguration("fallow")
      .update("changedSince", "origin/main", vscode.ConfigurationTarget.Workspace);

    const result = await api.runAnalysis(testContext());

    assert.ok(result.check, "check result should be available");
    assert.ok(result.dupes, "duplication result should be available");

    const analysisCalls = readCliLog();
    assert.ok(analysisCalls.length >= 1, "expected at least one CLI analysis call");
    // `.some` for the same reason as above: assert the awaited direct call's
    // argv is present, tolerating a stray background analysis from a prior
    // afterEach config reset.
    assert.ok(
      analysisCalls.some((entry) =>
        entry.args.join(" ") ===
        "--format json --quiet --skip health --changed-since origin/main --dupes-mode mild --dupes-threshold 0"
      ),
      "combined analysis should include --changed-since before duplication options"
    );
  });

  it("navigates to the selected dry-run fix even when labels collide", async () => {
    let openedPath = "";
    let openedLine = -1;

    windowApi.showQuickPick = async (items: readonly vscode.QuickPickItem[]) =>
      items[1];
    windowApi.showTextDocument = async (
      uri: vscode.Uri,
      options?: vscode.TextDocumentShowOptions
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
