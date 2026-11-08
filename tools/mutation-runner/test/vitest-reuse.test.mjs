import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";

import { createReusedPool } from "../src/adapters/vitest-reuse.mjs";

let processes = [];

/** A fake Vitest pool worker: one emitter per process, with the sent messages. */
class FakeWorker extends EventEmitter {
  constructor() {
    super();
    this.sent = [];
    this.stopped = false;
    processes.push(this);
  }
  async start() {}
  async stop() {
    this.stopped = true;
    // A process reports its exit later, as a child process does.
    await new Promise((resolve) => setTimeout(resolve, 5));
    this.emit("exit", 0, null);
  }
  send(message) {
    this.sent.push(message.type);
    if (message.type === "start") {
      queueMicrotask(() =>
        this.emit("message", { type: "started", __vitest_worker_response__: true }),
      );
    }
  }
  deserialize(message) {
    return message;
  }
}

/**
 * Runs one Vitest run through the pool, as `PoolRunner` does. Vitest does not
 * wait for the stop of a run before it starts the next run, so the stop
 * promise is returned.
 */
const runOnce = async (pool) => {
  const worker = pool.createPoolWorker({});
  const messages = [];
  const exits = [];
  worker.on("message", (message) => messages.push(worker.deserialize(message).type));
  worker.on("exit", () => exits.push("exit"));
  await worker.start();
  worker.send({ type: "start" });
  worker.send({ type: "run" });
  worker.send({ type: "stop" });
  await new Promise((resolve) => setImmediate(resolve));
  const stopping = worker.stop();
  return { messages, exits, stopping };
};

test("consecutive runs share one process and the pool answers start and stop", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  const first = await runOnce(pool);
  await first.stopping;
  const second = await runOnce(pool);
  await second.stopping;
  assert.equal(processes.length, 1);
  assert.deepEqual(processes[0].sent, ["start", "run", "run"]);
  assert.deepEqual(first.messages, ["started", "stopped"]);
  assert.deepEqual(second.messages, ["started", "stopped"]);
  await pool.close();
});

test("the process ends after the run limit, and its exit does not reach the next run", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker, { maxRuns: 2 });
  await (
    await runOnce(pool)
  ).stopping;
  const second = await runOnce(pool);
  // The next run is active while the closed process still exits.
  const next = pool.createPoolWorker({});
  const exits = [];
  next.on("exit", () => exits.push("exit"));
  await next.start();
  await second.stopping;
  assert.equal(processes[0].stopped, true);
  assert.equal(processes.length, 2);
  assert.deepEqual(exits, [], "the closed process reported its exit to the next run");
  await next.stop();
  await pool.close();
});

test("a process that exits by itself is replaced, and the run sees the exit", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  const worker = pool.createPoolWorker({});
  const exits = [];
  worker.on("exit", () => exits.push("exit"));
  await worker.start();
  processes[0].emit("exit", 1, null);
  assert.deepEqual(exits, ["exit"]);
  await (
    await runOnce(pool)
  ).stopping;
  assert.equal(processes.length, 2);
  await pool.close();
});

test("another key closes the process", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  await pool.prepare("5000");
  await (
    await runOnce(pool)
  ).stopping;
  await pool.prepare("5000");
  await (
    await runOnce(pool)
  ).stopping;
  await pool.prepare("10000");
  await (
    await runOnce(pool)
  ).stopping;
  assert.equal(processes.length, 2);
  assert.equal(processes[0].stopped, true);
  await pool.close();
});

test("a run in another environment gets a new process", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  const node = pool.createPoolWorker({ environment: { name: "node", options: null } });
  await node.start();
  await node.stop();
  const jsdom = pool.createPoolWorker({ environment: { name: "jsdom", options: null } });
  await jsdom.start();
  assert.equal(processes.length, 2);
  assert.equal(processes[0].stopped, true);
  await pool.close();
});

test("the stopped reply reaches the runner that asked, after the next run started", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  const first = pool.createPoolWorker({});
  const replies = [];
  first.on("message", (message) => replies.push(first.deserialize(message).type));
  await first.start();
  first.send({ type: "start" });
  await new Promise((resolve) => setImmediate(resolve));
  first.send({ type: "stop" });
  const stopping = first.stop();
  const next = pool.createPoolWorker({});
  await next.start();
  await new Promise((resolve) => setImmediate(resolve));
  await stopping;
  assert.deepEqual(replies, ["started", "stopped"]);
  await pool.close();
});

test("a stop without a stop message ends the process", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  const worker = pool.createPoolWorker({});
  await worker.start();
  await worker.stop();
  assert.equal(processes[0].stopped, true);
  await (
    await runOnce(pool)
  ).stopping;
  assert.equal(processes.length, 2);
  await pool.close();
});

test("a message between runs makes the next run use a new process", async () => {
  processes = [];
  const pool = createReusedPool("fallow-reused-forks", FakeWorker);
  await (
    await runOnce(pool)
  ).stopping;
  processes[0].emit("message", { type: "late" });
  await (
    await runOnce(pool)
  ).stopping;
  assert.equal(processes.length, 2);
  assert.equal(processes[0].stopped, true);
  await pool.close();
});
