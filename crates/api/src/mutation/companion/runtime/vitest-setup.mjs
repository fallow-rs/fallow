// Generated from tools/mutation-runner/src/runtime/vitest-setup.mjs. Do not edit.
import { afterEach, beforeEach, inject } from "vitest";

import { installFallowMutation } from "./global.mjs";

const ACTIVE_MUTANT_KEY = "fallowActiveMutant";
const OVERLAY_KEY = "fallowOverlay";
const SUBPROCESS_PRELOAD_KEY = "fallowSubprocessPreload";
const REQUIRE_HOOK = Symbol.for("fallow.mutation.vitestRequireHook");

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

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

/**
 * Sets the subprocess preload in NODE_OPTIONS with the active mutant, so a
 * Node process that a test starts (spawn, exec, execFile or fork) loads the
 * instrumented files and runs the same mutant. The function first removes
 * the flag of an earlier file or run in this worker, because a worker that
 * does not isolate its test files keeps its environment. The preload URL has
 * no space or quote, so the NODE_OPTIONS parse keeps it as one argument. The
 * existing value comes first, so a loader of the project keeps its order.
 *
 * Without the synchronous hook API, the preload cannot serve a file, so the
 * value stays as it is.
 */
const shareWithChildProcesses = (hooksReady, active) => {
  const preload = inject(SUBPROCESS_PRELOAD_KEY);
  if (!hooksReady || typeof preload !== "string") return;
  const url = new URL(preload);
  const base = preload.split("?", 1)[0];
  const earlier = new RegExp(`(?:^|\\s+)--import=${escapeRegExp(base)}(?:\\?\\S*)?(?=\\s|$)`, "gu");
  const current = (process.env.NODE_OPTIONS ?? "").replace(earlier, "").trim();
  if (active !== null) url.searchParams.set("active", active);
  const flag = `--import=${url.href}`;
  process.env.NODE_OPTIONS = current ? `${current} ${flag}` : flag;
};

// The setup file runs before the test file imports any source module. The
// active mutant is set here at the top level, so a mutant in module scope
// (a static mutant) is active when the module evaluates.
const mutation = installFallowMutation();
mutation.startFile();
mutation.setActive(activeMutant());
const requireHook = await installRequireHook();
shareWithChildProcesses(requireHook.hooksReady, activeMutant());

beforeEach(() => {
  const active = activeMutant();
  mutation.setActive(active);
  shareWithChildProcesses(requireHook.hooksReady, active);
  mutation.startTest();
});

afterEach((context) => {
  const { hits, staticHits } = mutation.finishTest();
  context.task.meta.fallowHits = hits;
  context.task.meta.fallowStaticHits = staticHits;
  context.task.meta.fallowLoaded = [...requireHook.loaded];
});
