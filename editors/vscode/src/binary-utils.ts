import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

export const getExecutableExtension = (): string => (os.platform() === "win32" ? ".exe" : "");

/**
 * Look for a locally installed binary in the workspace's node_modules/.bin.
 * This allows teams to pin fallow as a devDependency for consistent versions.
 */
export const findLocalBinary = (name: string): string | null => {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }

  const executableName = `${name}${getExecutableExtension()}`;
  const candidate = path.join(folders[0].uri.fsPath, "node_modules", ".bin", executableName);

  if (fs.existsSync(candidate)) {
    return candidate;
  }

  return null;
};

export const findBinaryInPath = (name: string): string | null => {
  const executableName = `${name}${getExecutableExtension()}`;
  const pathDirs = (process.env["PATH"] ?? "").split(path.delimiter);

  for (const dir of pathDirs) {
    const candidate = path.join(dir, executableName);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return null;
};
