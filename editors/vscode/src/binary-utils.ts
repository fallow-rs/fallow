import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

export const getExecutableExtension = (): string => (os.platform() === "win32" ? ".exe" : "");

/**
 * Map the current platform + arch to the `@fallow-cli/<target>` package name(s)
 * that ship the real native executable, mirroring
 * `npm/fallow/scripts/platform-package.js`.
 *
 * On Linux the extension has no `detect-libc`, so both the gnu and musl package
 * names are returned (most-likely first) and the caller probes each on disk; the
 * one that is actually installed wins. Returns an empty array on an unsupported
 * platform/arch.
 */
export const platformPackageNames = (
  platform: NodeJS.Platform = os.platform(),
  arch: string = os.arch(),
): ReadonlyArray<string> => {
  if (platform === "win32") {
    if (arch === "x64") return ["@fallow-cli/win32-x64-msvc"];
    if (arch === "arm64") return ["@fallow-cli/win32-arm64-msvc"];
    return [];
  }
  if (platform === "darwin") {
    if (arch === "x64" || arch === "arm64") return [`@fallow-cli/darwin-${arch}`];
    return [];
  }
  if (platform === "linux") {
    if (arch === "x64" || arch === "arm64") {
      // gnu is by far the common case; musl is the fallback for Alpine and
      // other musl distros. Probe both since libc is not detected here.
      return [`@fallow-cli/linux-${arch}-gnu`, `@fallow-cli/linux-${arch}-musl`];
    }
    return [];
  }
  return [];
};

/**
 * Resolve the real, directly-spawnable native executable for `name` from a npm
 * platform package under `nodeModulesDir`, e.g.
 * `node_modules/@fallow-cli/win32-x64-msvc/fallow-lsp.exe`.
 *
 * The `node_modules/.bin/<name>` entry npm creates is a launcher shim, not the
 * binary: on Windows it is `<name>.cmd` / `<name>.ps1` (which `child_process`
 * and the LSP client cannot `spawn` directly without a shell), and on Unix it is
 * an extensionless Node shebang script. The actual binary lives in the
 * `@fallow-cli/<target>` platform package the shim execs into, so resolving it
 * directly yields a path that spawns cleanly on every platform.
 */
export const findNativeInNodeModules = (nodeModulesDir: string, name: string): string | null => {
  const executableName = `${name}${getExecutableExtension()}`;
  for (const pkg of platformPackageNames()) {
    // `pkg` is a scoped name (`@scope/target`); join its segments onto the
    // node_modules dir so the lookup is correct on Windows path separators too.
    const candidate = path.join(nodeModulesDir, ...pkg.split("/"), executableName);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return null;
};

/**
 * On Unix the `node_modules/.bin/<name>` shim is an extensionless executable
 * Node script and spawns directly, so it is a valid resolution target. On
 * Windows the only `.bin` entries are non-spawnable `.cmd`/`.ps1` wrappers, so
 * there is no usable bare target there (the native exe is resolved separately).
 */
const findBinShim = (binDir: string, name: string): string | null => {
  if (os.platform() === "win32") {
    return null;
  }
  const candidate = path.join(binDir, name);
  return fs.existsSync(candidate) ? candidate : null;
};

/**
 * Look for a locally installed binary in the workspace's node_modules.
 * This allows teams to pin fallow as a devDependency for consistent versions.
 *
 * Resolution order, returning the first spawnable hit:
 *  1. the real native executable inside the `@fallow-cli/<target>` platform
 *     package (works on every OS, including Windows where the `.bin` entry is a
 *     non-spawnable `.cmd`/`.ps1` shim);
 *  2. the `node_modules/.bin/<name>` shim on Unix (extensionless, spawnable).
 */
export const findLocalBinary = (name: string): string | null => {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }

  const nodeModules = path.join(folders[0].uri.fsPath, "node_modules");

  const native = findNativeInNodeModules(nodeModules, name);
  if (native) {
    return native;
  }

  return findBinShim(path.join(nodeModules, ".bin"), name);
};

/**
 * The resolved LSP server invocation: a directly-spawnable `command` plus any
 * leading `args`. Older platform packages shipped a standalone `fallow-lsp`
 * (`args: []`); the current single-binary packages ship only the multicall
 * `fallow`, which serves the LSP as `fallow lsp-server` (`args: ["lsp-server"]`).
 */
export type ResolvedLspBinary = { readonly command: string; readonly args: readonly string[] };

/**
 * Resolve the LSP server from the workspace's `node_modules` platform package,
 * preferring a real `fallow-lsp` binary and falling back to the multicall
 * `fallow lsp-server` when the package ships only the bundled `fallow`. The
 * Unix `.bin/fallow-lsp` launcher shim (which itself spawns `fallow lsp-server`)
 * is the last resort. Returns null when nothing spawnable is installed.
 *
 * This is the only resolution path that produces the multicall form; configured
 * paths, PATH lookups, and the GitHub auto-download still point at real
 * `fallow-lsp` binaries, so old and new installs coexist.
 */
export const findLocalLspBinary = (): ResolvedLspBinary | null => {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }
  const nodeModules = path.join(folders[0].uri.fsPath, "node_modules");

  const lsp = findNativeInNodeModules(nodeModules, "fallow-lsp");
  if (lsp) {
    return { command: lsp, args: [] };
  }
  const multicall = findNativeInNodeModules(nodeModules, "fallow");
  if (multicall) {
    return { command: multicall, args: ["lsp-server"] };
  }
  const shim = findBinShim(path.join(nodeModules, ".bin"), "fallow-lsp");
  return shim ? { command: shim, args: [] } : null;
};

