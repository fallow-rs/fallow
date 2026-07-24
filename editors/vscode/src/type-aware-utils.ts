import type { TypeAwareSettings } from "./config.js";
import { compareVersions, type SkippedCliCapability } from "./cli-args-utils.js";

export type TypeAwareArgsOptions = TypeAwareSettings;

/**
 * Type-aware flags first ship after v3.9.1. The feature branch was reconciled
 * with that release, but was not part of the v3.9.1 tag.
 */
export const TYPE_AWARE_MIN_CLI_VERSION = "3.9.2";

export const TYPE_AWARE_VERSION_GATED_FLAGS = [
  "--type-aware",
  "--type-aware-project",
  "--type-aware-require",
  "--type-coupling",
] as const;

export const appendTypeAwareArgs = (
  args: string[],
  options: TypeAwareArgsOptions | undefined,
  cliVersion: string | null,
  commandFlags: ReadonlyArray<string> = [],
): SkippedCliCapability | null => {
  if (!options?.enabled) {
    return null;
  }

  if (cliVersion !== null && compareVersions(cliVersion, TYPE_AWARE_MIN_CLI_VERSION) < 0) {
    return {
      flag: "--type-aware",
      requires: TYPE_AWARE_MIN_CLI_VERSION,
      cliVersion,
    };
  }

  args.push(...commandFlags);
  args.push("--type-aware");
  for (const project of options.projects) {
    args.push("--type-aware-project", project);
  }
  if (options.require !== "best-effort") {
    args.push("--type-aware-require", options.require);
  }
  return null;
};
