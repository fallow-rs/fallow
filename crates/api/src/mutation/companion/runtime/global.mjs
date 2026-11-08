// Generated from tools/mutation-runner/src/runtime/global.mjs. Do not edit.
import { GLOBAL_NAME } from "../generated-protocol.mjs";

const CONTROL = Symbol.for("fallow.mutation.control");

/** The `name` of the error that ends a test whose mutant runs past the loop budget. */
export const LOOP_TIMEOUT_NAME = "FallowMutationLoopTimeout";

// `a` reads the clock once per this many site evaluations of a mutant run.
// A clock read on each evaluation would slow a hot loop that ends.
const LOOP_CHECK_MASK = 0x7f;

// The CPU time of the test thread, in ms. A test that waits uses no CPU
// time, and other load on the host does not count, so the loop budget
// measures only the work of the test. The function is read when the module
// loads, so fake timers of a test cannot replace it. The setup file loads
// this module before any test code. Without `process` (for example in
// browser mode), the clock is null and the loop check is off.
const cpuUsage = (() => {
  const proc = globalThis.process;
  if (typeof proc?.threadCpuUsage === "function") return proc.threadCpuUsage.bind(proc);
  if (typeof proc?.cpuUsage === "function") return proc.cpuUsage.bind(proc);
  return null;
})();

/** Gives the CPU time of the test thread in ms, or null without a clock. */
const readClock = () => {
  if (cpuUsage === null) return null;
  try {
    const usage = cpuUsage();
    return (usage.user + usage.system) / 1000;
  } catch {
    return null;
  }
};

const loopTimeout = (budgetMs) => {
  const error = new Error(
    `Test timed out in ${budgetMs}ms: the mutant run still evaluated instrumented code after the loop budget`,
  );
  error.name = LOOP_TIMEOUT_NAME;
  return error;
};

const createRuntime = () => {
  // `running` counts the tests that started and did not finish. Concurrent
  // tests overlap, so a hit belongs to a test while any test runs.
  // `loopBudget` is the CPU time in ms that the tests of one file can use
  // in a mutant run. `loopSpent` is the CPU time of the earlier tests
  // of the file, and `testStarted` the clock value at the start of the
  // current test time.
  const state = {
    active: null,
    running: 0,
    hits: new Set(),
    staticHits: new Set(),
    loopBudget: null,
    loopSpent: 0,
    testStarted: 0,
    evaluations: 0,
  };
  // A synchronous endless loop blocks the test worker, so no timer can stop
  // it. In a mutant run, each instrumented site calls `a` on each
  // evaluation. When a loop runs through any instrumented code, this check
  // ends it with an error that the adapter reports as a timeout, and the
  // companion stays alive. A loop through code with no site (test code or a
  // dependency) waits for the watchdog of the API.
  const checkLoop = () => {
    state.evaluations += 1;
    if ((state.evaluations & LOOP_CHECK_MASK) !== 0) return;
    if (state.loopBudget === null || state.running === 0) return;
    const now = readClock();
    if (now === null || state.loopSpent + now - state.testStarted <= state.loopBudget) return;
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
      if (state.active !== null) checkLoop();
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
      if (state.running === 0) {
        state.testStarted = readClock() ?? 0;
        state.evaluations = 0;
      }
      state.running += 1;
    },
    finishTest: () => {
      const result = { hits: [...state.hits], staticHits: [...state.staticHits] };
      state.running = Math.max(0, state.running - 1);
      if (state.running === 0) {
        state.loopSpent += Math.max(0, (readClock() ?? state.testStarted) - state.testStarted);
      }
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
