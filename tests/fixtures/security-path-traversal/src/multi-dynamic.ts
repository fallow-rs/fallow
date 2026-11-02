// Positive (#3712): two dynamic segments in one call produce one candidate.
import * as path from "node:path";

export const joinBoth = (base: string, input: string): string => path.join(base, input);

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
