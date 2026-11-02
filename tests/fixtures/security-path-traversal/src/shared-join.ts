// Negative: a request handler and build code both import this module. The
// build importer reads no request input, so `path.join` here is not armed.
import * as path from "node:path";

export const sharedJoin = (name: string): string => path.join("out", name);
