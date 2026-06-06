import * as crypto from "node:crypto";

export function digest(data: string): string {
  return crypto.createHash("md5").update(data).digest("hex");
}
