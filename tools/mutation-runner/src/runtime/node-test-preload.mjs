import { readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { afterEach, beforeEach } from "node:test";
import { fileURLToPath } from "node:url";

import { installChildHooks } from "./child-hooks.mjs";
import { CHILD_ENV, HITS_DIAGNOSTIC, readOverlay, realPath, takeChildEnv } from "./child.mjs";

// The node:test adapter starts one Node process for each test file with
// `--import` of this file. It runs before the test file loads.
const overlay = readOverlay(takeChildEnv(CHILD_ENV.overlay));
const active = takeChildEnv(CHILD_ENV.active);
const loaded = new Set();

const overlayEntry = (url) => {
  if (!url.startsWith("file:")) return undefined;
  return overlay.get(realPath(fileURLToPath(url)));
};

// A synchronous hook runs in this thread for `import` and `require`. It keeps
// the format of the original file, so Node strips the types of a TypeScript
// file as it does without the overlay.
registerHooks({
  load(url, context, nextLoad) {
    const result = nextLoad(url, context);
    const entry = overlayEntry(url);
    if (entry === undefined) return result;
    loaded.add(entry.source);
    return { ...result, source: readFileSync(entry.target, "utf8"), shortCircuit: true };
  },
});

// The diagnostic of a test follows its result event, with the same file,
// line and column. The adapter joins the two.
installChildHooks({
  active,
  beforeEach,
  afterEach,
  report: (row, context) => {
    context.diagnostic(`${HITS_DIAGNOSTIC}${JSON.stringify({ ...row, loaded: [...loaded] })}`);
  },
});
