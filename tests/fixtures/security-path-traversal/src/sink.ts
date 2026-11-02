// Positive: a non-literal path component passed to path.join (imported from
// node:path) is a path-traversal candidate (CWE-22), provenance-gated to node:path.
import * as path from "node:path";

export function resolveUpload(userBase: string): string {
  return path.join(userBase, "avatar.png");
}

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
