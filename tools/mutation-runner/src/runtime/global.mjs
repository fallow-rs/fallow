import { GLOBAL_NAME } from "../generated-protocol.mjs";
import { RECORD_CALL_LIMIT, recordEntries } from "./records.mjs";

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

// The longest JSON text of one recorded input. A longer input is not kept.
const MAX_RECORD_TEXT = 16_384;

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
    // An `AsyncLocalStorage` of a replay. A replay call runs in a store with
    // its own active mutant, so a timer or a promise of one call reads the
    // mutant of that call, also when it runs during a later call.
    context: null,
    // The argument recorder of a dry run: the encoder of the replay values,
    // the most inputs of one function, and the inputs and the reasons of
    // each recorded function key. Null outside a recording dry run.
    recorder: null,
  };
  /** The active mutant of the current replay call, else the process value. */
  const currentActive = () => {
    const store = state.context?.getStore();
    return store === undefined ? state.active : store.active;
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
  const refuse = (entry, reason) => {
    entry.refused.set(reason, (entry.refused.get(reason) ?? 0) + 1);
  };
  // Records one call of a wrapped function. Only a call with no active
  // mutant counts, so a recording shows inputs of the original code. A
  // failure here never changes the call.
  /** The record entry of `key`, or null past the call limit of the key. */
  const recordEntry = (recorder, key) => {
    // The call count outlives `takeRecords`, so the cap holds for the
    // whole record pass of the worker.
    const calls = recorder.calls.get(key) ?? 0;
    if (calls >= RECORD_CALL_LIMIT) return null;
    recorder.calls.set(key, calls + 1);
    let entry = recorder.functions.get(key);
    if (entry === undefined) {
      entry = { inputs: new Map(), refused: new Map() };
      recorder.functions.set(key, entry);
    }
    return entry;
  };
  const keepInput = (recorder, entry, encoded) => {
    if (!encoded.ok) {
      refuse(entry, encoded.reason);
      return;
    }
    const text = JSON.stringify(encoded.value);
    if (text.length > MAX_RECORD_TEXT) refuse(entry, "a value that is too large");
    else if (!entry.inputs.has(text) && entry.inputs.size < recorder.limit) {
      entry.inputs.set(text, encoded.value);
    }
  };
  const record = (key, args) => {
    const recorder = state.recorder;
    if (recorder === null || currentActive() !== null) return;
    const entry = recordEntry(recorder, key);
    if (entry === null) return;
    try {
      keepInput(recorder, entry, recorder.encode(args));
    } catch {
      // The call stays as it is. Only its record is lost.
      refuse(entry, "a value that cannot be read");
    }
  };
  // A wrap is a plain function, so `this` and `new` reach the target.
  const wrap = (key, target) => {
    const wrapped = function (...args) {
      record(key, args);
      return new.target === undefined
        ? Reflect.apply(target, this, args)
        : Reflect.construct(target, args, new.target);
    };
    for (const property of ["name", "length"]) {
      try {
        Object.defineProperty(wrapped, property, { value: target[property] });
      } catch {
        // The wrap keeps its own value.
      }
    }
    if (target.prototype !== undefined) wrapped.prototype = target.prototype;
    return wrapped;
  };

  // `a` and `c` run inside user code. They read no property of `id`, and a
  // failure inside them must never change the result of a test. The only
  // error that `a` throws is the loop timeout.
  const runtime = {
    a: (id) => {
      const current = state.context === null ? state.active : currentActive();
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
    // The prelude calls `w` when a module defines an exported function. Out
    // of a recording dry run, the function stays as it is, so a mutant run
    // runs the same code as without the wrap.
    w: (key, target) =>
      state.recorder === null || typeof target !== "function" ? target : wrap(key, target),
  };
  const control = {
    setActive: (id) => {
      state.active = typeof id === "string" ? id : null;
    },
    setLoopBudget: (ms) => {
      state.loopBudget = Number.isFinite(ms) && ms > 0 ? ms : null;
    },
    // The replay gives an `AsyncLocalStorage` instance, because this module
    // has no Node import (the Vitest setup file loads it in browser mode).
    enableContext: (storage) => {
      state.context ??= storage;
    },
    runWithActive: (id, task) => {
      if (state.context === null) throw new Error("the replay context is not enabled");
      return state.context.run({ active: typeof id === "string" ? id : null }, task);
    },
    // One replay call is one test for the loop check: it starts with the
    // full budget, and the time of the earlier calls does not count.
    startCall: (budgetMs) => {
      state.loopBudget = Number.isFinite(budgetMs) && budgetMs > 0 ? budgetMs : null;
      state.loopSpent = 0;
      state.evaluations = 0;
      state.testStarted = readClock() ?? 0;
      state.running = Math.max(state.running, 1);
    },
    finishCall: () => {
      state.loopBudget = null;
    },
    // The dry run gives the encoder, so this module stays free of imports.
    startRecording: (encode, limit) => {
      state.recorder = { encode, limit, functions: new Map(), calls: new Map() };
    },
    stopRecording: () => {
      state.recorder = null;
    },
    // Gives the recorded inputs and reasons of each function, and clears
    // them. The keys keep their first record order.
    takeRecords: () => {
      const recorder = state.recorder;
      if (recorder === null) return [];
      const records = recordEntries(recorder.functions);
      recorder.functions = new Map();
      return records;
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
