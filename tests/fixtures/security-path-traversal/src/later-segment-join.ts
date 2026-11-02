// Positive (#3712): a dynamic later `path.join` segment must fire.
import * as path from "node:path";

export const joinSecond = (input: string): string => path.join(".", input);

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
