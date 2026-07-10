import { copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = join(here, "..");
const source = join(root, "types", "index.d.ts");
const target = join(root, "index.d.ts");
const outputRoot = process.env.FALLOW_GENERATION_OUTPUT_ROOT;
const outputTarget = outputRoot ? resolve(outputRoot, "crates", "napi", "index.d.ts") : target;
const check = process.argv.includes("--check");

if (check) {
  const expected = readFileSync(source, "utf8");
  const actual = readFileSync(target, "utf8");
  if (actual !== expected) {
    console.error("crates/napi/index.d.ts is stale; run npm run publish:prepare in crates/napi");
    process.exitCode = 1;
  }
} else {
  mkdirSync(dirname(outputTarget), { recursive: true });
  copyFileSync(source, outputTarget);
}
