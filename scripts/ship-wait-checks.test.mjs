import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  parseGhChecks,
  parseGhMergeable,
  parseGhRuns,
  reportChecks,
  waitForChecks,
} from "./ship-wait-checks.mjs";

const SCRIPT = fileURLToPath(new URL("./ship-wait-checks.mjs", import.meta.url));
const INTERVAL_MS = 1000;

const check = (name, bucket) => ({ name, bucket, link: `https://example.invalid/${name}` });

// Return the reads in order, then repeat the last one. The clock moves one
// interval for each sleep.
const scripted = (reads) => {
  let index = 0;
  let clock = 0;
  return {
    readChecks: () => reads[Math.min(index++, reads.length - 1)],
    sleep: async (ms) => {
      clock += ms;
    },
    now: () => clock,
    reads: () => index,
  };
};

const wait = (script, { minChecks = 1, timeoutMs = 10 * INTERVAL_MS, readMergeable } = {}) =>
  waitForChecks({
    ...script,
    readMergeable,
    minChecks,
    intervalMs: INTERVAL_MS,
    timeoutMs,
    warn: () => {},
  });

// Return the mergeable states in order, then repeat the last one.
const mergeableStates = (states) => {
  let index = 0;
  const read = () => {
    const mergeable = states[Math.min(index++, states.length - 1)];
    return mergeable === null ? { ok: false, error: "network" } : { ok: true, mergeable };
  };
  return { read, reads: () => index };
};

test("the count guard waits until the expected checks exist", async () => {
  const script = scripted([
    { ok: true, checks: [] },
    { ok: true, checks: [check("lint", "pass")] },
    { ok: true, checks: [check("lint", "pass"), check("test", "pending")] },
    { ok: true, checks: [check("lint", "pass"), check("test", "pass")] },
  ]);

  const result = await wait(script, { minChecks: 2 });

  assert.equal(result.status, "pass");
  assert.equal(script.reads(), 4);
});

test("a failed or cancelled check makes the result fail", async () => {
  const script = scripted([
    { ok: true, checks: [check("lint", "pass"), check("test", "fail"), check("docs", "cancel")] },
  ]);

  const result = await wait(script);
  const lines = [];
  const code = reportChecks(result, { label: "PR 7", minChecks: 1 }, (line) => lines.push(line));

  assert.equal(result.status, "fail");
  assert.equal(code, 1);
  assert.deepEqual(lines, [
    "PR 7: checks failed (cancel=1 fail=1 pass=1).",
    "  FAIL: test https://example.invalid/test",
    "  CANCEL: docs https://example.invalid/docs",
  ]);
});

test("a cancelled run of a commit adds a hint to wait for a newer commit", async () => {
  const result = await wait(scripted([{ ok: true, checks: [check("CI", "cancel")] }]));
  const lines = [];
  const code = reportChecks(
    result,
    { label: "Commit abc", minChecks: 1, commit: "a".repeat(40) },
    (line) => lines.push(line),
  );

  assert.equal(code, 1);
  assert.equal(lines.length, 3);
  assert.match(lines[2], /newer push .* --commit \$\(git rev-parse origin\/main\)/u);
});

test("the wait times out while too few checks exist", async () => {
  const script = scripted([{ ok: true, checks: [check("lint", "pass")] }]);

  const result = await wait(script, { minChecks: 3, timeoutMs: 5 * INTERVAL_MS });
  const lines = [];
  const code = reportChecks(result, { label: "PR 7", minChecks: 3 }, (line) => lines.push(line));

  assert.equal(result.status, "timeout");
  assert.equal(code, 2);
  assert.deepEqual(lines, ["PR 7: timed out with 1 of at least 3 checks (pass=1)."]);
});

test("a pull request that conflicts with its base stops the wait with exit code 3", async () => {
  const script = scripted([{ ok: true, checks: [check("lint", "pass")] }]);
  const states = mergeableStates(["UNKNOWN", null, "CONFLICTING"]);

  const result = await wait(script, { minChecks: 3, readMergeable: states.read });
  const lines = [];
  const code = reportChecks(result, { label: "PR 7", minChecks: 3 }, (line) => lines.push(line));

  assert.equal(result.status, "conflict");
  assert.equal(states.reads(), 3);
  assert.equal(code, 3);
  assert.equal(lines.length, 1);
  assert.match(lines[0], /^PR 7: the pull request conflicts with its base branch/u);
  assert.match(lines[0], /no pull_request workflows/u);
});

test("an unknown or mergeable state keeps the wait going", async () => {
  const script = scripted([
    { ok: true, checks: [] },
    { ok: true, checks: [check("lint", "pending")] },
    { ok: true, checks: [check("lint", "pass")] },
  ]);
  const states = mergeableStates(["UNKNOWN", "MERGEABLE"]);

  const result = await wait(script, { readMergeable: states.read });

  assert.equal(result.status, "pass");
  assert.equal(script.reads(), 3);
});

