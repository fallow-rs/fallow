import assert from "node:assert/strict";
import { test } from "node:test";

import { compareRuns } from "./mutation-flaky-rate.mjs";

const report = (statuses) => ({
  summary: { counts: { total: Object.keys(statuses).length } },
  mutants: Object.entries(statuses).map(([mutant_id, status]) => ({ mutant_id, status })),
});

test("the same status in each run is stable", () => {
  const result = compareRuns([
    report({ a: "killed", b: "survived" }),
    report({ a: "killed", b: "survived" }),
  ]);
  assert.equal(result.compared, 2);
  assert.equal(result.unstable, 0);
  assert.equal(result.unstable_rate, 0);
});

test("a timeout that survives in another run is a timeout flip", () => {
  const result = compareRuns([
    report({ a: "timeout", b: "killed" }),
    report({ a: "survived", b: "killed" }),
    report({ a: "survived", b: "killed" }),
  ]);
  assert.equal(result.unstable, 1);
  assert.equal(result.score_flips, 1);
  assert.equal(result.timeout_flips, 1);
  assert.equal(result.timeout_flip_rate, 0.5);
  assert.deepEqual(result.mutants[0].statuses, ["timeout", "survived", "survived"]);
});

test("killed against timeout changes no score and is no score flip", () => {
  const result = compareRuns([report({ a: "killed" }), report({ a: "timeout" })]);
  assert.equal(result.unstable, 1);
  assert.equal(result.score_flips, 0);
  assert.equal(result.timeout_flips, 0);
});

test("a runtime error in one run is a runtime-error flip and no score flip", () => {
  const result = compareRuns([report({ a: "killed" }), report({ a: "runtime_error" })]);
  assert.equal(result.unstable, 1);
  assert.equal(result.score_flips, 0);
  assert.equal(result.runtime_error_flips, 1);
});

test("a flaky status in one run is unstable, also with no status change", () => {
  const result = compareRuns([report({ a: "flaky" }), report({ a: "flaky" })]);
  assert.equal(result.unstable, 1);
  assert.equal(result.score_flips, 0);
});

test("only the mutants that each run executed are compared", () => {
  const result = compareRuns([
    report({ a: "killed", b: "no_coverage", c: "pending" }),
    report({ a: "killed", b: "no_coverage" }),
  ]);
  assert.equal(result.compared, 1);
});

test("fewer than two reports is an error", () => {
  assert.throws(() => compareRuns([report({ a: "killed" })]), /at least 2 reports/u);
});

test("a result from the result cache did not run and is not compared", () => {
  const cachedRun = report({ a: "killed", b: "survived" });
  cachedRun.mutants[1].cached = true;
  const result = compareRuns([report({ a: "killed", b: "survived" }), cachedRun]);
  assert.equal(result.compared, 1, "only `a` ran in both runs");
  assert.deepEqual(result.cached, [0, 1]);
});
