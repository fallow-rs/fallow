import * as path from "node:path";

export function resolveUpload(baseDir: string, userPath: string): string {
  const resolved = path.resolve(baseDir, userPath);
  const relative = path.relative(baseDir, resolved);

  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error("path escapes base directory");
  }

  return resolved;
}
