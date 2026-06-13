// Importing node:child_process marks this module server-only.
import { execSync } from "node:child_process";
export function runTool(): string {
  return execSync("uname").toString();
}
