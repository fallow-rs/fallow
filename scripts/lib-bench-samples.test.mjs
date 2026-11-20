import assert from "node:assert/strict";
import { test } from "node:test";

import { sampleOutcome, sampleSummary, stats } from "./lib/bench-samples.mjs";
import { classifyRun } from "./lib/tool-run.mjs";

test("sampleSummary is complete when each requested sample succeeded", () => {
  const summary = sampleSummary([10, 20, 30], 3);
  assert.equal(summary.complete, true);
  assert.equal(summary.stats.median, 20);
});

test("sampleSummary reports a partial set of samples", () => {
  const summary = sampleSummary([10], 5);
  assert.equal(summary.complete, false);
  assert.equal(summary.successful, 1);
  assert.equal(summary.requested, 5);
  assert.equal(summary.stats.median, 10);
});

test("sampleSummary has no statistics without a successful sample", () => {
  const summary = sampleSummary([], 5);
  assert.equal(summary.complete, false);
  assert.equal(summary.stats, null);
});

test("sampleOutcome rejects a run that did not start", () => {
  const run = classifyRun({
    status: null,
    error: { code: "ENOENT", message: "spawn dpdm ENOENT" },
  });
  const outcome = sampleOutcome(run, { ok: true, value: {} });
  assert.equal(outcome.valid, false);
  assert.match(outcome.reason, /failed to start/);
});

test("sampleOutcome rejects a run without a report", () => {
  const outcome = sampleOutcome(classifyRun({ status: 0, signal: null }), {
    ok: false,
    error: "no JSON output",
  });
  assert.equal(outcome.valid, false);
  assert.equal(outcome.reason, "no JSON output");
});

test("sampleOutcome accepts a run with a report", () => {
  const outcome = sampleOutcome(classifyRun({ status: 0, signal: null }), { ok: true, value: [] });
  assert.deepEqual(outcome, { valid: true, reason: null });
});

test("stats gives the median of an even count", () => {
  assert.deepEqual(stats([4, 1, 3, 2]), { min: 1, max: 4, mean: 2.5, median: 2.5 });
});