/**
 * Candidate file names to probe for `name` on a PATH directory, most-specific
 * first. On Windows a global `npm i -g fallow` puts `<name>.exe` (rare) or the
 * `<name>.cmd` / `<name>.ps1` launcher shims on PATH; a Cargo/Homebrew/manual
 * install puts a bare `<name>.exe`. On Unix the install is always the bare
 * `<name>`.
 */
const pathCandidateNames = (name: string): ReadonlyArray<string> => {
  if (os.platform() === "win32") {
    return [`${name}.exe`, `${name}.cmd`, `${name}.ps1`];
  }
  return [name];
};

/**
 * Resolve a directory holding a Windows launcher shim (`<dir>/<name>.cmd`) to
 * the real native executable in the sibling `@fallow-cli/<target>` platform
 * package. npm global installs place the shim in the prefix `bin` dir and the
 * package under the adjacent `node_modules`, so probe `<dir>/node_modules` and
 * `<dir>/../node_modules`. Returns null when neither holds the native binary.
 */
const resolveWindowsShimToNative = (shimDir: string, name: string): string | null =>
  findNativeInNodeModules(path.join(shimDir, "node_modules"), name) ??
  findNativeInNodeModules(path.join(shimDir, "..", "node_modules"), name);

const isDirectory = (candidate: string): boolean => {
  try {
    return fs.statSync(candidate).isDirectory();
  } catch {
    return false;
  }
};

/**
 * True for a Windows npm launcher shim (`<name>.cmd` / `<name>.ps1`). These are
 * not spawnable by `child_process`/the LSP client without a shell, so a
 * configured path pointing straight at one must be re-resolved to the native exe
 * rather than honored verbatim.
 */
const isWindowsLauncherShim = (configured: string): boolean => {
  const lower = configured.toLowerCase();
  return lower.endsWith(".cmd") || lower.endsWith(".ps1");
};

/**
 * Resolve a user-configured `fallow.lspPath` to a concrete, spawnable binary for
 * `name` (`fallow-lsp` for the LSP, `fallow` for the CLI sibling).
 *
 * `configured` is honored as the user typed it, tolerating the common shapes
 * that used to silently fail:
 *  - a file path that points straight at the binary (used as-is);
 *  - a file path missing the Windows extension (`...\fallow-lsp`), retried with
 *    `.exe`;
 *  - a directory holding the binaries, in which case `<dir>/<name>(.exe)` is
 *    resolved (so `fallow.lspPath` set to an install folder works), including
 *    the sibling CLI when `name` differs from the configured file's stem;
 *  - a non-spawnable Windows launcher shim (`...\fallow-lsp.cmd` / `.ps1`), which
 *    is re-resolved to the native exe in the sibling `@fallow-cli/<target>`
 *    package so a directly-configured shim path still spawns.
 *
 * Returns the resolved path, or null when nothing exists at any interpretation
 * so the caller can warn that the configured path is wrong.
 */
export const resolveConfiguredBinaryPath = (configured: string, name: string): string | null => {
  const ext = getExecutableExtension();

  // Directory: resolve `<dir>/<name>(.exe)` inside it.
  if (isDirectory(configured)) {
    const inDir = path.join(configured, `${name}${ext}`);
    return fs.existsSync(inDir) ? inDir : null;
  }

  const dir = path.dirname(configured);
  const sibling = path.join(dir, `${name}${ext}`);

  // A directly-configured Windows `.cmd`/`.ps1` launcher shim cannot be spawned
  // as-is. Prefer the native exe sitting next to the shim's `node_modules`
  // (npm global prefix layout), then the bare sibling; never return the shim.
  if (os.platform() === "win32" && isWindowsLauncherShim(configured)) {
    const native = resolveWindowsShimToNative(dir, name);
    if (native) {
      return native;
    }
    return fs.existsSync(sibling) ? sibling : null;
  }

  // When the requested binary is the same one the user pointed at, honor the
  // configured file directly (and tolerate a missing Windows extension).
  const stem = path.basename(configured, path.extname(configured));
  if (stem === name) {
    if (fs.existsSync(configured)) {
      return configured;
    }
    return fs.existsSync(sibling) ? sibling : null;
  }

  // Different binary (e.g. the `fallow` CLI sibling of a configured
  // `fallow-lsp`): look for it next to the configured file.
  return fs.existsSync(sibling) ? sibling : null;
};

export const findBinaryInPath = (name: string): string | null => {
  const pathDirs = (process.env["PATH"] ?? "").split(path.delimiter);
  const candidateNames = pathCandidateNames(name);
  const win32 = os.platform() === "win32";

  for (const dir of pathDirs) {
    if (!dir) {
      continue;
    }
    for (const candidateName of candidateNames) {
      const candidate = path.join(dir, candidateName);
      if (!fs.existsSync(candidate)) {
        continue;
      }
      // A bare `.exe` (or any Unix entry) is the real, directly-spawnable
      // binary. A Windows `.cmd`/`.ps1` is a non-spawnable npm launcher shim, so
      // resolve it to the native exe in the sibling platform package; if that
      // cannot be found, skip it rather than returning an unspawnable shim.
      if (win32 && (candidateName.endsWith(".cmd") || candidateName.endsWith(".ps1"))) {
        const native = resolveWindowsShimToNative(dir, name);
        if (native) {
          return native;
        }
        continue;
      }
      return candidate;
    }
  }

  return null;
};
