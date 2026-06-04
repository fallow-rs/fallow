// Positive: a non-literal length passed to crypto.pseudoRandomBytes (imported
// from node:crypto) is an insecure-randomness candidate (CWE-338).
import * as crypto from "node:crypto";

export function weakToken(size: number): Buffer {
  return crypto.pseudoRandomBytes(size);
}
