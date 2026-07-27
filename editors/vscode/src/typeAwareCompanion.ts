import { existsSync } from "node:fs";
import { join } from "node:path";

const TYPE_AWARE_ENV = "FALLOW_TYPE_AWARE_BIN";
const TYPE_AWARE_SCRIPT_ENV = "FALLOW_TYPE_AWARE_SCRIPT";

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

  const command = typeAwareCommand(bundled);
  process.env[TYPE_AWARE_ENV] = command.binary;
  if (command.script) {
    process.env[TYPE_AWARE_SCRIPT_ENV] = command.script;
  } else {
    delete process.env[TYPE_AWARE_SCRIPT_ENV];
  }
  return bundled;
};
