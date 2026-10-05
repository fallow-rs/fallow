import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

writeFileSync(new URL("../out/data.json", import.meta.url), "{}");

export const gone = existsSync(new URL("./removed.ts", import.meta.url));

export const entry = fileURLToPath(new URL("./runner.mjs", import.meta.url));

export const source = readFileSync(new URL("./helper.ts", import.meta.url), "utf8");

export const worker = new Worker(new URL("./missing-worker.js", import.meta.url));
