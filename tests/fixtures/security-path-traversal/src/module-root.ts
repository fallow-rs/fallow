// Negative (#3711): module-relative paths built only from trusted bases
// (`import.meta.url`, `import.meta.dirname`, `__dirname`, `process.cwd()`) and
// literal segments cannot carry caller input, so they must NOT fire.
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

export const projectRoot = path.resolve(fileURLToPath(new URL(".", import.meta.url)), "..");

const here = path.dirname(fileURLToPath(import.meta.url));

export const dataDir = path.join(here, "data");
export const metaDir = path.join(import.meta.dirname, "..", "fixtures");
export const legacyDir = path.join(__dirname, `assets`);
export const outDir = path.resolve(process.cwd(), "dist");

export function readTemplate(): string {
  return fs.readFileSync(path.join(here, "template.html"), "utf8");
}
