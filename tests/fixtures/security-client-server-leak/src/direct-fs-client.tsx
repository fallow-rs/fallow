"use client";
// Direct server-only case: the client file ITSELF imports node:fs (a server-only
// package) with no intermediate module, so the BFS direct-server-only branch must
// emit exactly one server-only-import finding for this file. It ALSO transitively
// reaches a server-only module (headers-util -> next/headers), so the dedupe gate
// must keep it at exactly one finding (direct wins, transitive emit suppressed).
import { readFileSync } from "node:fs";
import { readSession } from "./headers-util";
export function ConfigView(): string {
  return readSession() ?? readFileSync("/etc/app.conf", "utf8");
}
