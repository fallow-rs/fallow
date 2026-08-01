"use server";
import { readFileSync } from "node:fs";

// Control for issue #2074: a "use server" file that ALSO imports node:fs and
// leaks it through a non-action export. The directive does not shield the
// server-only IMPORT, so this module stays a server-only sink.
export async function saveAudit(entry: string): Promise<string> {
  return entry;
}

export const auditTemplate = readFileSync("/etc/audit.tmpl", "utf8");
