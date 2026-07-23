import { existsSync } from "node:fs";
import { join } from "node:path";

const TYPE_AWARE_ENV = "FALLOW_TYPE_AWARE_BIN";

/** Make the companion bundled in the extension available to every Fallow child process. */
export const configureBundledTypeAwareCompanion = (extensionPath: string): string | null => {
  const configured = process.env[TYPE_AWARE_ENV]?.trim();
  if (configured) {
    return configured;
  }

  const bundled = join(extensionPath, "dist", "type-aware", "fallow-type-aware.mjs");
  if (!existsSync(bundled)) {
    return null;
  }

  process.env[TYPE_AWARE_ENV] = bundled;
  return bundled;
};
