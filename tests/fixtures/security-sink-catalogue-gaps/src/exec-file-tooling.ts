// Tooling code runs a computed binary path without untrusted input.
import { execFileSync } from "node:child_process";

export function version(binary: string): string {
  return execFileSync(binary, ["--version"]).toString();
}
