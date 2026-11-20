import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { makeTempDir } from "./lib/temp-dir.mjs";

const BENCH_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", "benchmarks");
// An empty PATH makes a `cargo build` that starts by mistake fail at once.
const EMPTY_PATH = makeTempDir("fallow-bench-options-");
const RUNS_SCRIPTS = [
  "bench.mjs",
  "bench-dupes.mjs",
  "bench-circular.mjs",
  "compare-dupes-rolling.mjs",
];

const runBench = (script, args) =>
  spawnSync(process.execPath, [join(BENCH_DIR, script), ...args], {
    encoding: "utf8",
    timeout: 10_000,
    env: { ...process.env, PATH: EMPTY_PATH },
  });

const assertRejected = (result, message) => {
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, message);
  assert.doesNotMatch(result.stdout, /Building fallow/);
};

for (const script of RUNS_SCRIPTS) {
  test(`${script} rejects --runs=0 before the build`, () => {
    assertRejected(runBench(script, ["--runs=0"]), /--runs must be a positive integer/);
  });

  test(`${script} rejects --runs=3abc before the build`, () => {
    assertRejected(runBench(script, ["--runs=3abc"]), /--runs must be a positive integer/);
  });
}

test("bench.mjs rejects a fractional --warmup before the build", () => {
  assertRejected(
    runBench("bench.mjs", ["--runs=1", "--warmup=1.5"]),
    /--warmup must be a non-negative integer/,
  );
});

test("compare-dupes-rolling.mjs rejects a negative --samples before the build", () => {
  assertRejected(
    runBench("compare-dupes-rolling.mjs", ["--samples=-1"]),
    /--samples must be a non-negative integer/,
  );
});

const compareArgs = (dir) => [`--fixtures=${dir}`, `--fallow-bin=${join(dir, "missing-fallow")}`];

test("compare.mjs fails when the fixture directory has no project", () => {
  const dir = makeTempDir("fallow-bench-compare-");
  const result = runBench("compare.mjs", compareArgs(dir));

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /no project gave a usable report/);
});

test("compare.mjs reports a tool that does not start and fails", () => {
  const dir = makeTempDir("fallow-bench-compare-");
  mkdirSync(join(dir, "demo"));
  writeFileSync(join(dir, "demo", "package.json"), "{}");
  const result = runBench("compare.mjs", compareArgs(dir));

  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /demo/);
  assert.match(result.stdout, /failed to start/);
  assert.match(result.stderr, /no project gave a usable report/);
});

test("compare.mjs looks up a bare --fallow-bin name on PATH", () => {
  const dir = makeTempDir("fallow-bench-compare-");
  mkdirSync(join(dir, "demo"));
  writeFileSync(join(dir, "demo", "package.json"), "{}");
  const result = runBench("compare.mjs", [`--fixtures=${dir}`, "--fallow-bin=no-such-fallow"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /spawnSync no-such-fallow ENOENT/);
});

test("bench.mjs names the inline form for a separate --runs value", () => {
  assertRejected(
    runBench("bench.mjs", ["--runs", "3"]),
    /Missing value for --runs: write --runs=VALUE/,
  );
});

test("bench-circular.mjs rejects an unknown argument before the build", () => {
  assertRejected(runBench("bench-circular.mjs", ["--bogus"]), /Unknown argument: --bogus/);
});

test("compare-dupes-rolling.mjs rejects an unknown argument before the build", () => {
  assertRejected(
    runBench("compare-dupes-rolling.mjs", ["--project=x"]),
    /Unknown argument: --project=x/,
  );
});

test("compare.mjs rejects a misspelled option", () => {
  const dir = makeTempDir("fallow-bench-compare-");
  const result = runBench("compare.mjs", [`--fixture=${dir}`]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unknown argument: --fixture=/);
});
