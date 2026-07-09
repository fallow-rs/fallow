import { createServer, type Server } from "node:http";
import { buildInspectorCard, type InspectorCard, type Selection } from "./inspect";
import { appendFeedItem } from "./feed";
import type { WalkthroughDocument } from "../model/walkthrough";

export const INSPECT_PORT = 7787;
const SELECT_PATH = "/fallow-select";

/** Max accepted request body size for the inspect bridge (bytes). */
const MAX_BODY_BYTES = 64 * 1024;

const LOOPBACK_HOSTNAMES = new Set(["127.0.0.1", "localhost", "::1"]);

const stripBrackets = (hostname: string): string =>
  hostname.startsWith("[") && hostname.endsWith("]") ? hostname.slice(1, -1) : hostname;

/** Strip the `:port` suffix from a `Host` header value, honoring IPv6 brackets. */
const hostnameOf = (host: string): string => {
  if (host.startsWith("[")) {
    const end = host.indexOf("]");
    return end === -1 ? host : host.slice(1, end);
  }
  const colon = host.lastIndexOf(":");
  return colon === -1 ? host : host.slice(0, colon);
};

/**
 * True when the `Host` header names a loopback address. Guards against
 * DNS-rebinding (a browser resolves an attacker-controlled name to
 * 127.0.0.1, then sends that name back as `Host`).
 */
export const isLoopbackHost = (host: string | undefined): boolean => {
  if (!host) return false;
  return LOOPBACK_HOSTNAMES.has(hostnameOf(host));
};

/**
 * True when `origin` is absent (non-browser clients: e2e fetches, curl) or
 * is a loopback `http:`/`https:` origin, any port. False for everything
 * else, including the literal string `"null"` (sent for opaque origins).
 */
export const isAllowedOrigin = (origin: string | undefined): boolean => {
  if (origin === undefined) return true;
  try {
    const url = new URL(origin);
    if (url.protocol !== "http:" && url.protocol !== "https:") return false;
    return LOOPBACK_HOSTNAMES.has(stripBrackets(url.hostname));
  } catch {
    return false;
  }
};

const corsHeaders = (origin: string | undefined): Record<string, string> => ({
  "access-control-allow-origin": origin ?? "*",
  "access-control-allow-methods": "POST, OPTIONS",
  "access-control-allow-headers": "content-type",
});

/**
 * Validate and parse a raw request body into a {@link Selection}. Returns
 * `null` for malformed JSON or any field that violates the expected shape,
 * rather than trusting a blind type assertion.
 */
export const parseSelection = (body: string): Selection | null => {
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const { file, line, column, component } = parsed as Record<string, unknown>;

  if (typeof file !== "string" || file.length === 0 || file.length > 1024) return null;
  if (typeof line !== "number" || !Number.isInteger(line) || line < 0) return null;
  if (
    column !== undefined &&
    (typeof column !== "number" || !Number.isInteger(column) || column < 0)
  ) {
    return null;
  }
  if (component !== undefined && (typeof component !== "string" || component.length > 256)) {
    return null;
  }

  const sel: Selection = { file, line };
  if (column !== undefined) sel.column = column;
  if (component !== undefined) sel.component = component;
  return sel;
};

/**
 * Localhost bridge: the in-page picker POSTs a {@link Selection}; we enrich it
 * with grounded facts, push the card to the renderer, and log it to the feed.
 *
 * Trust model: loopback-only transport (bind + `Host` check, closing
 * DNS-rebinding) AND loopback-only browser origins AND schema-valid payloads.
 * Non-browser clients (no `Origin` header, e.g. the e2e fetches and curl) are
 * allowed by design; CORS headers are meaningless for them.
 */
export const startInspectServer = (
  getDoc: () => WalkthroughDocument | null,
  send: (card: InspectorCard) => void,
  root: string,
  port: number = INSPECT_PORT,
): Server => {
  const server = createServer((req, res) => {
    const origin = req.headers.origin;
    const allowed = isLoopbackHost(req.headers.host) && isAllowedOrigin(origin);

    if (req.method === "OPTIONS") {
      if (!allowed) {
        res.writeHead(403).end();
        return;
      }
      res.writeHead(204, corsHeaders(origin)).end();
      return;
    }

    if (req.method !== "POST" || req.url !== SELECT_PATH) {
      res.writeHead(404).end();
      return;
    }

    if (!allowed) {
      res.writeHead(403).end();
      return;
    }

    let body = "";
    let bytes = 0;
    let rejected = false;
    req.on("data", (chunk: Buffer) => {
      if (rejected) return;
      bytes += chunk.length;
      if (bytes > MAX_BODY_BYTES) {
        rejected = true;
        res.writeHead(413, corsHeaders(origin)).end();
        req.destroy();
        return;
      }
      body += chunk;
    });
    req.on("end", () => {
      if (rejected) return;
      void (async () => {
        const sel = parseSelection(body);
        if (!sel) {
          res.writeHead(400, corsHeaders(origin)).end();
          return;
        }
        try {
          const card = buildInspectorCard(getDoc(), sel);
          send(card);
          await appendFeedItem(root, {
            target: { kind: "component", value: sel.component ?? `${sel.file}:${sel.line}` },
            note: `inspected ${sel.component ?? sel.file}`,
            at: new Date().toISOString(),
          });
          res
            .writeHead(200, { "content-type": "application/json", ...corsHeaders(origin) })
            .end(JSON.stringify(card));
        } catch (err) {
          res.writeHead(400, corsHeaders(origin)).end(String(err));
        }
      })();
    });
  });
  // The inspector bridge is optional; never crash the app if the port is taken
  // (e.g. a second window or a parallel e2e launch).
  server.on("error", () => undefined);
  server.listen(port, "127.0.0.1");
  return server;
};
