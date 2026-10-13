import { GLOBAL_NAME } from "../generated-protocol.mjs";

const CONTROL = Symbol.for("fallow.mutation.control");

const createRuntime = () => {
  // `running` counts the tests that started and did not finish. Concurrent
  // tests overlap, so a hit belongs to a test while any test runs.
  const state = { active: null, running: 0, hits: new Set(), staticHits: new Set() };
  // `a` and `c` run inside user code. They read no property of `id`, and a
  // failure inside them must never change the result of a test.
  const runtime = {
    a: (id) => {
      try {
        return state.active !== null && state.active === id;
      } catch {
        return false;
      }
    },
    c: (id) => {
      try {
        (state.running === 0 ? state.staticHits : state.hits).add(id);
      } catch {
        // A hit that cannot be recorded is lost. The test result stays.
      }
    },
  };
  const control = {
    setActive: (id) => {
      state.active = typeof id === "string" ? id : null;
    },
    // A test that never finished (for example a killed worker) must not
    // turn the module-scope hits of the next file into test hits.
    startFile: () => {
      state.running = 0;
    },
    // A start does not clear `hits`: a concurrent test can still own them.
    // Each finish takes the hits so far, so no hit is lost.
    startTest: () => {
      state.running += 1;
    },
    finishTest: () => {
      const result = { hits: [...state.hits], staticHits: [...state.staticHits] };
      state.running = Math.max(0, state.running - 1);
      state.hits = new Set();
      state.staticHits = new Set();
      return result;
    },
  };
  Object.defineProperty(runtime, CONTROL, { value: control });
  return runtime;
};

/**
 * Installs `globalThis.__fallowMutation` and returns the adapter control
 * object. A second call returns the control object of the installed runtime,
 * so two module instances of this file share one state.
 */
export const installFallowMutation = () => {
  const existing = globalThis[GLOBAL_NAME];
  if (existing?.[CONTROL]) {
    return existing[CONTROL];
  }
  const runtime = createRuntime();
  Object.defineProperty(globalThis, GLOBAL_NAME, {
    value: runtime,
    configurable: true,
    writable: true,
  });
  return runtime[CONTROL];
};
