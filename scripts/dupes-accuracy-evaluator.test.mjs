import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { makeTempDir } from "./lib/temp-dir.mjs";

const CORPUS_DIR = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "tests",
  "benchmark-corpus",
);
const RESULTS_DIR = join(CORPUS_DIR, "results");
const EVALUATOR = join(CORPUS_DIR, "evaluate-results.py");

// Copies the committed results of each mode, then removes from the `defaults`
// result each clone group that has an instance in a file of `droppedDir`.
const resultsWithout = (droppedDir) => {
  const dir = makeTempDir("fallow-dupes-accuracy-");
  for (const name of readdirSync(RESULTS_DIR).filter((file) => file.startsWith("dupes-"))) {
    copyFileSync(join(RESULTS_DIR, name), join(dir, name));
  }
  const defaultsPath = join(dir, "dupes-defaults.json");
  const report = JSON.parse(readFileSync(defaultsPath, "utf8"));
  report.clone_groups = report.clone_groups.filter(
    (group) => !group.instances.some((instance) => instance.file.includes(`/${droppedDir}/`)),
  );
  writeFileSync(defaultsPath, JSON.stringify(report));
  return dir;
};

const evaluate = (resultsDir) =>
  spawnSync("python3", [EVALUATOR, "--results-dir", resultsDir], {
    encoding: "utf8",
    timeout: 60_000,
  });

test("the committed results meet the committed accuracy floor", () => {
  const result = evaluate(RESULTS_DIR);

  assert.equal(result.status, 0, result.stdout + result.stderr);
});

test("a lost exact clone in the default run lowers its recall below the floor", () => {
  const result = evaluate(resultsWithout("type1-exact"));

  assert.notEqual(result.status, 0, result.stdout);
  assert.match(result.stdout + result.stderr, /defaults: recall/);
});

test("a lost near-miss clone in the default run lowers its recall below the floor", () => {
  const result = evaluate(resultsWithout("type3-near-miss"));

  assert.notEqual(result.status, 0, result.stdout);
  assert.match(result.stdout + result.stderr, /defaults: recall/);
});
