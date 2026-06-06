import * as crypto from "node:crypto";
import { createDecipheriv } from "node:crypto";

declare const key: Buffer;
declare const iv: Buffer;

export function weakEcb(): void {
  crypto.createCipheriv("aes-128-ecb", key, iv);
  createDecipheriv("AES-256-ECB", key, iv);
}
