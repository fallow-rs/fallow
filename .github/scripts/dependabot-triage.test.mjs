// Tests for dependabot-triage.mjs, the classifier in dependabot-repair.yml.
//
// Run: node --test .github/scripts/dependabot-triage.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { classify, excerptLog, MAX_REPAIRS, updateSize } from "./dependabot-triage.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./dependabot-triage.mjs", import.meta.url));
const HEAD = "abc123";
const pr = (overrides = {}) => ({ number: 7, headSha: HEAD, labels: [], ...overrides });
const dependabot = { author: "dependabot[bot]" };
const repair = { author: "claude[bot]" };
const job = (name, conclusion) => ({ name, conclusion });

const input = (overrides = {}) => ({
  pr: pr(),
  runHeadSha: HEAD,
  runAttempt: 1,
  commits: [dependabot],
  jobs: [job("Check", "success"), job("JS Lint and Format", "failure"), job("CI", "failure")],
  ...overrides,
});

test("a real failure asks for a repair and names the failed jobs without the aggregator", () => {
  assert.deepEqual(classify(input()), {
    action: "repair",
    reason: "1 job failed",
    failedJobs: ["JS Lint and Format"],
  });
});

test("no open Dependabot pull request for the branch is skipped", () => {
  assert.equal(classify(input({ pr: null })).action, "skip");
});

test("a run for an older head commit is skipped", () => {
  assert.equal(classify(input({ runHeadSha: "old" })).action, "skip");
});

test("a pull request with a commit from a person is left to that person", () => {
  const result = classify(input({ commits: [dependabot, { author: "octocat" }] }));
  assert.equal(result.action, "skip");
  assert.match(result.reason, /octocat/u);
});

test("a run with only cancelled jobs is rerun", () => {
  const jobs = [job("Check", "cancelled"), job("CI", "failure")];
  assert.equal(classify(input({ jobs })).action, "rerun");
});

test("a cancelled run is not rerun forever", () => {
  const jobs = [job("Check", "cancelled"), job("CI", "failure")];
  assert.equal(classify(input({ jobs, runAttempt: 3 })).action, "skip");
});

test("a known flaky job is rerun once, then repaired", () => {
  const jobs = [job("VS Code Extension", "failure"), job("CI", "failure")];
  assert.equal(classify(input({ jobs })).action, "rerun");
  assert.equal(classify(input({ jobs, runAttempt: 2 })).action, "repair");
});

test("a timed-out job counts as a failure", () => {
  const jobs = [job("Check", "timed_out")];
  assert.deepEqual(classify(input({ jobs })).failedJobs, ["Check"]);
});

test("a flaky job next to a real failure is repaired at once", () => {
  const jobs = [job("VS Code Extension", "failure"), job("Typos", "failure")];
  assert.equal(classify(input({ jobs })).action, "repair");
});

test("the repair stops after the maximum number of repair commits", () => {
  const commits = [dependabot, ...Array.from({ length: MAX_REPAIRS }, () => repair)];
  const result = classify(input({ commits }));
  assert.equal(result.action, "give-up");
  assert.deepEqual(result.failedJobs, ["JS Lint and Format"]);
});

test("a run where every job passed is skipped", () => {
  assert.equal(classify(input({ jobs: [job("Check", "success")] })).action, "skip");
});

test("the log excerpt keeps the failed step and drops the steps before it", () => {
  const log = [
    "2026-10-20T07:00:00.0000000Z ##[group]Run npm ci",
    "added 900 packages",
    "2026-10-20T07:00:01.0000000Z ##[group]Run node --test",
    "\u001b[31mnot ok 1 - pins the action\u001b[0m",
    "##[error]Process completed with exit code 1.",
  ].join("\n");
  assert.equal(
    excerptLog(log),
    "##[group]Run node --test\nnot ok 1 - pins the action\n##[error]Process completed with exit code 1.",
  );
});

test("a long step keeps the assertion that a late summary would push out of the tail", () => {
  const filler = (count) => Array.from({ length: count }, (_, index) => `ok ${index}`);
  const log = [
    "##[group]Run node --test",
    ...filler(1000),
    "not ok 684 - fuzz workflow runs every harness",
    "  expected: upload-artifact@043fb46",
    ...filler(1000),
    "##[error]Process completed with exit code 1.",
  ].join("\n");
  const excerpt = excerptLog(log);
  assert.match(excerpt, /not ok 684/u);
  assert.match(excerpt, /expected: upload-artifact@043fb46/u);
  assert.match(excerpt, /exit code 1/u);
  assert.ok(excerpt.split("\n").length < 300);
});

test("the script started as a process checks its environment before any request", () => {
  const run = spawnSync(process.execPath, [SCRIPT_PATH], {
    encoding: "utf8",
    env: { PATH: process.env.PATH, RUN_ID: "1" },
  });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /missing environment: GITHUB_TOKEN, GITHUB_REPOSITORY/u);
  assert.doesNotMatch(run.stderr, /RUN_ID/u);
});

test("a pull request with the needs-human label is left to the maintainer", () => {
  const result = classify(input({ pr: pr({ labels: ["needs-human"] }) }));
  assert.equal(result.action, "skip");
});

test("the update size follows semver, with 0.x minor bumps as major", () => {
  assert.equal(updateSize("chore(deps): bump syn from 3.0.6 to 3.0.9"), "minor-or-patch");
  assert.equal(updateSize("chore(deps): bump rmcp from 1.4.0 to 2.0.0"), "major");
  assert.equal(updateSize("chore(deps): bump jsonschema from 0.57.0 to 0.58.2"), "major");
  assert.equal(
    updateSize("chore(deps): bump actions/checkout from v7.0.1 to v7.1.0"),
    "minor-or-patch",
  );
  assert.equal(updateSize("chore(deps): bump the oxc group with 4 updates"), "unknown");
});

test("the excerpt keeps the first errors when there are more than fit", () => {
  const block = (index) => [
    `error[E0425]: cannot find function f${index}`,
    ...Array(59).fill("  |"),
  ];
  const log = [
    "##[group]Run cargo clippy",
    ...Array(500).fill("Compiling x"),
    ...Array.from({ length: 30 }, (_, index) => block(index)).flat(),
    "##[error]Process completed with exit code 101.",
  ].join("\n");
  const excerpt = excerptLog(log);
  assert.match(excerpt, /cannot find function f0\b/u);
  assert.match(excerpt, /exit code 101/u);
});

test("the excerpt keeps a nextest failure and the error a test returned", () => {
  const log = [
    "##[group]Run cargo nextest run",
    ...Array(1600).fill("        PASS [   0.010s] fallow-core tests::ok"),
    "        FAIL [   0.020s] fallow-core tests::finds_three",
    "--- STDERR:              fallow-core tests::finds_three ---",
    'Error: "expected 3 findings"',
    ...Array(300).fill("        PASS [   0.010s] fallow-core tests::ok"),
    "##[error]Process completed with exit code 100.",
  ].join("\n");
  assert.match(excerptLog(log), /expected 3 findings/u);
});
