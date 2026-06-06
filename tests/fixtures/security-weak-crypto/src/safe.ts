// Negative: a strong literal algorithm must not produce a weak-crypto candidate.
import * as crypto from "node:crypto";

export function digestStatic(data: string): string {
  return crypto.createHash("sha256").update(data).digest("hex");
}
