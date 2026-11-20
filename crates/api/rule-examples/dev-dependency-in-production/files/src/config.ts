import { readFileSync } from "node:fs";
import { parse } from "yaml";

export const loadConfig = (): Record<string, unknown> =>
  parse(readFileSync("shop.yaml", "utf8"));
