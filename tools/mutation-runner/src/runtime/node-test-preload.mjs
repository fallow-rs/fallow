import { afterEach, beforeEach } from "node:test";

import { installChildHooks } from "./child-hooks.mjs";
import { CHILD_ENV, HITS_DIAGNOSTIC, readOverlay, takeChildEnv } from "./child.mjs";
import { registerOverlayHook } from "./overlay-hook.mjs";

const SUBPROCESS_PRELOAD = new URL("./node-subprocess-preload.mjs", import.meta.url);

/**
 * Adds the subprocess preload to NODE_OPTIONS, so a Node process that a test
 * starts (spawn, exec, execFile or fork) also loads the instrumented files
 * with the active mutant. The value is a URL, which has no space or quote,
 * so the NODE_OPTIONS parse keeps it as one argument. The existing value
 * comes first, so a loader of the project keeps its order.
 */
const shareWithChildProcesses = (overlayPath, active) => {
  const url = new URL(SUBPROCESS_PRELOAD);
  url.searchParams.set("overlay", overlayPath);
  if (active !== null) url.searchParams.set("active", active);
  const flag = `--import=${url.href}`;
  const current = process.env.NODE_OPTIONS?.trim();
  process.env.NODE_OPTIONS = current ? `${current} ${flag}` : flag;
};

const start = (overlayPath) => {
  const active = takeChildEnv(CHILD_ENV.active);
  const loaded = new Set();
  registerOverlayHook(readOverlay(overlayPath), (entry) => loaded.add(entry.source));
  shareWithChildProcesses(overlayPath, active);
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
};

// The node:test adapter starts one Node process for each test file with
// `--import` of this file. It runs before the test file loads. A process that
// a test starts with `fork()` inherits the `--import` but not the overlay
// variable. It is not a test process, so it gets no test hooks here. The
// subprocess preload in NODE_OPTIONS serves its files.
const overlayPath = takeChildEnv(CHILD_ENV.overlay);
if (overlayPath !== null) start(overlayPath);