test("complete checks win over a conflict", async () => {
  const states = mergeableStates(["CONFLICTING"]);

  const result = await wait(scripted([{ ok: true, checks: [check("lint", "fail")] }]), {
    readMergeable: states.read,
  });

  assert.equal(result.status, "fail");
  assert.equal(states.reads(), 0);
});

test("read errors in a row stop the wait, and a good read resets the count", async () => {
  const failure = { ok: false, error: "network" };
  const passing = { ok: true, checks: [check("lint", "pass")] };
  const pending = { ok: true, checks: [check("lint", "pending")] };

  const recovered = await wait(scripted([failure, failure, pending, failure, failure, passing]));
  const stopped = await wait(scripted([failure, failure, failure, passing]));

  assert.equal(recovered.status, "pass");
  assert.equal(stopped.status, "error");
  const lines = [];
  const code = reportChecks(stopped, { label: "PR 7", minChecks: 1 }, (line) => lines.push(line));
  assert.equal(code, 2);
  assert.deepEqual(lines, ["PR 7: stopped after 3 failed reads of the checks."]);
});

test("parseGhChecks reads the JSON when gh exits with 0 or with 8 for a pending check", () => {
  const checks = [check("lint", "pass"), check("test", "pending")];
  const stdout = JSON.stringify(checks);

  assert.deepEqual(parseGhChecks({ status: 0, stdout, stderr: "" }), { ok: true, checks });
  assert.deepEqual(parseGhChecks({ status: 8, stdout, stderr: "" }), { ok: true, checks });
});

test("parseGhChecks reads both gh messages for a pull request without checks", () => {
  for (const stderr of [
    "no checks reported on the 'feat' branch\n",
    "no required checks reported on the 'feat' branch\n",
  ]) {
    assert.deepEqual(parseGhChecks({ status: 1, stdout: "", stderr }), { ok: true, checks: [] });
  }
});

test("parseGhChecks reports a failed run, invalid JSON and a spawn error", () => {
  assert.deepEqual(parseGhChecks({ status: 1, stdout: "", stderr: "HTTP 502\n" }), {
    ok: false,
    error: "HTTP 502",
  });
  assert.deepEqual(parseGhChecks({ status: 4, stdout: "", stderr: "" }), {
    ok: false,
    error: "gh exited with 4",
  });
  assert.match(parseGhChecks({ status: 0, stdout: "{", stderr: "" }).error, /invalid JSON/u);
  assert.deepEqual(parseGhChecks({ error: new Error("spawn gh ENOENT") }), {
    ok: false,
    error: "spawn gh ENOENT",
  });
});

test("parseGhMergeable reads the mergeable field of gh pr view", () => {
  assert.deepEqual(
    parseGhMergeable({
      status: 0,
      stdout: JSON.stringify({ mergeable: "CONFLICTING", mergeStateStatus: "DIRTY" }),
      stderr: "",
    }),
    { ok: true, mergeable: "CONFLICTING" },
  );
  assert.deepEqual(parseGhMergeable({ status: 1, stdout: "", stderr: "HTTP 502\n" }), {
    ok: false,
    error: "HTTP 502",
  });
  assert.match(parseGhMergeable({ status: 0, stdout: "{", stderr: "" }).error, /invalid JSON/u);
  assert.deepEqual(parseGhMergeable({ error: new Error("spawn gh ENOENT") }), {
    ok: false,
    error: "spawn gh ENOENT",
  });
});

test("parseGhRuns turns each workflow run into a check", () => {
  const run = (workflowName, status, conclusion) => ({
    workflowName,
    status,
    conclusion,
    url: `https://example.invalid/${workflowName}`,
  });
  const stdout = JSON.stringify([
    run("CI", "in_progress", ""),
    run("Lint", "completed", "success"),
    run("Bench", "completed", "skipped"),
    run("Docs", "completed", "cancelled"),
    run("Coverage", "completed", "timed_out"),
  ]);

  const read = parseGhRuns({ status: 0, stdout, stderr: "" });

  assert.deepEqual(
    read.checks.map(({ name, bucket }) => `${name}=${bucket}`),
    ["CI=pending", "Lint=pass", "Bench=skipping", "Docs=cancel", "Coverage=fail"],
  );
});

test("the CLI needs exactly one of a pull request and a commit", () => {
  for (const args of [
    ["--min-checks", "3"],
    ["--pr", "7", "--commit", "a".repeat(40)],
  ]) {
    const result = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });

    assert.equal(result.status, 2);
    assert.match(result.stderr, /exactly one of --pr and --commit/u);
  }
});

test("the CLI rejects a short commit SHA, which gh run list never matches", () => {
  const result = spawnSync(process.execPath, [SCRIPT, "--commit", "08c83d5"], {
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.match(result.stderr, /full 40-character SHA/u);
});

test("the CLI rejects a count that is not a positive integer", () => {
  const result = spawnSync(process.execPath, [SCRIPT, "--pr", "7", "--min-checks", "0"], {
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.match(result.stderr, /--min-checks must be a positive integer/u);
});
