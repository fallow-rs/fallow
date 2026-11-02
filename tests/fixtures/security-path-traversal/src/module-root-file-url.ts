// Positive (#3711): `fileURLToPath` of an unknown value is not trusted.
import * as path from "node:path";
import { fileURLToPath } from "node:url";

export function resolveFileUrl(fileUrl: string): string {
  return path.resolve(fileURLToPath(fileUrl), "..");
}

// This module reads request input, so its `path.join` / `path.resolve` calls
// pass the same-module source gate.
export const requestUrl = (req: { url: string }): string => req.url;
