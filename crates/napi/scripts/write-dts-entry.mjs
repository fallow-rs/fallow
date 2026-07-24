import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = join(here, "..");
const target = join(root, "index.d.ts");
const outputRoot = process.env.FALLOW_GENERATION_OUTPUT_ROOT;
const outputTarget = outputRoot ? resolve(outputRoot, "crates", "napi", "index.d.ts") : target;
const check = process.argv.includes("--check");
const declarationEntry = 'export * from "./types/index";\n';

if (check) {
  const actual = readFileSync(target, "utf8");
  if (actual !== declarationEntry) {
    console.error("crates/napi/index.d.ts is stale; run npm run publish:prepare in crates/napi");
    process.exitCode = 1;
  }
} else {
  mkdirSync(dirname(outputTarget), { recursive: true });
  writeFileSync(outputTarget, declarationEntry);
}
