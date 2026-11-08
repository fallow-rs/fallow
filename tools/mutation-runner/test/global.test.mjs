import assert from "node:assert/strict";
import test from "node:test";

import { LOOP_TIMEOUT_NAME, installFallowMutation } from "../src/runtime/global.mjs";

test("a() is true only for the active mutant", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  assert.equal(runtime.a("m1"), false);
  control.setActive("m1");
  assert.equal(runtime.a("m1"), true);
  assert.equal(runtime.a("m2"), false);
  control.setActive(null);
  assert.equal(runtime.a("m1"), false);
});

test("c() records a hit for the current test, and a static hit outside a test", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  runtime.c("m3");
  control.startTest();
  runtime.c("m1");
  runtime.c("m1");
  runtime.c("m2");
  assert.deepEqual(control.finishTest(), { hits: ["m1", "m2"], staticHits: ["m3"] });

  control.startTest();
  runtime.c("m4");
  assert.deepEqual(control.finishTest(), { hits: ["m4"], staticHits: [] });

  runtime.c("m5");
  control.startTest();
  assert.deepEqual(control.finishTest(), { hits: [], staticHits: ["m5"] });
});

test("concurrent tests lose no hit, and a hit while any test runs is not static", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startTest();
  runtime.c("m1");
  control.startTest();
  runtime.c("m2");
  assert.deepEqual(control.finishTest(), { hits: ["m1", "m2"], staticHits: [] });
  runtime.c("m3");
  assert.deepEqual(control.finishTest(), { hits: ["m3"], staticHits: [] });
  runtime.c("m4");
  control.startTest();
  assert.deepEqual(control.finishTest(), { hits: [], staticHits: ["m4"] });
});

test("a new test file starts with no running test", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startTest();
  control.startFile();
  runtime.c("m1");
  assert.deepEqual(control.finishTest(), { hits: [], staticHits: ["m1"] });
});

const spin = (runtime, id, until) => {
  while (performance.now() < until) runtime.a(id);
};

test("a() throws a loop timeout when the active mutant runs past the loop budget in a test", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(20);
  control.startTest();
  assert.throws(
    () => spin(runtime, "m1", performance.now() + 5_000),
    (error) => {
      assert.equal(error.name, LOOP_TIMEOUT_NAME);
      assert.match(error.message, /^Test timed out in 20ms/u);
      return true;
    },
  );
  control.finishTest();
  control.setActive(null);
  control.setLoopBudget(null);
});

test("a() never throws for an inactive mutant, outside a test, or without a loop budget", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(5);
  control.startTest();
  assert.doesNotThrow(() => spin(runtime, "m2", performance.now() + 30));
  control.finishTest();
  assert.doesNotThrow(() => spin(runtime, "m1", performance.now() + 30));
  control.setLoopBudget(null);
  control.startTest();
  assert.doesNotThrow(() => spin(runtime, "m1", performance.now() + 30));
  control.finishTest();
  control.setActive(null);
});

test("the loop budget counts the time of the earlier tests of the file", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(40);
  control.startTest();
  spin(runtime, "m1", performance.now() + 30);
  control.finishTest();
  control.startTest();
  assert.throws(() => spin(runtime, "m1", performance.now() + 5_000), { name: LOOP_TIMEOUT_NAME });
  control.finishTest();
  control.startFile();
  control.startTest();
  assert.doesNotThrow(() => spin(runtime, "m1", performance.now() + 20));
  control.finishTest();
  control.setActive(null);
  control.setLoopBudget(null);
});

test("a replaced performance clock neither throws from a() nor moves the loop budget", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  const original = Object.getOwnPropertyDescriptor(globalThis, "performance");
  const evaluate = () => {
    for (let i = 0; i < 1_000; i += 1) runtime.a("m1");
  };
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(1_000);
  control.startTest();
  try {
    Object.defineProperty(globalThis, "performance", {
      configurable: true,
      value: { now: () => 1e12 },
    });
    assert.doesNotThrow(evaluate, "fake time moved the loop budget");
    Object.defineProperty(globalThis, "performance", {
      configurable: true,
      value: {
        now() {
          throw new Error("stubbed clock");
        },
      },
    });
    assert.doesNotThrow(evaluate, "a stubbed clock error reached the caller of a()");
  } finally {
    Object.defineProperty(globalThis, "performance", original);
    control.finishTest();
    control.setActive(null);
    control.setLoopBudget(null);
  }
});

test("each test starts its own count of active evaluations", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  const evaluate = () => {
    for (let i = 0; i < 100; i += 1) runtime.a("m1");
  };
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(1);
  control.startTest();
  evaluate();
  control.finishTest();
  control.startTest();
  const until = performance.now() + 5;
  while (performance.now() < until) {
    // Spend test time with no evaluation of the active mutant.
  }
  try {
    assert.doesNotThrow(evaluate, "the evaluations of the earlier test started a check");
  } finally {
    control.finishTest();
    control.setActive(null);
    control.setLoopBudget(null);
  }
});

test("time that a test spends waiting does not use the loop budget", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startFile();
  control.setActive("m1");
  control.setLoopBudget(20);
  control.startTest();
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 60);
  try {
    assert.doesNotThrow(() => {
      for (let i = 0; i < 1_000; i += 1) runtime.a("m1");
    }, "a wait of 60 ms used a loop budget of 20 ms");
  } finally {
    control.finishTest();
    control.setActive(null);
    control.setLoopBudget(null);
  }
});

test("a() and c() never throw", () => {
  installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  const hostile = {
    toString() {
      throw new Error("boom");
    },
  };
  assert.equal(runtime.a(hostile), false);
  assert.doesNotThrow(() => runtime.c(hostile));
  assert.doesNotThrow(() => runtime.a.call(undefined, "m1"));
  assert.doesNotThrow(() => runtime.c.call(undefined, "m1"));
});

test("a second install keeps one runtime object", () => {
  const first = installFallowMutation();
  first.setActive("m7");
  const second = installFallowMutation();
  assert.equal(globalThis.__fallowMutation.a("m7"), true);
  second.setActive(null);
  assert.equal(globalThis.__fallowMutation.a("m7"), false);
});
