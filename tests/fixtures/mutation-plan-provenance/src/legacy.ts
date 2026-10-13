const https = require("node:https");

export function legacyDownload(url: string): void {
  if (url.length > 0) {
    https.get(url, { rejectUnauthorized: true });
  }
}
