import { createServer, type Server } from "node:http";
import { buildInspectorCard, type InspectorCard, type Selection } from "./inspect";
import { appendFeedItem } from "./feed";
import type { WalkthroughDocument } from "../model/walkthrough";

export const INSPECT_PORT = 7787;
const SELECT_PATH = "/fallow-select";

const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "POST, OPTIONS",
  "access-control-allow-headers": "content-type",
};

/**
 * Localhost bridge: the in-page picker POSTs a {@link Selection}; we enrich it
 * with grounded facts, push the card to the renderer, and log it to the feed.
 */
export const startInspectServer = (
  getDoc: () => WalkthroughDocument | null,
  send: (card: InspectorCard) => void,
  root: string,
  port: number = INSPECT_PORT,
): Server => {
  const server = createServer((req, res) => {
    if (req.method === "OPTIONS") {
      res.writeHead(204, CORS).end();
      return;
    }
    if (req.method !== "POST" || req.url !== SELECT_PATH) {
      res.writeHead(404, CORS).end();
      return;
    }
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      void (async () => {
        try {
          const sel = JSON.parse(body) as Selection;
          const card = buildInspectorCard(getDoc(), sel);
          send(card);
          await appendFeedItem(root, {
            target: { kind: "component", value: sel.component ?? `${sel.file}:${sel.line}` },
            note: `inspected ${sel.component ?? sel.file}`,
            at: new Date().toISOString(),
          });
          res
            .writeHead(200, { "content-type": "application/json", ...CORS })
            .end(JSON.stringify(card));
        } catch (err) {
          res.writeHead(400, CORS).end(String(err));
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
