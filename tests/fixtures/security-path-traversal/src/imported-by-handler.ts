// Negative: this module does not read request input itself. Only an importer
// does, and the value reaches it through a call the scan does not follow, so
// `path.join` here is not a candidate.
import * as path from "node:path";

export const joinUnderRoot = (input: string): string => path.join("/srv", input);
