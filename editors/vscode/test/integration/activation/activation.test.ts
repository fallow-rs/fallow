import * as assert from "node:assert/strict";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

const EXTENSION_ID = "fallow-rs.fallow-vscode";
const STARTUP_SETTLE_MS = 3_000;
const ACTIVATION_TIMEOUT_MS = 15_000;
const POLL_MS = 100;

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => {
    setTimeout(resolve, ms);
  });

const fallowExtension = (): vscode.Extension<unknown> => {
  const extension = vscode.extensions.getExtension(EXTENSION_ID);
  assert.ok(extension, `${EXTENSION_ID} should be installed`);
  return extension;
};

const waitForActivation = async (extension: vscode.Extension<unknown>): Promise<boolean> => {
  const deadline = Date.now() + ACTIVATION_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (extension.isActive) {
      return true;
    }
    await sleep(POLL_MS);
  }
  return extension.isActive;
};

describe("activation without a root package.json", () => {
  it("stays inactive until a source file opens", async function () {
    this.timeout(STARTUP_SETTLE_MS + ACTIVATION_TIMEOUT_MS + 5_000);
    const folder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(folder, "workspace folder should exist");
    const extension = fallowExtension();

    await vscode.window.showTextDocument(
      vscode.Uri.file(path.join(folder.uri.fsPath, "notes.md")),
    );
    await sleep(STARTUP_SETTLE_MS);
    assert.equal(
      extension.isActive,
      false,
      "a nested package.json and a Markdown file must not activate the extension",
    );

    await vscode.window.showTextDocument(
      vscode.Uri.file(path.join(folder.uri.fsPath, "packages", "app", "src", "index.ts")),
    );
    assert.equal(
      await waitForActivation(extension),
      true,
      "opening a TypeScript file must activate the extension",
    );
  });
});
