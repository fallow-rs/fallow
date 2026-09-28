import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  formatPercent,
  gateFailures,
  geomeanChange,
  median,
  parsePerfInstructions,
} from "./pgo-compare.mjs";

test("median takes the middle value or the mean of the two middle values", () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 3, 2]), 2.5);
});

test("geomean change is the relative change of the geometric mean ratio", () => {
  assert.ok(Math.abs(geomeanChange([0.9, 0.9, 0.9]) + 0.1) < 1e-12);
  assert.ok(Math.abs(geomeanChange([0.5, 2])) < 1e-12);
});

test("the gate passes at the minimum gain and with a smaller PGO binary", () => {
  assert.deepEqual(
    gateFailures({ wallChange: -0.05, minGain: 0.05, baseBytes: 100, pgoBytes: 100 }),
    [],
  );
});

test("the gate fails on a small gain and on a larger PGO binary", () => {
  const failures = gateFailures({
    wallChange: -0.049,
    minGain: 0.05,
    baseBytes: 100,
    pgoBytes: 101,
  });
  assert.equal(failures.length, 2);
  assert.match(failures[0], /-4\.9%/);
  assert.match(failures[1], /larger than the base binary/);
});

test("without a minimum gain only the size check applies", () => {
  assert.deepEqual(
    gateFailures({ wallChange: 0.2, minGain: null, baseBytes: 10, pgoBytes: 9 }),
    [],
  );
});

test("a NaN wall change fails the gate", () => {
  assert.equal(
    gateFailures({ wallChange: Number.NaN, minGain: 0.05, baseBytes: 1, pgoBytes: 1 }).length,
    1,
  );
});

test("perf stat CSV output gives the instruction count or null", () => {
  assert.equal(
    parsePerfInstructions("# started on\n\n123456,,instructions:u,1000,100.00,,\n"),
    123456,
  );
  assert.equal(parsePerfInstructions("<not supported>,,instructions:u,0,100.00,,\n"), null);
  assert.equal(parsePerfInstructions(""), null);
});

test("percent format keeps the sign", () => {
  assert.equal(formatPercent(0.123), "+12.3%");
  assert.equal(formatPercent(-0.05), "-5.0%");
});

test("the script runs main from a path with a space and through a symlink", () => {
  const dir = mkdtempSync(join(tmpdir(), "pgo compare "));
  try {
    const link = join(dir, "pgo-compare.mjs");
    symlinkSync(resolve(fileURLToPath(new URL("./pgo-compare.mjs", import.meta.url))), link);
    // Without the required options, main prints the usage and exits 2.
    const result = spawnSync(process.execPath, [link], { encoding: "utf8" });
    assert.equal(result.status, 2);
    assert.match(result.stderr, /usage: pgo-compare\.mjs/u);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
