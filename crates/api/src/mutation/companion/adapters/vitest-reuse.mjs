// Generated from tools/mutation-runner/src/adapters/vitest-reuse.mjs. Do not edit.
/**
 * A Vitest pool runner that keeps one worker process for many mutant runs.
 *
 * Vitest stops the worker of a run when its queue is empty, so each run of
 * `runTestSpecifications` starts a new process. That start costs more CPU
 * than most mutant runs. This pool wraps the `forks` or `threads` worker of
 * Vitest. It answers the `start` and `stop` messages of Vitest itself, so the
 * next run gets the same process. Vitest sends the provided context (the
 * active mutant) with each run request, and the setup file loads the modules
 * of each test file again (see `runtime/vitest-setup.mjs`).
 *
 * The worker reads its config only from the first `start` message. A run
 * with another `testTimeout` or `hookTimeout` therefore gets a new process.
 */

/** The pool name of a reused `forks` worker and of a reused `threads` worker. */
export const REUSED_POOLS = { forks: "fallow-reused-forks", threads: "fallow-reused-threads" };

/** A worker gets a new process after this many runs, so its memory stays bounded. */
const DEFAULT_MAX_RUNS = 200;

/**
 * Creates one reused pool for a Vitest worker class. `name` is the pool name
 * that the project config uses while the pool is active. `maxRuns` is the
 * number of runs after which the process ends.
 */
export const createReusedPool = (name, WorkerClass, { maxRuns = DEFAULT_MAX_RUNS } = {}) => {
  const state = {
    worker: null,
    started: false,
    runs: 0,
    key: null,
    environment: null,
    dirty: false,
    current: null,
  };

  const deliver = (target, event, args) => {
    const listeners = target?.listeners.get(event);
    if (listeners) for (const listener of Array.from(listeners)) listener(...args);
  };

  const forget = () => {
    state.worker = null;
    state.started = false;
    state.runs = 0;
    state.environment = null;
    state.dirty = false;
  };

  const close = async () => {
    const worker = state.worker;
    forget();
    await worker?.stop();
  };

  /** The environment of a worker: the worker sets it up only at start. */
  const environmentKey = (environment) =>
    JSON.stringify([environment?.name ?? null, environment?.options ?? null]);

  class ReusedWorker {
    name = name;

    constructor(options) {
      this.options = options;
      this.listeners = new Map();
      this.stopAsked = false;
    }

    get reportMemory() {
      return state.worker?.reportMemory ?? false;
    }

    get cacheFs() {
      return state.worker?.cacheFs;
    }

    on(event, callback) {
      if (!this.listeners.has(event)) this.listeners.set(event, new Set());
      this.listeners.get(event).add(callback);
    }

    off(event, callback) {
      this.listeners.get(event)?.delete(callback);
    }

    async start() {
      const environment = environmentKey(this.options.environment);
      if (state.worker !== null && (state.dirty || state.environment !== environment)) {
        await close();
      }
      state.current = this;
      if (state.worker !== null) return;
      const worker = new WorkerClass(this.options);
      await worker.start();
      state.worker = worker;
      state.environment = environment;
      // Only the active process reaches the runner of the current run. A
      // process that this pool closed must not report its exit to the next
      // run as a crash. A message with no current run (for example a call
      // of a timer that a test left) cannot get an answer, so the process is
      // not reused.
      const forward =
        (event) =>
        (...args) => {
          if (state.worker !== worker) return;
          if (state.current === null) {
            state.dirty = true;
            return;
          }
          deliver(state.current, event, args);
        };
      worker.on("message", forward("message"));
      worker.on("error", forward("error"));
      // A process that ends by itself (a crash, or a kill by the API) is not
      // reused.
      worker.on("exit", (...args) => {
        if (state.worker !== worker) return;
        const current = state.current;
        forget();
        deliver(current, "exit", args);
      });
    }

    send(message) {
      // The reply goes to the runner that asked, also when the next run has
      // already started.
      const reply = (type) =>
        queueMicrotask(() =>
          deliver(this, "message", [{ type, __vitest_worker_response__: true, fallowReply: true }]),
        );
      if (message?.type === "start" && state.started) return reply("started");
      if (message?.type === "start") state.started = true;
      if (message?.type === "stop") {
        this.stopAsked = true;
        return reply("stopped");
      }
      if (message?.type === "run") state.runs += 1;
      return state.worker.send(message);
    }

    deserialize(message) {
      return message?.fallowReply === true ? message : state.worker.deserialize(message);
    }

    async stop() {
      if (state.current === this) state.current = null;
      // Vitest stops a runner without a stop message after a task error, a
      // start failure or a forced cancel. That process is not reused.
      if (!this.stopAsked || state.runs >= maxRuns) await close();
    }
  }

  return {
    name,
    createPoolWorker: (options) => new ReusedWorker(options),
    /**
     * Keeps the process only for runs with the same `key` (the config that
     * the worker reads at start). Another key closes the process first.
     */
    prepare: async (key) => {
      if (state.key !== key) await close();
      state.key = key;
    },
    close,
  };
};
