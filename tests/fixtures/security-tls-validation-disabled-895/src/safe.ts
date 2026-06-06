import * as https from "node:https";
import * as tls from "node:tls";

https.request("https://example.com", {
  rejectUnauthorized: true,
});

tls.connect({
  host: "example.com",
  rejectUnauthorized: true,
});

process.env.NODE_TLS_REJECT_UNAUTHORIZED = "1";
