import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { assertLocalResolution } from "./assert-local-resolution.mjs";
import { runCliMain } from "./cli-main.mjs";

runCliMain(() => {
  const directory = process.argv[2];
  if (!directory || process.argv.length !== 3) {
    throw new Error("usage: node scripts/render-contributors.mjs <output-directory>");
  }
  assertLocalResolution({ dependency: "@resvg/resvg-js" });
  const { Resvg } = createRequire(import.meta.url)("@resvg/resvg-js");
  const svg = readFileSync(join(directory, "contributors.svg"));
  const png = new Resvg(svg, { font: { loadSystemFonts: false } }).render().asPng();
  writeFileSync(join(directory, "contributors.png"), png);
  return 0;
});
