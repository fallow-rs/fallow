// Positive: crypto.createCipher / createDecipher are deprecated (single-pass MD5
// key derivation, no IV). Anchored on the non-literal key argument (index 1).
import * as crypto from "node:crypto";

export function encrypt(password: string, data: string): string {
  const cipher = crypto.createCipher("aes-192", password);
  return cipher.update(data, "utf8", "hex") + cipher.final("hex");
}

export function decrypt(password: string, payload: string): string {
  const decipher = crypto.createDecipher("aes-192", password);
  return decipher.update(payload, "hex", "utf8") + decipher.final("utf8");
}
