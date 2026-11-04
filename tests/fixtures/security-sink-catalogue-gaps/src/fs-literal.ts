import { readFileSync } from "node:fs";

export function load(): string {
  return readFileSync("./config.json", "utf8");
}
