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
  const state = { worker: null, started: false, runs: 0, key: null, current: null };

  const dispatch = (event, args) => {
    const listeners = state.current?.listeners.get(event);
    if (listeners) for (const listener of Array.from(listeners)) listener(...args);
  };

  const forget = () => {
    state.worker = null;
    state.started = false;
    state.runs = 0;
  };

  const close = async () => {
    const worker = state.worker;
    forget();
    await worker?.stop();
  };

  class ReusedWorker {
    name = name;

    constructor(options) {
      this.options = options;
      this.listeners = new Map();
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
      state.current = this;
      if (state.worker !== null) return;
      const worker = new WorkerClass(this.options);
      await worker.start();
      state.worker = worker;
      // Only the active process reaches the runner of the current run. A
      // process that this pool closed must not report its exit to the next
      // run as a crash.
      const forward =
        (event) =>
        (...args) => {
          if (state.worker === worker) dispatch(event, args);
        };
      worker.on("message", forward("message"));
      worker.on("error", forward("error"));
      // A process that ends by itself (a crash, or a kill by the API) is not
      // reused.
      worker.on("exit", (...args) => {
        if (state.worker !== worker) return;
        forget();
        dispatch("exit", args);
      });
    }

    send(message) {
      const reply = (type) =>
        queueMicrotask(() =>
          dispatch("message", [{ type, __vitest_worker_response__: true, fallowReply: true }]),
        );
      if (message?.type === "start" && state.started) return reply("started");
      if (message?.type === "start") state.started = true;
      if (message?.type === "stop") return reply("stopped");
      if (message?.type === "run") state.runs += 1;
      return state.worker.send(message);
    }

    deserialize(message) {
      return message?.fallowReply === true ? message : state.worker.deserialize(message);
    }

    async stop() {
      if (state.current === this) state.current = null;
      if (state.runs >= maxRuns) await close();
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
