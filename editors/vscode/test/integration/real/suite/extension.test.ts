import * as assert from "node:assert/strict";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type { FallowCheckResult, FallowDupesResult } from "../../../../src/types.js";

interface ExtensionApi {
  readonly runAnalysis: (context: vscode.ExtensionContext) => Promise<{
    check: FallowCheckResult | null;
    dupes: FallowDupesResult | null;
  }>;
}

const workspaceFolder = (): vscode.WorkspaceFolder => {
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, "workspace folder should exist");
  return folder;
};

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

const fallowDiagnostics = async (
  uri: vscode.Uri,
): Promise<readonly vscode.Diagnostic[]> => {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const diagnostics = vscode.languages
      .getDiagnostics(uri)
      .filter((diagnostic) => diagnostic.source === "fallow");
    if (diagnostics.length > 0) {
      return diagnostics;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  return [];
};

describe("Fallow VS Code real-process contracts", () => {
  it("parses the current CLI envelope and receives current LSP diagnostics", async () => {
    const extension = vscode.extensions.getExtension("fallow-rs.fallow-vscode");
    assert.ok(extension, "extension should be discoverable");
    const api = (await extension.activate()) as ExtensionApi;

    const result = await api.runAnalysis(testContext());
    assert.ok(result.check, "current CLI check envelope should parse");
    assert.ok(result.dupes, "current CLI duplication envelope should parse");
    assert.ok(
      result.check.unused_files.some((finding) => finding.path === "src/orphan.ts"),
      "current CLI should report the real fixture's unused file",
    );

    const orphanUri = vscode.Uri.joinPath(workspaceFolder().uri, "src", "orphan.ts");
    const document = await vscode.workspace.openTextDocument(orphanUri);
    await vscode.window.showTextDocument(document);
    const diagnostics = await fallowDiagnostics(orphanUri);

    assert.ok(diagnostics.length > 0, "current LSP should publish a diagnostic for orphan.ts");
  });
});
