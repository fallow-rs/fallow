// Generated from tools/mutation-runner/src/runtime/bun-test-preload.mjs. Do not edit.
import { appendFileSync, readFileSync } from "node:fs";
import { extname } from "node:path";

import { plugin } from "bun";
import { afterEach, beforeEach } from "bun:test";

import { installChildHooks } from "./child-hooks.mjs";
import { CHILD_ENV, escapeRegExp, readOverlay, realPath, takeChildEnv } from "./child.mjs";

// The bun-test adapter starts one `bun test` process for each test file with
// `--preload` of this file. It runs before the test file loads.
const overlay = readOverlay(takeChildEnv(CHILD_ENV.overlay));
const active = takeChildEnv(CHILD_ENV.active);
const hitsFile = takeChildEnv(CHILD_ENV.hits);
const loaded = new Set();

const LOADERS = { ".tsx": "tsx", ".jsx": "jsx", ".js": "js", ".mjs": "js", ".cjs": "js" };

// The filter names each overlay file by its real path and by the path in the
// map, so that no other file goes through the hook.
const overlayPaths = [
  ...new Set([...overlay].flatMap(([real, entry]) => [real, entry.path ?? real])),
];
if (overlayPaths.length > 0) {
  const filter = new RegExp(`^(?:${overlayPaths.map(escapeRegExp).join("|")})$`, "u");
  plugin({
    name: "fallow-mutation-overlay",
    setup(build) {
      build.onLoad({ filter }, (args) => {
        const entry = overlay.get(realPath(args.path));
        loaded.add(entry.source);
        return {
          contents: readFileSync(entry.target, "utf8"),
          loader: LOADERS[extname(args.path)] ?? "ts",
        };
      });
    },
  });
}

// A Bun hook gets no test context. The adapter joins the lines to the test
// results by their order.
installChildHooks({
  active,
  beforeEach,
  afterEach,
  report: (row) => {
    if (hitsFile === null) return;
    appendFileSync(hitsFile, `${JSON.stringify({ ...row, loaded: [...loaded] })}\n`);
  },
});
