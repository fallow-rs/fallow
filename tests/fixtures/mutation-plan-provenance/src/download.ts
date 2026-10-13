import https from "node:https";
import { get } from "node:https";

export function download(url: string): void {
  if (url.length > 0) {
    https.get(url, { rejectUnauthorized: true });
  }
}

export function fetchNamed(url: string): void {
  if (url.length > 0) {
    get(url, { rejectUnauthorized: true });
  }
}
