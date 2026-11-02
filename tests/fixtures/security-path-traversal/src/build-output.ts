// Negative: build code joins paths from options and the environment. No
// untrusted request input reaches this module, so `path.join` is not a
// path-traversal candidate.
import * as path from "node:path";

export const outputDir = (root: string): string =>
  path.join(root, process.env.OUT_DIR ?? "dist");
