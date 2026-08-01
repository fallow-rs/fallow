"use server";
import { readFileSync } from "node:fs";

// Control for issue #2074: a "use server" file that ALSO imports node:fs.
// The sink predicate is module-level (it does not inspect export shape), so
// the server-only import keeps this module a sink no matter what it exports.
// The non-action export below is the leak shape that makes such a report real.
export async function saveAudit(entry: string): Promise<string> {
  return entry;
}

export const auditTemplate = readFileSync("/etc/audit.tmpl", "utf8");
