// Negative: two imports away from the request handler. Only a direct importer
// arms the `path.join` gate, so this build code is not a candidate.
import * as path from "node:path";

export const outputFile = (name: string): string => path.join("dist", name);
