import assert from "node:assert/strict";
import { test } from "node:test";

import { join } from "node:path";

import {
  formatMs,
  sampleOutcome,
  sampleSummary,
  stats,
  timeCells,
  timeSample,
} from "./lib/bench-samples.mjs";
import { makeTempDir } from "./lib/temp-dir.mjs";
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

test("timeSample accepts exit code 1 and measures the time", () => {
  const run = timeSample(process.execPath, ["-e", "process.stdout.write('{}'); process.exit(1)"], {
    timeout: 10_000,
  });
  assert.equal(run.ok, true);
  assert.equal(run.stdout, "{}");
  assert.ok(run.elapsed > 0);
  assert.equal(run.peakRssBytes, 0);
});

test("timeSample reads stdout back from a regular file", () => {
  const stdoutFile = join(makeTempDir("fallow-lib-bench-samples-"), "stdout");
  const run = timeSample(process.execPath, ["-e", "process.stdout.write('[1]')"], {
    timeout: 10_000,
    stdoutFile,
  });
  assert.equal(run.ok, true);
  assert.equal(run.stdout, "[1]");
});

test("timeSample reports the peak memory under /usr/bin/time", () => {
  const run = timeSample(process.execPath, ["-e", "0"], { timeout: 10_000, memory: true });
  assert.equal(run.ok, true, run.reason);
  assert.ok(run.peakRssBytes > 0);
});

test("timeSample rejects exit code 2", () => {
  const run = timeSample(process.execPath, ["-e", "process.exit(2)"], { timeout: 10_000 });
  assert.equal(run.ok, false);
  assert.equal(run.reason, "exited with status 2");
});

test("formatMs and timeCells format durations", () => {
  assert.equal(formatMs(850), "850ms");
  assert.equal(formatMs(1250), "1.25s");
  assert.deepEqual(timeCells(null), { Min: "--", Mean: "--", Median: "--", Max: "--" });
  assert.equal(timeCells(stats([10, 30])).Median, "20ms");
});
