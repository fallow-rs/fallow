// This module has no static Node import, because the Vitest replay file
// loads it in the test worker.
import { LOOP_TIMEOUT_NAME, WALL_LOOP_TIMEOUT_NAME } from "./global.mjs";
import { decodeValue, encodeResult } from "./replay-value.mjs";

/** The sides of one replay, in call order. */
export const SIDES = ["original", "mutant"];

const TEXT_LIMIT = 200;
const LOOP_TIMEOUT_NAMES = new Set([LOOP_TIMEOUT_NAME, WALL_LOOP_TIMEOUT_NAME]);

// The real timer functions, read when the module loads. The call timeout
// must fire also when the user code replaces the timers.
const realSetTimeout = globalThis.setTimeout;
const realClearTimeout = globalThis.clearTimeout;

const inspect = globalThis.process?.getBuiltinModule?.("node:util")?.inspect;

/** A random source with a fixed seed (mulberry32). */
const seededRandom = (seed) => {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4_294_967_296;
  };
};

/**
 * Fixes the clock and the random source for one replay call: `Date.now()`,
 * `new Date()` and `Date()` give `clockMs`, `performance.now()` gives 0, and
 * `Math.random` starts from `seed`. Each call starts from the same values,
 * so a value from the clock or from `Math.random` cannot give a difference
 * between the calls. `restore` puts the real functions back. A date that
 * the real constructor made is still an `instanceof Date`.
 */
export const installDeterminism = ({ clockMs, seed }) => {
  const RealDate = globalThis.Date;
  const realRandom = Math.random;
  const performance = globalThis.performance;
  const realPerformanceNow = performance?.now;
  class ReplayDate extends RealDate {
    constructor(...args) {
      if (args.length === 0) super(clockMs);
      else super(...args);
    }

    static now() {
      return clockMs;
    }
  }
  Object.defineProperty(ReplayDate, Symbol.hasInstance, {
    value: (value) => value instanceof RealDate,
  });
  globalThis.Date = new Proxy(ReplayDate, {
    apply: () => new RealDate(clockMs).toString(),
  });
  Math.random = seededRandom(seed);
  if (typeof realPerformanceNow === "function") performance.now = () => 0;
  return {
    restore: () => {
      globalThis.Date = RealDate;
      Math.random = realRandom;
      if (typeof realPerformanceNow === "function") performance.now = realPerformanceNow;
    },
  };
};

const textOf = (value, threw) => {
  let text;
  if (threw && value instanceof Error) {
    text = `${value.name}: ${value.message}`;
  } else if (typeof inspect === "function") {
    text = inspect(value, { depth: 4, breakLength: Number.POSITIVE_INFINITY });
  } else {
    text = String(value);
  }
  return text.length > TEXT_LIMIT ? `${text.slice(0, TEXT_LIMIT - 1)}…` : text;
};

const record = (outcome, value) => {
  if (outcome === "timed_out") return { outcome, value: null, text: "timed out" };
  return { outcome, value: encodeResult(value), text: textOf(value, outcome === "threw") };
};

const isLoopTimeout = (error) => LOOP_TIMEOUT_NAMES.has(error?.name);

/** Loads the module, calls the export and awaits a returned thenable. */
const invoke = async (request, load) => {
  const namespace = await load();
  const target = namespace?.[request.export];
  if (typeof target !== "function") return record("not_callable", target);
  const args = decodeValue(request.args);
  return record("returned", await target(...args));
};

/**
 * Runs one call of one side. The load of the module counts as part of the
 * call, so a mutant in top-level code is active when the module evaluates,
 * and an error that the load throws is a `threw` outcome.
 */
export const runCall = async (request, side, { load, control, beforeCall = () => {} }) => {
  const active = side === "mutant" ? request.mutant_id : null;
  beforeCall(active);
  const determinism = installDeterminism({ clockMs: request.clock_ms, seed: request.seed });
  let timer;
  const timeout = new Promise((resolve) => {
    timer = realSetTimeout(() => resolve(record("timed_out")), request.call_timeout_ms);
  });
  control.startCall(request.call_timeout_ms);
  try {
    const call = control
      .runWithActive(active, () => invoke(request, load))
      .catch((error) => (isLoopTimeout(error) ? record("timed_out") : record("threw", error)));
    return await Promise.race([call, timeout]);
  } finally {
    realClearTimeout(timer);
    control.finishCall();
    determinism.restore();
  }
};

/**
 * Runs `request.calls` calls of each side, in turns: the original, then the
 * mutant, and again. Each call record has its side, its 1-based index, its
 * outcome, the encoded value and a short text.
 */
export const runReplay = async (request, environment) => {
  const calls = [];
  for (let index = 1; index <= request.calls; index += 1) {
    for (const side of SIDES) {
      calls.push({ side, index, ...(await runCall(request, side, environment)) });
    }
  }
  return calls;
};
