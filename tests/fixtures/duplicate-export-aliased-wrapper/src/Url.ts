import { encode as base64 } from "./Base64";

export function encode(value: string): string {
  return encodeURIComponent(base64(value));
}
