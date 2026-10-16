import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { parseArguments, quantile, runTimings, summarize } from "./ci-metrics.mjs";

const script = fileURLToPath(new URL("./ci-metrics.mjs", import.meta.url));

const at = (minute) => new Date(Date.UTC(2026, 0, 1, 0, minute)).toISOString();

const job = (name, created, started, completed, extra = {}) => ({
  name,
  runner_name: "runner-1",
  created_at: at(created),
  started_at: at(started),
  completed_at: at(completed),
  conclusion: "success",
  ...extra,
});

const workflowRun = (extra = {}) => ({
  name: "CI",
  event: "pull_request",
  created_at: at(0),
  ...extra,
});

test("parseArguments returns the defaults for no arguments", () => {
  assert.deepEqual(parseArguments([]), {
    limit: 20,
    workflow: undefined,
    event: undefined,
    repo: undefined,
    json: false,
    help: false,
  });
});

test("parseArguments reads every flag", () => {
  const options = parseArguments([
    "--limit",
    "5",
    "--workflow",
    "ci.yml",
    "--event",
    "push",
    "--repo",
    "owner/name",
    "--json",
  ]);
  assert.deepEqual(options, {
    limit: 5,
    workflow: "ci.yml",
    event: "push",
    repo: "owner/name",
    json: true,
    help: false,
  });
});

test("parseArguments accepts both help flags", () => {
  assert.equal(parseArguments(["--help"]).help, true);
  assert.equal(parseArguments(["-h"]).help, true);
});

test("parseArguments rejects a flag without a value", () => {
  assert.throws(() => parseArguments(["--repo"]), /--repo needs a value/);
});

test("parseArguments rejects an unknown flag", () => {
  assert.throws(() => parseArguments(["--bogus", "1"]), /unknown argument: --bogus/);
});

test("parseArguments rejects a limit that is not a positive integer", () => {
  for (const value of ["0", "-3", "1.5", "abc"]) {
    assert.throws(
      () => parseArguments(["--limit", value]),
      /--limit must be a positive integer/,
      value,
    );
  }
});

test("quantile returns null for no values", () => {
  assert.equal(quantile([], 0.5), null);
});

test("quantile returns the only value for a single value", () => {
  assert.equal(quantile([7], 0), 7);
  assert.equal(quantile([7], 0.9), 7);
});

test("quantile uses the nearest rank on unsorted values", () => {
  const values = [5, 1, 4, 2, 3];
  assert.equal(quantile(values, 0.5), 3);
  assert.equal(quantile(values, 0.9), 5);
  assert.equal(quantile(values, 0), 1);
  assert.deepEqual(values, [5, 1, 4, 2, 3]);
});

test("runTimings returns null when no job used a runner", () => {
  const jobs = [
    job("skipped", 0, 1, 2, { conclusion: "skipped" }),
    job("no runner", 0, 1, 2, { runner_name: "" }),
    job("not started", 0, 1, 2, { started_at: null }),
    job("not completed", 0, 1, 2, { completed_at: null }),
  ];
  assert.equal(runTimings(workflowRun(), jobs), null);
  assert.equal(runTimings(workflowRun(), []), null);
});

test("runTimings measures the run from the jobs that used a runner", () => {
  const jobs = [
    job("build", 1, 2, 10),
    job("test", 1, 4, 13),
    job("skipped", 0, 0, 30, { conclusion: "skipped" }),
  ];
  assert.deepEqual(runTimings(workflowRun(), jobs), {
    queue: 2,
    run: 11,
    wall: 13,
    jobs: 2,
    jobTimings: [
      { name: "build", queue: 1, run: 8 },
      { name: "test", queue: 3, run: 9 },
    ],
  });
});

test("runTimings starts a re-run at the latest attempt", () => {
  const timings = runTimings(workflowRun({ run_started_at: at(20) }), [job("build", 21, 22, 25)]);
  assert.equal(timings.queue, 2);
  assert.equal(timings.wall, 5);
});

test("summarize returns no workflows for no entries", () => {
  assert.deepEqual(summarize([]), []);
});

test("summarize skips runs without timings", () => {
  assert.deepEqual(summarize([{ workflowRun: workflowRun(), timings: null }]), []);
});

test("summarize groups runs per workflow and sorts jobs by run p90", () => {
  const entry = (extra, jobs) => {
    const value = workflowRun(extra);
    return { workflowRun: value, timings: runTimings(value, jobs) };
  };
  const summary = summarize([
    entry({}, [job("fast", 0, 1, 2), job("slow", 0, 1, 11)]),
    entry({ event: "push" }, [job("fast", 0, 2, 4)]),
    entry({ name: "Docs" }, [job("lint", 0, 1, 3)]),
  ]);

  assert.deepEqual(
    summary.map((item) => [item.workflow, item.runs]),
    [
      ["CI", 2],
      ["Docs", 1],
    ],
  );
  const ci = summary[0];
  assert.deepEqual(ci.wall, { n: 2, p50: 4, p90: 11 });
  // Only the pull request run counts toward jobs per PR run.
  assert.deepEqual(ci.jobsPerPrRun, { n: 1, p50: 2, p90: 2 });
  assert.deepEqual(
    ci.jobs.map((item) => [item.job, item.run.n, item.run.p90]),
    [
      ["slow", 1, 10],
      ["fast", 2, 2],
    ],
  );
});

test("summarize reports null statistics when a workflow has no pull request runs", () => {
  const value = workflowRun({ event: "push" });
  const [summary] = summarize([
    { workflowRun: value, timings: runTimings(value, [job("a", 0, 1, 2)]) },
  ]);
  assert.deepEqual(summary.jobsPerPrRun, { n: 0, p50: null, p90: null });
});

test("the CLI prints the usage for --help", () => {
  const result = spawnSync(process.execPath, [script, "--help"], { encoding: "utf8" });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /^Usage: node scripts\/ci-metrics\.mjs/);
});

test("the CLI exits with 1 and an error for an unknown flag", () => {
  const result = spawnSync(process.execPath, [script, "--bogus", "1"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.equal(result.stderr, "ci-metrics: unknown argument: --bogus\n");
});
