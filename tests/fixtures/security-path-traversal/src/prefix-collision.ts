import * as path from "node:path";

export function resolveUpload(baseDir: string, userPath: string): string {
  const resolved = path.resolve(baseDir, userPath);

  if (!resolved.startsWith(baseDir)) {
    throw new Error("path escapes base directory");
  }

  return resolved;
}
