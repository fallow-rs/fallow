// Generated from tools/mutation-runner/src/runtime/vitest-setup.mjs. Do not edit.
import { afterEach, beforeEach, inject, vi } from "vitest";

import { installFallowMutation } from "./global.mjs";
import { RECORD_LIMIT, encodeArgs } from "./replay-value.mjs";
import { setSubprocessPreload } from "./subprocess-options.mjs";
import { captureWorkerState, workerStateChanged } from "./worker-state.mjs";

const ACTIVE_MUTANT_KEY = "fallowActiveMutant";
const LOOP_BUDGET_KEY = "fallowLoopBudget";
const OVERLAY_KEY = "fallowOverlay";
const SUBPROCESS_PRELOAD_KEY = "fallowSubprocessPreload";
const REQUIRE_HOOK = Symbol.for("fallow.mutation.vitestRequireHook");
const FRESH_MODULES_KEY = "fallowFreshModules";
const RECORD_ARGS_KEY = "fallowRecordArgs";

const activeMutant = () => {
  const value = inject(ACTIVE_MUTANT_KEY);
  return typeof value === "string" ? value : null;
};

/**
 * Loads the Node APIs of the require hook. Browser mode has no Node modules,
 * and a runtime without the synchronous hook API (Node older than 22.15)
 * gives null.
 */
const loadHookApis = async () => {
  if (typeof globalThis.process?.versions?.node !== "string") return null;
  try {
    const [{ registerHooks }, { readFileSync, realpathSync }, { fileURLToPath }] =
      await Promise.all([import("node:module"), import("node:fs"), import("node:url")]);
    if (typeof registerHooks !== "function") return null;
    return { registerHooks, readFileSync, realpathSync, fileURLToPath };
  } catch {
    return null;
  }
};

/** Gives the overlay entry `{ source, target }` of a file URL, or undefined. */
const overlayEntry = (apis, overlay, url) => {
  if (!url.startsWith("file:")) return undefined;
  const path = apis.fileURLToPath(url);
  let key = path;
  try {
    key = apis.realpathSync(path);
  } catch {
    // A file that does not exist keeps its path. It has no overlay entry.
  }
  return Object.hasOwn(overlay, key) ? overlay[key] : undefined;
};

/**
 * Serves the instrumented copy of a file that a native `require` loads, for
 * example through `createRequire`. The Vite overlay plugin of the adapter
 * never sees such a load, and a file that Vite serves never reaches this
 * hook. Without the hook, the original file loads and its path stays
 * unloaded.
 *
 * The hook state lives on `globalThis`, so a worker that runs more than one
 * test file (`isolate: false`) registers the hook once, and each setup run
 * replaces the overlay.
 */
const installRequireHook = async () => {
  const state = (globalThis[REQUIRE_HOOK] ??= {
    overlay: {},
    loaded: new Set(),
    ready: false,
    hooksReady: false,
  });
  const overlay = inject(OVERLAY_KEY);
  state.overlay = overlay !== null && typeof overlay === "object" ? overlay : {};
  if (state.ready) return state;
  state.ready = true;
  const apis = await loadHookApis();
  if (apis === null) return state;
  state.hooksReady = true;
  apis.registerHooks({
    load(url, context, nextLoad) {
      const result = nextLoad(url, context);
      const entry = overlayEntry(apis, state.overlay, url);
      if (entry === undefined) return result;
      state.loaded.add(entry.source);
      return { ...result, source: apis.readFileSync(entry.target, "utf8"), shortCircuit: true };
    },
  });
  return state;
};

/**
 * Sets the subprocess preload in NODE_OPTIONS with the active mutant, so a
 * Node process that a test starts (spawn, exec, execFile or fork) loads the
 * instrumented files and runs the same mutant. `test` is the token of the
 * child coverage in a dry run, or null. A worker that does not isolate its
 * test files keeps its environment, so each call replaces the flag of an
 * earlier file or run.
 *
 * Without the synchronous hook API, the preload cannot serve a file, so the
 * value stays as it is.
 */
const shareWithChildProcesses = (hooksReady, active, test) => {
  const preload = inject(SUBPROCESS_PRELOAD_KEY);
  if (!hooksReady || typeof preload !== "string") return;
  setSubprocessPreload(preload, { active, test });
};

