// Generated from tools/mutation-runner/src/runtime/global.mjs. Do not edit.
import { GLOBAL_NAME } from "../generated-protocol.mjs";

const CONTROL = Symbol.for("fallow.mutation.control");

/** The `name` of the error that ends a test whose mutant runs past the loop budget. */
export const LOOP_TIMEOUT_NAME = "FallowMutationLoopTimeout";

// `a` reads the clock once per this many evaluations of the active mutant.
// A clock read on each evaluation would slow a hot loop that ends.
const LOOP_CHECK_MASK = 0x7f;

const loopTimeout = (budgetMs) => {
  const error = new Error(
    `Test timed out in ${budgetMs}ms: the active mutant still ran after the loop budget`,
  );
  error.name = LOOP_TIMEOUT_NAME;
  return error;
};

const createRuntime = () => {
  // `running` counts the tests that started and did not finish. Concurrent
  // tests overlap, so a hit belongs to a test while any test runs.
  // `loopBudget` is the time in ms that the tests of one file can run with
  // the active mutant. `loopSpent` is the test time of the earlier tests of
  // the file, and `testStarted` the start of the current test time.
  const state = {
    active: null,
    running: 0,
    hits: new Set(),
    staticHits: new Set(),
    loopBudget: null,
    loopSpent: 0,
    testStarted: 0,
    activeHits: 0,
  };
  // A synchronous endless loop blocks the test worker, so no timer can stop
  // it. When the loop passes through the active mutant, this check ends it
  // with an error that the adapter reports as a timeout. The companion then
  // stays alive. A loop that does not pass through the mutant waits for the
  // watchdog of the API.
  const checkLoop = () => {
    state.activeHits += 1;
    if ((state.activeHits & LOOP_CHECK_MASK) !== 0) return;
    if (state.loopBudget === null || state.running === 0) return;
    if (state.loopSpent + performance.now() - state.testStarted <= state.loopBudget) return;
    throw loopTimeout(state.loopBudget);
  };
  // `a` and `c` run inside user code. They read no property of `id`, and a
  // failure inside them must never change the result of a test. The only
  // error that `a` throws is the loop timeout.
  const runtime = {
    a: (id) => {
      let active;
      try {
        active = state.active !== null && state.active === id;
      } catch {
        return false;
      }
      if (active) checkLoop();
      return active;
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
    setLoopBudget: (ms) => {
      state.loopBudget = Number.isFinite(ms) && ms > 0 ? ms : null;
    },
    // A test that never finished (for example a killed worker) must not
    // turn the module-scope hits of the next file into test hits.
    startFile: () => {
      state.running = 0;
      state.loopSpent = 0;
    },
    // A start does not clear `hits`: a concurrent test can still own them.
    // Each finish takes the hits so far, so no hit is lost.
    startTest: () => {
      if (state.running === 0) state.testStarted = performance.now();
      state.running += 1;
    },
    finishTest: () => {
      const result = { hits: [...state.hits], staticHits: [...state.staticHits] };
      state.running = Math.max(0, state.running - 1);
      if (state.running === 0) state.loopSpent += performance.now() - state.testStarted;
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
