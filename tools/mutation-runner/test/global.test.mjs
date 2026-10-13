import assert from "node:assert/strict";
import test from "node:test";

import { installFallowMutation } from "../src/runtime/global.mjs";

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