/**
 * Loads the child coverage of this test file, or null without the synchronous
 * hook API. A Node child of a test then has no overlay and no hits to give.
 */
const loadChildCoverage = async (hooksReady) => {
  const preload = inject(SUBPROCESS_PRELOAD_KEY);
  if (!hooksReady || typeof preload !== "string") return null;
  const overlayPath = new URL(preload).searchParams.get("overlay");
  if (overlayPath === null) return null;
  const { createChildCoverage } = await import("./child-coverage.mjs");
  return createChildCoverage(overlayPath);
};

// The setup file runs before the test file imports any source module. The
// active mutant is set here at the top level, so a mutant in module scope
// (a static mutant) is active when the module evaluates.
const WORKER_BASELINE = Symbol.for("fallow.mutation.workerBaseline");
// Global keys that the runtime of the adapter changes itself.
const OWN_GLOBALS = new Set([WORKER_BASELINE, REQUIRE_HOOK, "__fallowMutation"]);

/**
 * True when a test of an earlier run of this worker process left state
 * outside the modules: a global, a prototype, the environment, a listener,
 * the working directory, the exit code or a timer. The first test of the
 * process records the state, after each setup file of the project ran.
 */
const workerPolluted = () => {
  const baseline = globalThis[WORKER_BASELINE];
  if (baseline === undefined) {
    Object.defineProperty(globalThis, WORKER_BASELINE, {
      value: captureWorkerState(),
      configurable: true,
    });
    return false;
  }
  return workerStateChanged(baseline, OWN_GLOBALS);
};

/**
 * Removes the overlay files from the CommonJS cache of the process, so that a
 * native `require` of a reused worker loads them again with the current
 * mutant. `vi.resetModules()` resets only the modules of Vitest.
 */
const clearRequireCache = () => {
  // Browser mode has no Node modules, so the import stays dynamic.
  const createRequire = globalThis.process?.getBuiltinModule?.("node:module")?.createRequire;
  if (typeof createRequire !== "function") return;
  const cache = createRequire(import.meta.url).cache;
  const overlay = inject(OVERLAY_KEY);
  if (overlay === null || typeof overlay !== "object") return;
  for (const [source, entry] of Object.entries(overlay)) {
    delete cache[source];
    if (typeof entry?.target === "string") delete cache[entry.target];
  }
};

// A worker that the adapter keeps for more than one run still has the
// modules of the earlier runs. Each test file then loads them again.
const freshModules = inject(FRESH_MODULES_KEY) === true;
if (freshModules) {
  vi.resetModules();
  clearRequireCache();
}
const mutation = installFallowMutation();
mutation.startFile();
mutation.setActive(activeMutant());
// The recorder starts before the test file imports a source module, so each
// exported function gets its wrap. Other runs leave the functions as they
// are.
const recordArgs = inject(RECORD_ARGS_KEY) === true;
if (recordArgs) mutation.startRecording(encodeArgs, RECORD_LIMIT);
else mutation.stopRecording();
const requireHook = await installRequireHook();
shareWithChildProcesses(requireHook.hooksReady, activeMutant(), null);
const childCoverage = await loadChildCoverage(requireHook.hooksReady);

let pollutedAtStart = null;

beforeEach(() => {
  pollutedAtStart ??= freshModules && workerPolluted();
  const active = activeMutant();
  mutation.setActive(active);
  mutation.setLoopBudget(inject(LOOP_BUDGET_KEY));
  // Only a dry run reads hits, so a mutant run gives the children no token.
  const token = childCoverage?.startTest() ?? null;
  shareWithChildProcesses(requireHook.hooksReady, active, active === null ? token : null);
  mutation.startTest();
});

// Vitest reads the first argument of a hook for the fixtures of a
// `test.extend` test, and fails such a test when the argument is not an
// object pattern.
afterEach(({ task }) => {
  const { hits, staticHits } = mutation.finishTest();
  const children = childCoverage?.finishTest() ?? { hits: [], loaded: [] };
  task.meta.fallowHits = [...new Set([...hits, ...children.hits])];
  task.meta.fallowStaticHits = staticHits;
  task.meta.fallowLoaded = [...new Set([...requireHook.loaded, ...children.loaded])];
  if (recordArgs) task.meta.fallowArgs = mutation.takeRecords();
  if (freshModules) task.meta.fallowPolluted = pollutedAtStart || workerPolluted();
});
