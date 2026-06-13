import { readFileSync } from "node:fs";
export function loadConfig(): string {
  return readFileSync("/etc/app.conf", "utf8");
}
