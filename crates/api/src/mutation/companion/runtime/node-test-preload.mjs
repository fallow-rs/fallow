// Generated from tools/mutation-runner/src/runtime/node-test-preload.mjs. Do not edit.
import { afterEach, beforeEach } from "node:test";

import { createChildCoverage } from "./child-coverage.mjs";
import { installChildHooks } from "./child-hooks.mjs";
import {
  CHILD_ENV,
  HITS_DIAGNOSTIC,
  readOverlay,
  recordOverlayError,
  takeChildEnv,
} from "./child.mjs";
import { registerOverlayHook } from "./overlay-hook.mjs";
import { setSubprocessPreload } from "./subprocess-options.mjs";

const SUBPROCESS_PRELOAD = new URL("./node-subprocess-preload.mjs", import.meta.url);

/**
 * Gives the child coverage of a dry run to the test hooks. Before each test,
 * the subprocess preload in NODE_OPTIONS gets the token of the test, so the
 * Node children of the test give their hits to it. The paths that a child
 * loads join `loaded`. A mutant run reads no hits, so it needs none.
 */
const childHooks = (preload, overlayPath, loaded) => {
  const coverage = createChildCoverage(overlayPath);
  return {
    startTest: () => setSubprocessPreload(preload, { test: coverage.startTest() }),
    finishTest: () => {
      const children = coverage.finishTest();
      for (const source of children.loaded) loaded.add(source);
      return children.hits;
    },
  };
};

const start = (overlayPath) => {
  const active = takeChildEnv(CHILD_ENV.active);
  const loaded = new Set();
  registerOverlayHook(
    readOverlay(overlayPath),
    (entry) => loaded.add(entry.source),
    (message) => recordOverlayError(overlayPath, message),
  );
  // A Node process that a test starts (spawn, exec, execFile or fork) also
  // loads the instrumented files with the active mutant.
  const preload = new URL(SUBPROCESS_PRELOAD);
  preload.searchParams.set("overlay", overlayPath);
  if (active !== null) preload.searchParams.set("active", active);
  setSubprocessPreload(preload.href, {});
  // The diagnostic of a test follows its result event, with the same file,
  // line and column. The adapter joins the two.
  installChildHooks({
    active,
    beforeEach,
    afterEach,
    children: active === null ? childHooks(preload.href, overlayPath, loaded) : null,
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
