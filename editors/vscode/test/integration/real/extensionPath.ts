import { realpathSync } from "node:fs";
import { resolve } from "node:path";

/** Resolve the exact extension root that the real-host smoke must load. */
export const resolveExtensionDevelopmentPath = (
  repositoryPath: string,
  configuredPath: string | undefined,
): string => realpathSync(resolve(configuredPath ?? repositoryPath));
