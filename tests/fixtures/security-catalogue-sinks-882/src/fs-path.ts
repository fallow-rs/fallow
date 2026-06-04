// Positive: non-literal paths passed to node:fs methods are path-traversal candidates.
import * as fs from "node:fs";

export function readUserFile(userPath: string): string {
  return fs.readFileSync(userPath, "utf8");
}

export function moveUserFile(targetPath: string): void {
  fs.rename("safe.txt", targetPath, () => {});
}
