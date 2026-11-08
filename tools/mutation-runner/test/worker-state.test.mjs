import assert from "node:assert/strict";
import test from "node:test";

import { captureWorkerState, workerStateChanged } from "../src/runtime/worker-state.mjs";

test("an unchanged worker, NaN and lazy runtime globals are not a change", async () => {
  const baseline = captureWorkerState();
  assert.equal(workerStateChanged(baseline), false);
  globalThis.__vitest_worker__ = {};
  try {
    assert.equal(workerStateChanged(baseline), false, "a runner global changed the state");
  } finally {
    delete globalThis.__vitest_worker__;
  }
});

test("a new global, a changed global and an ignored key", () => {
  const baseline = captureWorkerState();
  globalThis.fallowAdded = 1;
  try {
    assert.equal(workerStateChanged(baseline), true);
    assert.equal(workerStateChanged(baseline, new Set(["fallowAdded"])), false);
  } finally {
    delete globalThis.fallowAdded;
  }
  const original = globalThis.structuredClone;
  globalThis.structuredClone = () => null;
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    globalThis.structuredClone = original;
  }
  assert.equal(workerStateChanged(baseline), false);
});

test("a prototype, the environment, a listener and the exit code are a change", () => {
  const baseline = captureWorkerState();
  // The check must see a polyfill or a test that extends a built-in.
  // oxlint-disable-next-line no-extend-native
  Array.prototype.fallowExtra = () => 0;
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    delete Array.prototype.fallowExtra;
  }
  process.env.FALLOW_WORKER_STATE_TEST = "1";
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    delete process.env.FALLOW_WORKER_STATE_TEST;
  }
  process.env.VITEST_POOL_ID = "7";
  try {
    assert.equal(workerStateChanged(baseline), false, "a Vitest variable changed the state");
  } finally {
    delete process.env.VITEST_POOL_ID;
  }
  const listener = () => {};
  process.on("fallow-test-event", listener);
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    process.off("fallow-test-event", listener);
  }
  process.exitCode = 3;
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    process.exitCode = undefined;
  }
  assert.equal(workerStateChanged(baseline), false);
});

test("a timer that a test left is a change", () => {
  const baseline = captureWorkerState();
  const timer = setTimeout(() => {}, 60_000);
  try {
    assert.equal(workerStateChanged(baseline), true);
  } finally {
    clearTimeout(timer);
  }
});
