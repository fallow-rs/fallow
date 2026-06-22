import assert from "node:assert/strict";
import { test } from "node:test";

import { computeGate, parseRows } from "./check-telemetry-fail-rate.mjs";

const OPTIONS = {
  targetVersion: "2.101.0",
  minTargetEvents: 100,
  minBaselineEvents: 100,
  maxDeltaPoints: 5,
  maxMultiplier: 1.5,
};

test("fails when the target fail rate jumps above the trailing baseline", () => {
  const rows = parseRows([
    ["2.101.0", 1000, 200],
    ["2.100.0", 1200, 24],
    ["2.99.0", 800, 16],
  ]);

  const result = computeGate({ rows, ...OPTIONS });

  assert.equal(result.ok, false);
  assert.equal(result.skipped, false);
  assert.equal(result.targetRate, 0.2);
  assert.equal(result.baselineRate, 0.02);
});

test("passes when the target is within the baseline envelope", () => {
  const rows = parseRows([
    ["2.101.0", 1000, 35],
    ["2.100.0", 1200, 30],
    ["2.99.0", 800, 20],
  ]);

  const result = computeGate({ rows, ...OPTIONS });

  assert.equal(result.ok, true);
  assert.equal(result.skipped, false);
});

test("skips until the target has enough telemetry", () => {
  const rows = parseRows([
    ["2.101.0", 99, 80],
    ["2.100.0", 1000, 20],
  ]);

  const result = computeGate({ rows, ...OPTIONS });

  assert.equal(result.ok, true);
  assert.equal(result.skipped, true);
  assert.match(result.reason, /target 2\.101\.0/);
});

test("skips until the baseline has enough telemetry", () => {
  const rows = parseRows([
    ["2.101.0", 1000, 200],
    ["2.100.0", 99, 2],
  ]);

  const result = computeGate({ rows, ...OPTIONS });

  assert.equal(result.ok, true);
  assert.equal(result.skipped, true);
  assert.match(result.reason, /baseline/);
});

test("ignores newer versions when building the baseline", () => {
  const rows = parseRows([
    ["2.102.0", 1000, 800],
    ["2.101.0", 1000, 40],
    ["2.100.0", 1000, 20],
  ]);

  const result = computeGate({ rows, ...OPTIONS });

  assert.equal(result.ok, true);
  assert.equal(result.baseline.failed, 20);
});

test("accepts object rows from fixtures", () => {
  const rows = parseRows([
    { version: "v2.101.0", events: "1000", failed: "200" },
    { version: "2.100.0", events: 1000, failed: 20 },
  ]);

  assert.deepEqual(rows, [
    { version: "2.101.0", events: 1000, failed: 200 },
    { version: "2.100.0", events: 1000, failed: 20 },
  ]);
});
