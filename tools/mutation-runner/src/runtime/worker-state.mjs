/**
 * Records the process-wide state of a test worker, so that the adapter can
 * tell when a test file of one mutant run left state that a later run of the
 * same process would see. `vi.resetModules()` gives each run fresh source
 * modules. This check covers the state outside the modules.
 */

// Variables that Vitest and the adapter set for each file or test.
const OWN_ENV = /^(?:VITEST|NODE_OPTIONS$)/u;

// Built-in prototypes that a test or a polyfill can extend.
const PROTOTYPES = [
  Object,
  Array,
  String,
  Number,
  Boolean,
  Function,
  Promise,
  Map,
  Set,
  RegExp,
  Date,
  Error,
];

const TIMER_RESOURCES = new Set(["Timeout", "Immediate"]);

/** The own properties of `target`, without calling a getter. */
const descriptors = (target) =>
  new Map(
    Reflect.ownKeys(target).map((key) => {
      const descriptor = Reflect.getOwnPropertyDescriptor(target, key);
      return [key, descriptor];
    }),
  );

const sameDescriptor = (left, right) =>
  left !== undefined &&
  right !== undefined &&
  Object.is(left.value, right.value) &&
  left.get === right.get &&
  left.set === right.set;

// Node creates these globals lazily, for example the dispatcher of `fetch`.
const RUNTIME_SYMBOL = /^(?:undici|nodejs)\./u;
// Vitest sets its own globals again for each test file.
const RUNNER_GLOBAL = /^__vitest_/u;

const isRuntimeKey = (key) =>
  typeof key === "symbol" ? RUNTIME_SYMBOL.test(key.description ?? "") : RUNNER_GLOBAL.test(key);

/** True when `current` has a key that `baseline` lacks, or another value. */
const changedProperties = (baseline, current, ignored) => {
  for (const [key, descriptor] of current) {
    if (ignored.has(key) || isRuntimeKey(key)) continue;
    if (!sameDescriptor(baseline.get(key), descriptor)) return true;
  }
  for (const key of baseline.keys()) {
    if (!current.has(key) && !ignored.has(key) && !isRuntimeKey(key)) return true;
  }
  return false;
};

const envText = () =>
  JSON.stringify(
    Object.entries(globalThis.process?.env ?? {})
      .filter(([key]) => !OWN_ENV.test(key))
      .toSorted(([left], [right]) => (left < right ? -1 : 1)),
  );

const listenerCounts = () => {
  const proc = globalThis.process;
  if (typeof proc?.eventNames !== "function") return "";
  return JSON.stringify(
    proc.eventNames().map((event) => [String(event), proc.listenerCount(event)]),
  );
};

const activeTimers = () => {
  const info = globalThis.process?.getActiveResourcesInfo?.() ?? [];
  return info.filter((resource) => TIMER_RESOURCES.has(resource)).length;
};

const safe = (read, fallback) => {
  try {
    return read();
  } catch {
    return fallback;
  }
};

/** Records the state that later runs of the worker compare with. */
export const captureWorkerState = () => ({
  globals: descriptors(globalThis),
  prototypes: PROTOTYPES.map((type) => descriptors(type.prototype)),
  env: envText(),
  listeners: listenerCounts(),
  cwd: safe(() => globalThis.process?.cwd(), null),
  exitCode: globalThis.process?.exitCode,
  timers: activeTimers(),
});

/**
 * True when the state of the worker differs from `baseline`. `ignored` names
 * the global keys that the runner itself sets again for each file or test.
 */
export const workerStateChanged = (baseline, ignored = new Set()) => {
  if (changedProperties(baseline.globals, descriptors(globalThis), ignored)) return true;
  const prototypesChanged = PROTOTYPES.some((type, index) =>
    changedProperties(baseline.prototypes[index], descriptors(type.prototype), new Set()),
  );
  return (
    prototypesChanged ||
    envText() !== baseline.env ||
    listenerCounts() !== baseline.listeners ||
    safe(() => globalThis.process?.cwd(), null) !== baseline.cwd ||
    globalThis.process?.exitCode !== baseline.exitCode ||
    activeTimers() > baseline.timers
  );
};
