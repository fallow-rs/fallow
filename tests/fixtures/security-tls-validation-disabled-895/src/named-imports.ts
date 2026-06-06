import { get, request } from "https";
import { connect } from "node:tls";
import * as https from "https";

export function namedHttpsRequest(): void {
  request("https://example.com", {
    rejectUnauthorized: false,
  });
}

export function namedHttpsGet(): void {
  get("https://example.com", {
    rejectUnauthorized: false,
  });
}

export function namespaceHttpsGet(): void {
  https.get("https://example.com", {
    rejectUnauthorized: false,
  });
}

export function namedTlsConnect(): void {
  connect({
    host: "example.com",
    rejectUnauthorized: false,
  });
}
