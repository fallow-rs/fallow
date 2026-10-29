// Positive (#3711): a caller-controlled URL resolved against `import.meta.url`
// is not a trusted module-relative path and must still fire.
import * as path from "node:path";
import { fileURLToPath } from "node:url";

export function resolveAsset(userInput: string): string {
  return path.resolve(fileURLToPath(new URL(userInput, import.meta.url)), "..");
}
