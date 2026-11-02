// Positive (#3711 and #3712): a trusted base does not contain a dynamic later
// segment, so caller input after `__dirname` must fire.
import * as path from "node:path";

export const resolveUnderModule = (input: string): string => path.join(__dirname, input);

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
