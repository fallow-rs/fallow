export interface CommonScopeArgs {
  readonly changedSince: string;
  readonly workspace?: string;
  readonly configPath: string;
}

/** A CLI option omitted because the resolved binary predates it. */
export interface SkippedCliCapability {
  readonly flag: string;
  readonly requires: string;
  readonly cliVersion: string;
}

export interface BuiltCliArgs {
  readonly args: string[];
  readonly skipped: readonly SkippedCliCapability[];
}

/**
 * Compare dotted numeric versions. Pre-release suffixes are intentionally
 * ignored because the CLI version probe returns the numeric release core.
 */
export const compareVersions = (a: string, b: string): number => {
  const parse = (version: string): number[] =>
    version.split(".").map((segment) => Number.parseInt(segment, 10) || 0);
  const left = parse(a);
  const right = parse(b);
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
};

/** Narrow a parsed CLI JSON envelope to the structured-error shape. */
export const isStructuredError = (value: unknown): value is { error: true; message?: string } =>
  typeof value === "object" &&
  value !== null &&
  "error" in value &&
  (value as { error: unknown }).error === true;

/** `--allow-remote-extends` first ships in CLI v3.4.2. */
export const ALLOW_REMOTE_EXTENDS_MIN_CLI_VERSION = "3.4.2";

/**
 * Append `--allow-remote-extends` when the `fallow.allowRemoteExtends`
 * setting is on, so a CLI run reads a config with an `https://` extends entry
 * as the language server does. Returns the skipped capability when the
 * resolved CLI predates the flag, and `null` otherwise.
 */
export const appendAllowRemoteExtends = (
  args: string[],
  allowRemoteExtends: boolean,
  cliVersion: string | null,
): SkippedCliCapability | null => {
  if (!allowRemoteExtends) {
    return null;
  }
  const requires = ALLOW_REMOTE_EXTENDS_MIN_CLI_VERSION;
  if (cliVersion !== null && compareVersions(cliVersion, requires) < 0) {
    return { flag: "--allow-remote-extends", requires, cliVersion };
  }
  args.push("--allow-remote-extends");
  return null;
};

/** Build the argument vector of the `fallow workspaces` run of the workspace picker. */
export const buildWorkspacesArgs = (
  allowRemoteExtends: boolean,
  cliVersion: string | null,
): BuiltCliArgs => {
  const args = ["workspaces", "--format", "json", "--quiet"];
  const skipped = appendAllowRemoteExtends(args, allowRemoteExtends, cliVersion);
  return { args, skipped: skipped ? [skipped] : [] };
};

/** Append the project-scope flags shared by every analysis command. */
export const appendCommonScopeArgs = (args: string[], options: CommonScopeArgs): void => {
  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }
  if (options.workspace) {
    args.push("--workspace", options.workspace);
  }
  if (options.configPath) {
    args.push("--config", options.configPath);
  }
};
