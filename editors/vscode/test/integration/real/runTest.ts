import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

const extensionDevelopmentPath = path.resolve(__dirname, "../../../..");
const extensionTestsPath = path.resolve(__dirname, "suite/index.js");
const vscodeTestCachePath = path.join(os.tmpdir(), "fallow-vscode-test-cache");
const fixtureWorkspacePath = path.resolve(
  extensionDevelopmentPath,
  "test/integration/fixtures/real-workspace",
);

const requiredCurrentBinary = (): string => {
  const configured = process.env["FALLOW_BIN"];
  if (!configured) {
    throw new Error(
      "FALLOW_BIN is required for the real VS Code CLI/LSP contract smoke",
    );
  }

  const binary = path.resolve(configured);
  fs.accessSync(binary, fs.constants.X_OK);
  return binary;
};

const writeExecutable = (filePath: string, contents: string): void => {
  fs.writeFileSync(filePath, contents, "utf8");
  fs.chmodSync(filePath, 0o755);
};

const createWorkspace = (binary: string): string => {
  const workspaceDir = fs.mkdtempSync(
    path.join(fs.realpathSync(os.tmpdir()), "fallow-vscode-real-"),
  );
  const vscodeDir = path.join(workspaceDir, ".vscode");
  const binDir = path.join(workspaceDir, "bin");
  const cliPath = path.join(binDir, "fallow");
  const lspPath = path.join(binDir, "fallow-lsp");

  fs.cpSync(fixtureWorkspacePath, workspaceDir, { recursive: true });
  fs.mkdirSync(vscodeDir, { recursive: true });
  fs.mkdirSync(binDir, { recursive: true });
  fs.symlinkSync(binary, cliPath);
  writeExecutable(lspPath, '#!/bin/sh\nexec "$FALLOW_BIN" lsp-server\n');
  fs.writeFileSync(
    path.join(vscodeDir, "settings.json"),
    `${JSON.stringify(
      {
        "fallow.autoDownload": false,
        "fallow.lspPath": lspPath,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  return workspaceDir;
};

const main = async (): Promise<void> => {
  const binary = requiredCurrentBinary();
  const workspaceDir = createWorkspace(binary);
  const extensionsDir = path.join(workspaceDir, ".vscode-test", "extensions");
  const userDataDir = path.join(workspaceDir, ".vscode-test", "user-data");

  try {
    await runTests({
      cachePath: vscodeTestCachePath,
      extensionDevelopmentPath,
      extensionTestsEnv: { FALLOW_BIN: binary },
      extensionTestsPath,
      launchArgs: [
        workspaceDir,
        "--disable-extensions",
        `--extensions-dir=${extensionsDir}`,
        `--user-data-dir=${userDataDir}`,
      ],
      version: "1.96.0",
    });
  } finally {
    fs.rmSync(workspaceDir, { recursive: true, force: true });
  }
};

void main();
