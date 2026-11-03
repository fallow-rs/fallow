import { existsSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative } from "node:path";

const TYPE_AWARE_ENV = "FALLOW_TYPE_AWARE_BIN";
const TYPE_AWARE_SCRIPT_ENV = "FALLOW_TYPE_AWARE_SCRIPT";
const TYPE_AWARE_SOURCE_ENV = "FALLOW_TYPE_AWARE_BIN_SOURCE";
// `type-aware status` reads this marker as "a launcher resolved a
// version-matched companion from node_modules", not as a user override.
const PROJECT_COMPANION_SOURCE = "npm-wrapper";
const COMPANION_PACKAGE = "fallow-type-aware";
const COMPANION_FILE = "fallow-type-aware.mjs";

export interface TypeAwareCommand {
  readonly binary: string;
  readonly script: string | null;
}

/** Resolve the executable form required by the Rust sidecar transport. */
export const typeAwareCommand = (
  companion: string,
  platform: NodeJS.Platform = process.platform,
  execPath: string = process.execPath,
): TypeAwareCommand =>
  platform === "win32"
    ? { binary: execPath, script: companion }
    : { binary: companion, script: null };

let bundledCompanion: string | null = null;

/**
 * Record the companion bundled in the extension. The extension gives it to
 * each Fallow child process through {@link typeAwareEnvironment}.
 */
export const configureBundledTypeAwareCompanion = (extensionPath: string): string | null => {
  const bundled = join(extensionPath, "dist", "type-aware", COMPANION_FILE);
  bundledCompanion = existsSync(bundled) ? bundled : null;
  return bundledCompanion;
};

const readVersion = (manifest: string): string | null => {
  try {
    const version: unknown = JSON.parse(readFileSync(manifest, "utf8")).version;
    return typeof version === "string" ? version : null;
  } catch {
    return null;
  }
};

const isInside = (parent: string, child: string): boolean => {
  const rel = relative(parent, child);
  return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
};

/**
 * Find the `fallow-type-aware` companion in the project `node_modules` for a
 * Fallow binary from the same `node_modules`. The Rust client rejects a
 * companion with a different version, so accept only an exact version match.
 */
const projectTypeAwareCompanion = (
  fallowBinary: string,
  workspaceRoot: string | null,
): string | null => {
  if (!workspaceRoot) {
    return null;
  }
  const nodeModules = join(workspaceRoot, "node_modules");
  if (!isInside(nodeModules, fallowBinary)) {
    return null;
  }
  // The platform package manifest gives the version of the native binary.
  // The `.bin` launcher has no manifest, so use the wrapper package version.
  const fallowVersion =
    readVersion(join(dirname(fallowBinary), "package.json")) ??
    readVersion(join(nodeModules, "fallow", "package.json"));
  const companionDir = join(nodeModules, COMPANION_PACKAGE);
  if (!fallowVersion || readVersion(join(companionDir, "package.json")) !== fallowVersion) {
    return null;
  }
  const companion = join(companionDir, COMPANION_FILE);
  return existsSync(companion) ? companion : null;
};

export interface TypeAwareEnvironmentOptions {
  readonly bundled?: string | null;
  readonly env?: NodeJS.ProcessEnv;
  readonly platform?: NodeJS.Platform;
  readonly execPath?: string;
}

/**
 * Environment entries that point a Fallow child process at its type-aware
 * companion. A user-set `FALLOW_TYPE_AWARE_BIN` wins. Next is a version-matched
 * project companion for a project-local binary. Last is the bundled companion.
 */
export const typeAwareEnvironment = (
  fallowBinary: string,
  workspaceRoot: string | null,
  {
    bundled = bundledCompanion,
    env = process.env,
    platform = process.platform,
    execPath = process.execPath,
  }: TypeAwareEnvironmentOptions = {},
): Readonly<Record<string, string>> => {
  if (env[TYPE_AWARE_ENV]?.trim()) {
    return {};
  }
  const project = projectTypeAwareCompanion(fallowBinary, workspaceRoot);
  const companion = project ?? bundled;
  if (!companion) {
    return {};
  }
  const command = typeAwareCommand(companion, platform, execPath);
  return {
    [TYPE_AWARE_ENV]: command.binary,
    ...(command.script ? { [TYPE_AWARE_SCRIPT_ENV]: command.script } : {}),
    ...(project ? { [TYPE_AWARE_SOURCE_ENV]: PROJECT_COMPANION_SOURCE } : {}),
  };
};
