import assert from "node:assert/strict";
import test from "node:test";

import {
  PROJECTS,
  failedRun,
  missedTargets,
  parseArgs,
  parseTime,
  summarize,
  verdictDifferences,
} from "./mutation-bench.mjs";

test("parseTime reads the POSIX output of time -p", () => {
  assert.deepEqual(parseTime("real 12.50\nuser 20.25\nsys 3.75\n"), {
    wallMs: 12_500,
    cpuMs: 24_000,
  });
  assert.equal(parseTime("no timing here"), null);
});

test("summarize counts the executed mutants and the throughput", () => {
  const report = {
    summary: { score: 0.5 },
    mutants: [
      { mutant_id: "a", status: "killed" },
      { mutant_id: "b", status: "survived" },
      { mutant_id: "c", status: "no_coverage" },
      { mutant_id: "d", status: "timeout" },
    ],
  };
  const measured = summarize(report, { wallMs: 1000, cpuMs: 3600 });
  assert.equal(measured.executed, 3);
  assert.equal(measured.mutantsPerCpuHour, 3000);
  assert.equal(measured.verdicts.get("c"), "no_coverage");
});

test("verdictDifferences lists changed, added and removed mutants", () => {
  const reference = new Map([
    ["a", "killed"],
    ["b", "survived"],
    ["c", "killed"],
  ]);
  const candidate = new Map([
    ["a", "killed"],
    ["b", "killed"],
    ["d", "survived"],
  ]);
  assert.deepEqual(verdictDifferences(reference, candidate), [
    { id: "b", from: "survived", to: "killed" },
    { id: "c", from: "killed", to: null },
    { id: "d", from: null, to: "survived" },
  ]);
});

test("missedTargets names each target that a run misses", () => {
  const targets = { wallMs: 30_000, mutantsPerCpuHour: 40_000 };
  assert.deepEqual(missedTargets(targets, { wallMs: 20_000, mutantsPerCpuHour: 45_000 }), []);
  assert.deepEqual(missedTargets(targets, { wallMs: 31_000, mutantsPerCpuHour: 39_000 }), [
    "wall 31000 ms > 30000 ms",
    "39000 mutants per CPU hour < 40000",
  ]);
  assert.deepEqual(
    missedTargets({ mutantsPerCpuHour: 38_000 }, { wallMs: 99_000, mutantsPerCpuHour: 38_000 }),
    [],
  );
});

test("each project has a pinned commit and at least one target", () => {
  for (const project of PROJECTS) {
    assert.match(project.commit, /^[0-9a-f]{7,40}$/u, project.name);
    assert.ok(Object.keys(project.targets).length > 0, project.name);
  }
});

test("parseArgs reads each flag and rejects bad input", () => {
  const options = parseArgs([
    "--bin",
    "a",
    "--base",
    "b",
    "--runs",
    "2",
    "--projects",
    "defu,ufo",
    "--check",
    "--json",
  ]);
  assert.equal(options.runs, 2);
  assert.deepEqual(options.projects, ["defu", "ufo"]);
  assert.equal(options.check, true);
  assert.equal(options.json, true);
  assert.match(options.bin, /a$/u);
  assert.match(options.base, /b$/u);
  assert.throws(() => parseArgs([]), /--bin/u);
  assert.throws(() => parseArgs(["--bin", "a", "--runs", "0"]), /--runs/u);
  assert.throws(() => parseArgs(["--bin", "a", "--fast"]), /unknown argument/u);
});

test("failedRun fails a verdict change, and a missed target only under --check", () => {
  const clean = { differences: [], missed: [] };
  const slow = { differences: [], missed: ["wall"] };
  const changed = { differences: [{ id: "a" }], missed: [] };
  assert.equal(failedRun(clean, true), false);
  assert.equal(failedRun(slow, false), false);
  assert.equal(failedRun(slow, true), true);
  assert.equal(failedRun(changed, false), true);
});
