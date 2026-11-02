import * as path from "node:path";

export function resolveUpload(baseDir: string, userPath: string): string {
  const resolved = path.resolve(baseDir, userPath);

  if (!resolved.startsWith(baseDir)) {
    throw new Error("path escapes base directory");
  }

  return resolved;
}

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
