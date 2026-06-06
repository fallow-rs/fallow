import * as https from "node:https";

export function requestWithTlsValidationDisabled(): void {
  https.request("https://example.com", {
    rejectUnauthorized: false,
  });
}
