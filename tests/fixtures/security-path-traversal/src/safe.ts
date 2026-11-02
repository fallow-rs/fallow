// Negative (literal): a fully-literal path.join is never captured (every argument
// is a string literal), so it must NOT produce a path-traversal candidate.
import * as path from "node:path";

export function resolveStatic(): string {
  return path.join("/var/uploads", "avatar.png");
}

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
