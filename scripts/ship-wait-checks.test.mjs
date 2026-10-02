import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  followMain,
  parseGhChecks,
  parseGhMergeable,
  parseGhRuns,
  parseGitAncestor,
  parseGitHead,
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

const wait = (
  script,
  { minChecks = 1, timeoutMs = 10 * INTERVAL_MS, readMergeable, settleMs = null } = {},
) =>
  waitForChecks({
    ...script,
    readMergeable,
    settleMs,
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

test("commit runs that stay complete for the settle window end the wait below the count", async () => {
  const script = scripted([{ ok: true, checks: [check("CI", "pass"), check("Lint", "pass")] }]);

  const result = await wait(script, {
    minChecks: 3,
    timeoutMs: 20 * INTERVAL_MS,
    settleMs: 3 * INTERVAL_MS,
  });
  const lines = [];
  const code = reportChecks(
    result,
    { label: "Commit abc", minChecks: 3, settleMs: 3 * INTERVAL_MS },
    (line) => lines.push(line),
  );

  assert.equal(result.status, "settled");
  assert.equal(script.reads(), 4);
  assert.equal(code, 0);
  assert.match(lines[0], /^Commit abc: all 2 runs passed, fewer than --min-checks 3/u);
});

test("a new or changed run starts the settle window again", async () => {
  const script = scripted([
    { ok: true, checks: [check("CI", "pass")] },
    { ok: true, checks: [check("CI", "pass")] },
    { ok: true, checks: [check("CI", "pass"), check("Bench", "pass")] },
  ]);

  const result = await wait(script, { minChecks: 3, settleMs: 2 * INTERVAL_MS });

  assert.equal(result.status, "settled");
  assert.equal(script.reads(), 5);
  assert.equal(result.checks.length, 2);
});

test("a failed run that settles below the count still fails", async () => {
  const script = scripted([{ ok: true, checks: [check("CI", "fail")] }]);

  const result = await wait(script, { minChecks: 3, settleMs: 2 * INTERVAL_MS });

  assert.equal(result.status, "fail");
});

test("a pending run never settles", async () => {
  const script = scripted([{ ok: true, checks: [check("CI", "pending")] }]);

  const result = await wait(script, {
    minChecks: 3,
    timeoutMs: 6 * INTERVAL_MS,
    settleMs: 2 * INTERVAL_MS,
  });

  assert.equal(result.status, "timeout");
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

test("the CLI accepts --follow-main only with --commit and without --repo", () => {
  for (const [args, message] of [
    [["--pr", "7", "--follow-main"], /--follow-main works only with --commit/u],
    [
      ["--commit", "a".repeat(40), "--repo", "fallow-rs/docs", "--follow-main"],
      /does not work with --repo/u,
    ],
  ]) {
    const result = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });

    assert.equal(result.status, 2);
    assert.match(result.stderr, message);
  }
});

const ORIGIN = "a".repeat(40);
const NEWER = "b".repeat(40);
const NEWEST = "c".repeat(40);

// Give each commit a fixed wait result. Each wait spends one interval on the
// clock. Return the newest commits of main in order, then the last one again.
const following = ({ results, heads, ancestors = new Set([NEWER, NEWEST]) }) => {
  let clock = 0;
  let headIndex = 0;
  const waits = [];
  const lines = [];
  return {
    waitCommit: async (sha, timeoutMs) => {
      waits.push({ sha, timeoutMs });
      clock += INTERVAL_MS;
      return results.get(sha);
    },
    readHead: () => heads[Math.min(headIndex++, heads.length - 1)],
    isAncestor: (ancestor, descendant) => ({
      ok: true,
      contains: ancestor === ORIGIN && ancestors.has(descendant),
    }),
    now: () => clock,
    log: (line) => lines.push(line),
    waits,
    lines,
  };
};

const cancelled = { status: "fail", checks: [check("CI", "cancel"), check("Lint", "pass")] };
const passed = { status: "pass", checks: [check("CI", "pass"), check("Lint", "skipping")] };
const head = (sha) => ({ ok: true, sha });

const follow = (script, { maxHops } = {}) =>
  followMain({ ...script, commit: ORIGIN, timeoutMs: 10 * INTERVAL_MS, maxHops });

const report = (result, label) => {
  const lines = [];
  const code = reportChecks(
    result,
    { label, minChecks: 1, commit: ORIGIN, followMain: true },
    (line) => lines.push(line),
  );
  return { code, lines };
};

test("--follow-main waits for a newer commit after a cancel and passes", async () => {
  const script = following({
    results: new Map([
      [ORIGIN, cancelled],
      [NEWER, passed],
    ]),
    heads: [head(NEWER)],
  });

  const result = await follow(script);
  const { code, lines } = report(result, "Commit bbbbbbbbbbbb");

  assert.equal(result.status, "pass");
  assert.equal(result.commit, NEWER);
  assert.equal(code, 0);
  assert.deepEqual(
    script.waits.map(({ sha }) => sha),
    [ORIGIN, NEWER],
  );
  // The second wait gets only the rest of the total budget.
  assert.deepEqual(
    script.waits.map(({ timeoutMs }) => timeoutMs),
    [10 * INTERVAL_MS, 9 * INTERVAL_MS],
  );
  assert.deepEqual(script.lines, [
    "Commit aaaaaaaaaaaa: runs cancelled (cancel=1 pass=1). Wait for bbbbbbbbbbbb of origin/main (move 1 of 10).",
  ]);
  assert.deepEqual(lines, ["Commit bbbbbbbbbbbb: all checks passed (pass=1 skipping=1)."]);
});

test("--follow-main stops on a real failure, also next to a cancel", async () => {
  const failed = { status: "fail", checks: [check("CI", "fail"), check("Docs", "cancel")] };
  const script = following({
    results: new Map([
      [ORIGIN, cancelled],
      [NEWER, failed],
    ]),
    heads: [head(NEWER), head(NEWEST)],
  });

  const result = await follow(script);
  const { code, lines } = report(result, "Commit bbbbbbbbbbbb");

  assert.equal(result.status, "fail");
  assert.equal(result.commit, NEWER);
  assert.equal(code, 1);
  assert.equal(script.waits.length, 2);
  assert.deepEqual(lines, [
    "Commit bbbbbbbbbbbb: checks failed (cancel=1 fail=1).",
    "  FAIL: CI https://example.invalid/CI",
    "  CANCEL: Docs https://example.invalid/Docs",
  ]);
});

test("--follow-main stops when the newest commit does not contain the original", async () => {
  const script = following({
    results: new Map([[ORIGIN, cancelled]]),
    heads: [head(NEWER)],
    ancestors: new Set(),
  });

  const result = await follow(script);
  const { code, lines } = report(result, "Commit aaaaaaaaaaaa");

  assert.equal(result.status, "diverged");
  assert.equal(code, 2);
  assert.equal(script.waits.length, 1);
  assert.deepEqual(lines, [
    "Commit aaaaaaaaaaaa: origin/main at bbbbbbbbbbbb does not contain commit aaaaaaaaaaaa, so the wait stops.",
    "  CANCEL: CI https://example.invalid/CI",
  ]);
});

test("--follow-main stops at the maximum count of moves", async () => {
  const script = following({
    results: new Map([
      [ORIGIN, cancelled],
      [NEWER, cancelled],
      [NEWEST, cancelled],
    ]),
    heads: [head(NEWER), head(NEWEST)],
  });

  const result = await follow(script, { maxHops: 2 });
  const { code, lines } = report(result, "Commit cccccccccccc");

  assert.equal(result.status, "hops");
  assert.equal(result.commit, NEWEST);
  assert.equal(code, 1);
  assert.equal(script.waits.length, 3);
  assert.equal(script.lines.length, 2);
  assert.match(lines[0], /^Commit cccccccccccc: runs cancelled again after 2 moves/u);
});

test("--follow-main stops when main has not moved or cannot be read", async () => {
  const unmoved = following({ results: new Map([[ORIGIN, cancelled]]), heads: [head(ORIGIN)] });
  const unread = following({
    results: new Map([[ORIGIN, cancelled]]),
    heads: [{ ok: false, error: "fetch failed" }],
  });

  const same = await follow(unmoved);
  const failedRead = await follow(unread);

  assert.equal(same.status, "fail");
  assert.match(unmoved.lines[0], /is the newest commit of origin\/main/u);
  assert.equal(failedRead.status, "head-error");
  const { code, lines } = report(failedRead, "Commit aaaaaaaaaaaa");
  assert.equal(code, 2);
  assert.equal(lines[0], "Commit aaaaaaaaaaaa: read of origin/main failed: fetch failed");
});

test("parseGitHead and parseGitAncestor read the git results", () => {
  assert.deepEqual(parseGitHead({ status: 0, stdout: `${NEWER}\n`, stderr: "" }), head(NEWER));
  assert.deepEqual(parseGitHead({ status: 128, stdout: "", stderr: "fatal: bad ref\n" }), {
    ok: false,
    error: "fatal: bad ref",
  });
  assert.deepEqual(parseGitAncestor({ status: 0, stderr: "" }), { ok: true, contains: true });
  assert.deepEqual(parseGitAncestor({ status: 1, stderr: "" }), { ok: true, contains: false });
  assert.deepEqual(parseGitAncestor({ status: 128, stderr: "fatal: not a commit\n" }), {
    ok: false,
    error: "fatal: not a commit",
  });
});
