import assert from "node:assert/strict";
import test from "node:test";

import { MUTATION_RUNNER_PROTOCOL } from "../src/generated-protocol.mjs";
import { installFallowMutation } from "../src/runtime/global.mjs";
import { readMutationConstant } from "./support/vitest-fixture.mjs";

const { runtime } = MUTATION_RUNNER_PROTOCOL;

test("the manifest names the prelude functions of fallow-mutation", () => {
  assert.equal(readMutationConstant("ACTIVE_FUNCTION"), runtime.active_function);
  assert.equal(readMutationConstant("HIT_FUNCTION"), runtime.hit_function);
});

test("the fallow-mutation prelude drives the companion runtime", () => {
  const [active, hit] = new Function(
    `${readMutationConstant("PRELUDE")}\nreturn [${runtime.active_function}, ${runtime.hit_function}];`,
  )();
  const control = installFallowMutation();
  control.setActive("m1");
  assert.equal(active("m1"), true);
  assert.equal(active("m2"), false);
  control.startTest();
  hit("m2");
  assert.deepEqual(control.finishTest(), { hits: ["m2"], staticHits: [] });
});
