"use server";
import { readFileSync } from "node:fs";

// Control for issues #2074 and #2941: a "use server" file that ALSO imports
// node:fs. The non-action export below ships to the client, so this module is
// not a Server Action boundary and its server-only import stays a sink.
export async function saveAudit(entry: string): Promise<string> {
  return entry;
}

export const auditTemplate = readFileSync("/etc/audit.tmpl", "utf8");
