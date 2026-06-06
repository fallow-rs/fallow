import * as tls from "node:tls";

export function connectWithTlsValidationDisabled(): void {
  tls.connect({
    host: "example.com",
    rejectUnauthorized: false,
  });
}
