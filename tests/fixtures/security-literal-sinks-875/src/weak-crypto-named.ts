import { createHash } from "node:crypto";

export function digestNamed(data: string): string {
  return createHash("sha1").update(data).digest("hex");
}
