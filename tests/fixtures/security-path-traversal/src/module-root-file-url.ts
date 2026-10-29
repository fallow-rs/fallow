// Positive (#3711): `fileURLToPath` of an unknown value is not trusted.
import * as path from "node:path";
import { fileURLToPath } from "node:url";

export function resolveFileUrl(fileUrl: string): string {
  return path.resolve(fileURLToPath(fileUrl), "..");
}
