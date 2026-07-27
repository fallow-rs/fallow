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
