// Negative: fully-literal sink arguments are never captured, so none of the #897
// categories fire here (the conservative prefer-false-negatives trade-off).
import * as crypto from "node:crypto";

export function literalRandom(): Buffer {
  return crypto.pseudoRandomBytes(16);
}

export function literalCipher(data: string): string {
  const cipher = crypto.createCipher("aes-192", "static-password");
  return cipher.update(data, "utf8", "hex") + cipher.final("hex");
}

export function literalBuffer(): Buffer {
  return Buffer.allocUnsafe(64);
}

export function literalSafeString(): string {
  const Handlebars = { SafeString: (value: string): string => value };
  return Handlebars.SafeString("<b>static</b>");
}
