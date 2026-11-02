// Positive (#3712): a literal first segment does not contain a dynamic later
// segment, so the later argument must fire.
import * as path from "node:path";

export const resolveSecond = (input: string): string => path.resolve(".", input);

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
