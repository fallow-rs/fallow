import * as path from "node:path";

interface Res {
  sendFile(filePath: string): void;
}

export function sendUpload(res: Res, baseDir: string, userPath: string): void {
  const resolved = path.resolve(baseDir, userPath);
  res.sendFile(resolved);

  const relative = path.relative(baseDir, resolved);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error("path escapes base directory");
  }
}
