import { GLOBAL_NAME } from "../generated-protocol.mjs";

const CONTROL = Symbol.for("fallow.mutation.control");

/**
 * The `name` of the error that ends a test of a mutant run past the loop
 * budget, measured in CPU time of the test thread.
 */
export const LOOP_TIMEOUT_NAME = "FallowMutationLoopTimeout";

/**
 * The `name` of the same error when the runtime has no thread CPU clock and
 * measures wall time. Host load can use wall time, so the API retries it.
 */
export const WALL_LOOP_TIMEOUT_NAME = "FallowMutationWallLoopTimeout";

// `a` reads the clock once per this many site evaluations of a mutant run.
// A clock read on each evaluation would slow a hot loop that ends.
const LOOP_CHECK_MASK = 0x7f;

// The clock of the loop budget, in ms. The CPU time of the test thread is
// the first choice: a test that waits uses none, and other load on the host
// does not count. `process.cpuUsage` is no fallback, because it adds the
// time of every thread of the process. A runtime without
// `process.threadCpuUsage` (Node before 22.19 and 23.9, or browser mode)
// uses the wall clock. The functions are read when the module loads, so
// fake timers of a test cannot replace them. The setup file loads this
// module before any test code.
const clock = (() => {
  const proc = globalThis.process;
  if (typeof proc?.threadCpuUsage === "function") {
    const usage = proc.threadCpuUsage.bind(proc);
    return {
      read: () => {
        const { user, system } = usage();
        return (user + system) / 1000;
      },
      errorName: LOOP_TIMEOUT_NAME,
    };
  }
  const perf = globalThis.performance;
  if (typeof perf?.now === "function") {
    return { read: perf.now.bind(perf), errorName: WALL_LOOP_TIMEOUT_NAME };
  }
  return null;
})();

/** Gives the clock value in ms, or null without a working clock. */
const readClock = () => {
  if (clock === null) return null;
  try {
    return clock.read();
  } catch {
    return null;
  }
};

const loopTimeout = (budgetMs) => {
  const error = new Error(
    `Test timed out in ${budgetMs}ms: the mutant run still evaluated instrumented code after the loop budget`,
  );
  error.name = clock?.errorName ?? LOOP_TIMEOUT_NAME;
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
    if (state.loopBudget === null) return;
    state.evaluations += 1;
    if ((state.evaluations & LOOP_CHECK_MASK) !== 0 || state.running === 0) return;
    const now = readClock();
    if (now === null || state.loopSpent + now - state.testStarted <= state.loopBudget) return;
    throw loopTimeout(state.loopBudget);
  };
  // `a` and `c` run inside user code. They read no property of `id`, and a
  // failure inside them must never change the result of a test. The only
  // error that `a` throws is the loop timeout.
  const runtime = {
    a: (id) => {
      const current = state.active;
      if (current === null) return false;
      let active;
      try {
        active = current === id;
      } catch {
        return false;
      }
      checkLoop();
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
