// A helper that a request handler calls. This module does not read request
// input itself, but its direct importer does, so `path.join` here is a
// path-traversal candidate.
import * as path from "node:path";

export const joinUnderRoot = (input: string): string => path.join("/srv", input);
