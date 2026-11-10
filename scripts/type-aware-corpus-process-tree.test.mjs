import assert from "node:assert/strict";
import test from "node:test";

import {
  splitTimeReport,
  timedCommand,
  parsePsCpuTime,
  processTreeCpuMs,
  processTreePids,
  recordCpuSample,
} from "./type-aware-corpus/process-tree.mjs";
import { pairedOverheads } from "./type-aware-corpus/summary.mjs";

test("ps CPU times parse to milliseconds", () => {
  assert.equal(parsePsCpuTime("0:00.00"), 0);
  assert.equal(parsePsCpuTime("0:01.84"), 1840);
  assert.equal(parsePsCpuTime("836:35.10"), 50_195_100);
  assert.equal(parsePsCpuTime("01:02:03"), 3_723_000);
  assert.equal(parsePsCpuTime("2-00:00:01"), 172_801_000);
  assert.equal(parsePsCpuTime("bogus"), null);
});

test("the process tree holds the root and each descendant", () => {
  const rows = [
    [10, 1],
    [11, 10],
    [12, 11],
    [20, 1],
  ];
  assert.deepEqual([...processTreePids(10, rows)].toSorted(), [10, 11, 12]);
});

test("tree CPU time sums the highest sample of each process, also after it exits", () => {
  const maxima = new Map();
  recordCpuSample(maxima, {
    cpuMsByPid: new Map([
      [10, 100],
      [11, 500],
    ]),
  });
  recordCpuSample(maxima, { cpuMsByPid: new Map([[10, 300]]) });
  recordCpuSample(maxima, null);
  assert.equal(processTreeCpuMs(maxima), 800);
  assert.equal(processTreeCpuMs(new Map()), null);
});

test("overheads use CPU time, and wall time without a CPU sample or on request", () => {
  const run = (mode, wall, cpu) => ({
    warmup: false,
    iteration: 0,
    mode,
    wall_ms: wall,
    cpu_ms: cpu,
  });
  const projects = [
    { runs: [run("baseline", 1000, 400), run("refined", 9000, 900)] },
    { runs: [run("baseline", 100, null), run("refined", 300, null)] },
  ];
  assert.deepEqual(pairedOverheads(projects), [500, 200]);
  assert.deepEqual(pairedOverheads(projects, "wall"), [8000, 200]);
});

test("a time -p report at the end of stderr gives the CPU time and the own stderr", () => {
  assert.deepEqual(splitTimeReport("warning: x\nreal 1.18\nuser 1.47\nsys 0.97\n"), {
    stderr: "warning: x\n",
    cpuMs: 2440,
  });
  assert.deepEqual(splitTimeReport("real 0.10\nuser 0.00\nsys 0.01\n"), { stderr: "", cpuMs: 10 });
  assert.deepEqual(splitTimeReport("user 1.00\n"), { stderr: "user 1.00\n", cpuMs: null });
  assert.equal(timedCommand("fallow", ["x"], "win32").timed, false);
});
