import assert from "node:assert/strict";
import test from "node:test";

import { validateRun } from "../src/adapters/common.mjs";

const request = (fields) => ({
  mutant_id: "m1",
  test_ids: ["t::a"],
  timeout_ms: 5000,
  bail: true,
  ...fields,
});

test("a run request without loop_budget_ms is valid", () => {
  assert.doesNotThrow(() => validateRun(request({})));
  assert.doesNotThrow(() => validateRun(request({ loop_budget_ms: 650 })));
});

test("a loop_budget_ms that is not a positive integer is a bad request", () => {
  for (const value of [0, -1, 1.5, "650", null]) {
    assert.throws(
      () => validateRun(request({ loop_budget_ms: value })),
      (error) => error.code === "bad-request" && /loop_budget_ms/u.test(error.message),
      `loop_budget_ms ${JSON.stringify(value)}`,
    );
  }
});

test("an isolated field that is not a boolean is a bad request", () => {
  assert.doesNotThrow(() => validateRun(request({ isolated: true })));
  assert.doesNotThrow(() => validateRun(request({ isolated: false })));
  for (const value of [1, "true", null]) {
    assert.throws(
      () => validateRun(request({ isolated: value })),
      (error) => error.code === "bad-request" && /isolated/u.test(error.message),
      `isolated ${JSON.stringify(value)}`,
    );
  }
});
