import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  COMMIT_SETTLE_MS,
  followMain,
  parseGhChecks,
  parseGhMergeable,
  parseGhRuns,
  parseGitAncestor,
  parseGitHead,
  reportChecks,
  waitForChecks,
  waitForCommitRuns,
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
  assert.match(lines[0], /^Commit abc: all 2 checks passed, fewer than --min-checks 3/u);
});

// The merge of #3611 changed only a workflow, docs and a policy test. Path
// filters started 5 runs, and CI ran for most of the timeout.
const pathFilteredRuns = (bucket) => [
  check("CI", bucket),
  check("Commitlint", "pass"),
  check("Scorecard", "pass"),
  check("CodeQL", "pass"),
  check("Coverage", "pass"),
];
const COMMIT_INTERVAL_MS = 30 * 1000;
const COMMIT_TIMEOUT_MS = 30 * 60 * 1000;

const waitCommit = (script, { minChecks = null } = {}) =>
  waitForCommitRuns({
    ...script,
    minChecks,
    intervalMs: COMMIT_INTERVAL_MS,
    timeoutMs: COMMIT_TIMEOUT_MS,
    warn: () => {},
  });

test("a commit with fewer runs than --min-checks passes soon after its runs complete", async () => {
  const pendingReads = Array.from({ length: 50 }, () => ({
    ok: true,
    checks: pathFilteredRuns("pending"),
  }));
  const script = scripted([...pendingReads, { ok: true, checks: pathFilteredRuns("pass") }]);

  const result = await waitCommit(script, { minChecks: 40 });

  assert.equal(result.status, "settled");
  assert.equal(result.checks.length, 5);
});

test("a commit wait needs no --min-checks", async () => {
  const script = scripted([
    { ok: true, checks: [] },
    { ok: true, checks: pathFilteredRuns("pending") },
    { ok: true, checks: pathFilteredRuns("pass") },
  ]);

  const result = await waitCommit(script);
  const lines = [];
  const code = reportChecks(
    result,
    { label: "Commit abc", minChecks: null, settleMs: COMMIT_SETTLE_MS },
    (line) => lines.push(line),
  );

  assert.equal(result.status, "settled");
  assert.equal(code, 0);
  assert.match(lines[0], /^Commit abc: all 5 runs passed, and no run started or changed/u);
});

test("a commit wait does not pass on a read without runs", async () => {
  const script = scripted([{ ok: true, checks: [] }]);

  const result = await waitCommit(script);
  const lines = [];
  const code = reportChecks(result, { label: "Commit abc", minChecks: null }, (line) =>
    lines.push(line),
  );

  assert.equal(result.status, "timeout");
  assert.equal(code, 2);
  assert.deepEqual(lines, ["Commit abc: timed out with 0 checks (no checks)."]);
});

test("a commit run that starts after the others complete restarts the settle time", async () => {
  const done = pathFilteredRuns("pass");
  const script = scripted([
    { ok: true, checks: done },
    { ok: true, checks: done },
    { ok: true, checks: [...done, check("Release", "pending")] },
    { ok: true, checks: [...done, check("Release", "pass")] },
  ]);

  const result = await waitCommit(script);

  // Release completes at the fourth read (90 seconds). The settle time then
  // ends at 210 seconds, the eighth read. Without the restart, it ends at
  // 120 seconds, the fifth read.
  assert.equal(result.status, "settled");
  assert.equal(result.checks.length, 6);
  assert.equal(script.reads(), 8);
});

test("pull request checks that stay complete for the settle window end the wait below the count", async () => {
  const script = scripted([{ ok: true, checks: [check("CI", "pass"), check("Lint", "skipping")] }]);

  const result = await wait(script, {
    minChecks: 3,
    timeoutMs: 20 * INTERVAL_MS,
    settleMs: 3 * INTERVAL_MS,
    readMergeable: mergeableStates(["MERGEABLE"]).read,
  });

  assert.equal(result.status, "settled");
  assert.equal(script.reads(), 4);
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

const MINUTE_MS = 60 * 1000;
const failure = { ok: false, error: "HTTP 504" };
// More failures than any error window allows.
const ENDLESS = 100;

// Fail `failures` times, then pass. Record each sleep and each warning.
const outage = (failures, { intervalMs = 30 * 1000, timeoutMs = 60 * MINUTE_MS } = {}) => {
  const script = scripted([
    ...Array.from({ length: failures }, () => failure),
    { ok: true, checks: [check("lint", "pass")] },
  ]);
  const sleeps = [];
  const warnings = [];
  const result = waitForChecks({
    ...script,
    sleep: async (ms) => {
      sleeps.push(ms);
      await script.sleep(ms);
    },
    minChecks: 1,
    intervalMs,
    timeoutMs,
    warn: (line) => warnings.push(line),
  });
  return { script, sleeps, warnings, result };
};

test("a short outage backs off and the wait goes on", async () => {
  const { script, sleeps, warnings, result } = outage(4);

  assert.equal((await result).status, "pass");
  assert.equal(script.reads(), 5);
  assert.deepEqual(
    sleeps,
    [30, 60, 120, 240].map((seconds) => seconds * 1000),
  );
  assert.deepEqual(warnings, [
    "Read of the checks failed: HTTP 504. Next read in 30 seconds.",
    "Read of the checks failed: HTTP 504. Next read in 60 seconds.",
    "Read of the checks failed: HTTP 504. Next read in 120 seconds.",
    "Read of the checks failed: HTTP 504. Next read in 240 seconds.",
  ]);
});

test("the backoff stops at the cap and the error window stops the wait", async () => {
  const { sleeps, warnings, result } = outage(ENDLESS);
  const stopped = await result;

  assert.equal(stopped.status, "error");
  assert.equal(stopped.readErrors, 6);
  assert.equal(stopped.errorMs, 10 * MINUTE_MS);
  // The last delay is cut to the end of the 10-minute window.
  assert.deepEqual(
    sleeps,
    [30, 60, 120, 240, 150].map((seconds) => seconds * 1000),
  );
  assert.equal(warnings.at(-1), "Read of the checks failed: HTTP 504");
  const lines = [];
  const code = reportChecks(stopped, { label: "PR 7", minChecks: 1 }, (line) => lines.push(line));
  assert.equal(code, 2);
  assert.deepEqual(lines, ["PR 7: stopped after 6 failed reads of the checks in 600 seconds."]);
});

test("the delay never grows above 5 minutes", async () => {
  const { sleeps, result } = outage(ENDLESS, { intervalMs: 4 * MINUTE_MS });

  assert.equal((await result).status, "error");
  assert.deepEqual(
    sleeps,
    [4, 5, 1].map((minutes) => minutes * MINUTE_MS),
  );
});

test("the deadline stops failed reads before the error window", async () => {
  const { sleeps, result } = outage(ENDLESS, { timeoutMs: 2 * MINUTE_MS });
  const stopped = await result;

  assert.equal(stopped.status, "error");
  assert.deepEqual(sleeps, [30 * 1000, 60 * 1000]);
});

test("a suspended machine does not use up the error window", async () => {
  // Each sleep takes 16 minutes, for example on a laptop in sleep mode.
  const script = scripted([
    failure,
    failure,
    failure,
    { ok: true, checks: [check("lint", "pass")] },
  ]);
  const warnings = [];

  const result = await waitForChecks({
    ...script,
    sleep: async () => {
      await script.sleep(16 * MINUTE_MS);
    },
    minChecks: 1,
    intervalMs: 30 * 1000,
    timeoutMs: 120 * MINUTE_MS,
    warn: (line) => warnings.push(line),
  });

  assert.equal(result.status, "pass");
  assert.equal(script.reads(), 4);
  assert.equal(warnings.length, 3);
});

test("a good read resets the backoff and the error window", async () => {
  const passing = { ok: true, checks: [check("lint", "pass")] };
  const pending = { ok: true, checks: [check("lint", "pending")] };
  const script = scripted([failure, failure, failure, pending, failure, failure, passing]);
  const sleeps = [];

  const result = await waitForChecks({
    ...script,
    sleep: async (ms) => {
      sleeps.push(ms);
      await script.sleep(ms);
    },
    minChecks: 1,
    intervalMs: INTERVAL_MS,
    timeoutMs: MINUTE_MS,
    errorWindowMs: 5 * INTERVAL_MS,
    warn: () => {},
  });

  assert.equal(result.status, "pass");
  assert.deepEqual(
    sleeps,
    [1, 2, 2, 1, 1, 2].map((seconds) => seconds * INTERVAL_MS),
  );
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

test("parseGhChecks accepts failed-check JSON for required-name selection", () => {
  const checks = [check("Commit messages", "fail")];
  assert.deepEqual(parseGhChecks({ status: 1, stdout: JSON.stringify(checks), stderr: "" }), {
    ok: true,
    checks,
  });
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

const checkRun = (name, status, conclusion) => ({
  __typename: "CheckRun",
  name,
  status,
  conclusion,
  detailsUrl: `https://example.invalid/current/${name}`,
});

const runWithFakeGh = ({ selected, rollups, runs = workflowRuns() }, args = []) => {
  const root = mkdtempSync(join(tmpdir(), "ship-wait-checks-"));
  try {
    writeFileSync(join(root, "responses.json"), JSON.stringify({ selected, rollups, runs }));
    writeFileSync(
      join(root, "gh"),
      [
        "#!/usr/bin/env node",
        'const fs = require("node:fs");',
        'const path = require("node:path");',
        'const responses = JSON.parse(fs.readFileSync(path.join(__dirname, "responses.json"), "utf8"));',
        'const callsPath = path.join(__dirname, "calls.json");',
        'const calls = fs.existsSync(callsPath) ? JSON.parse(fs.readFileSync(callsPath, "utf8")) : [];',
        "const args = process.argv.slice(2);",
        "calls.push(args);",
        "fs.writeFileSync(callsPath, JSON.stringify(calls));",
        'if (args[0] === "pr" && args[1] === "checks" && typeof responses.selected === "string") {',
        "  process.stderr.write(responses.selected);",
        "  process.exit(1);",
        '} else if (args[0] === "pr" && args[1] === "checks") {',
        "  process.stdout.write(JSON.stringify(responses.selected));",
        '} else if (args.some((arg) => arg.includes("statusCheckRollup"))) {',
        '  const index = calls.filter((call) => call.some((arg) => arg.includes("statusCheckRollup"))).length - 1;',
        "  const reply = responses.rollups[Math.min(index, responses.rollups.length - 1)];",
        '  process.stdout.write(typeof reply === "string" ? reply : JSON.stringify(reply));',
        '} else if (args[0] === "api") {',
        "  const reply = responses.runs[args[1]];",
        '  if (!reply || reply.error) { process.stderr.write(reply?.error ?? "missing workflow run"); process.exit(1); }',
        "  process.stdout.write(JSON.stringify(reply));",
        '} else if (args[0] === "pr" && args[1] === "view") {',
        '  process.stdout.write(JSON.stringify({ mergeable: "MERGEABLE", mergeStateStatus: "BLOCKED" }));',
        "} else {",
        '  process.stderr.write("unexpected gh command");',
        "  process.exit(1);",
        "}",
      ].join("\n"),
      { mode: 0o755 },
    );
    const result = spawnSync(
      process.execPath,
      [SCRIPT, "--pr", "7", "--interval", "1", "--timeout", "1", "--error-window", "1", ...args],
      { env: { ...process.env, PATH: `${root}${delimiter}${process.env.PATH}` }, encoding: "utf8" },
    );
    const calls = existsSync(join(root, "calls.json"))
      ? JSON.parse(readFileSync(join(root, "calls.json"), "utf8"))
      : [];
    return { ...result, calls };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const rollupReads = (calls) =>
  calls.filter((args) => args.some((arg) => arg.includes("statusCheckRollup")));
const fakeGhOptions = { skip: process.platform === "win32" };

const EARLIER_START = "2026-10-05T11:05:44Z";
const NEWER_START = "2026-10-05T11:05:51Z";
const EARLIER_RUN = "200";
const NEWER_RUN = "100";
const RUN_ENDPOINT = "repos/example/project/actions/runs";
const RUN_URL = "https://github.com/example/project/actions/runs";
const workflowRuns = () =>
  Object.fromEntries(
    [
      [EARLIER_RUN, EARLIER_START],
      [NEWER_RUN, NEWER_START],
    ].map(([id, created_at]) => [
      `${RUN_ENDPOINT}/${id}`,
      { created_at, html_url: `${RUN_URL}/${id}`, workflow_id: 7, head_sha: "a".repeat(40) },
    ]),
  );
const attempt = (
  conclusion,
  startedAt,
  workflowName = "Commitlint",
  run = startedAt === EARLIER_START ? EARLIER_RUN : NEWER_RUN,
) => ({
  ...checkRun("Commit messages", "COMPLETED", conclusion),
  startedAt,
  workflowName,
  detailsUrl: `${RUN_URL}/${run}/job/1`,
});

const matrixReplacement = () => {
  const runs = workflowRuns();
  Object.assign(runs[`${RUN_ENDPOINT}/${NEWER_RUN}`], {
    status: "completed",
    conclusion: "success",
  });
  return {
    selected: [],
    runs,
    rollups: [
      {
        statusCheckRollup: [
          {
            ...attempt("CANCELLED", EARLIER_START, "Benchmarks"),
            name: "Simulation (${{ matrix.label }})",
          },
          { ...attempt("SUCCESS", NEWER_START, "Benchmarks"), name: "Simulation (Linux)" },
          { ...attempt("SUCCESS", NEWER_START, "Benchmarks"), name: "Simulation (macOS)" },
        ],
      },
    ],
  };
};

test("a successful replacement retires a canceled matrix placeholder", fakeGhOptions, () => {
  for (const names of [null, ["Simulation (Linux)", "Simulation (macOS)"]]) {
    const fixture = matrixReplacement();
    fixture.selected = (names ?? []).map((name) => check(name, "pass"));
    const result = runWithFakeGh(fixture, names === null ? [] : ["--required"]);
    assert.equal(result.status, 0, result.stdout + result.stderr);
  }
});

test(
  "required mode retains a canceled placeholder even with selected replacement jobs",
  fakeGhOptions,
  () => {
    for (const replacements of [[], [check("Simulation (Linux)", "pass")]]) {
      const fixture = matrixReplacement();
      fixture.selected = [check("Simulation (${{ matrix.label }})", "cancel"), ...replacements];
      const result = runWithFakeGh(fixture, ["--required"]);
      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
  },
);

test("renamed matrix jobs retain current failures and cancellations", fakeGhOptions, () => {
  for (const [status, conclusion] of [
    ["COMPLETED", "FAILURE"],
    ["COMPLETED", "CANCELLED"],
  ]) {
    const fixture = matrixReplacement();
    Object.assign(fixture.rollups[0].statusCheckRollup[1], { status, conclusion });
    const result = runWithFakeGh(fixture);
    assert.equal(result.status, 1, result.stdout + result.stderr);
  }
});

test("renamed matrix jobs wait for the replacement to finish", fakeGhOptions, () => {
  const fixture = matrixReplacement();
  const completed = fixture.rollups[0];
  fixture.rollups.unshift({
    statusCheckRollup: completed.statusCheckRollup.map((entry, index) =>
      index === 1 ? { ...entry, status: "IN_PROGRESS", conclusion: null } : entry,
    ),
  });
  const result = runWithFakeGh(fixture);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(rollupReads(result.calls).length, 2);
});

test("renamed jobs need an unambiguous completed successful replacement", fakeGhOptions, () => {
  for (const changed of [
    { workflow_id: 8 },
    { head_sha: "b".repeat(40) },
    { created_at: EARLIER_START },
    { status: "in_progress", conclusion: null },
    { conclusion: "failure" },
    { conclusion: "cancelled" },
    { conclusion: "skipped" },
  ]) {
    const fixture = matrixReplacement();
    Object.assign(fixture.runs[`${RUN_ENDPOINT}/${NEWER_RUN}`], changed);
    const result = runWithFakeGh(fixture);
    assert.equal(result.status, 1, result.stdout + result.stderr);
  }
  for (const scenario of ["skipped", "unknown", "tie", "unreadable"]) {
    const fixture = matrixReplacement();
    const entries = fixture.rollups[0].statusCheckRollup;
    if (scenario === "skipped") {
      entries[1].conclusion = "SKIPPED";
      entries[2].conclusion = "SKIPPED";
    } else if (scenario === "unknown") {
      entries[0].detailsUrl = "https://example.invalid/check";
    } else if (scenario === "tie") {
      fixture.runs[`${RUN_ENDPOINT}/300`] = {
        ...fixture.runs[`${RUN_ENDPOINT}/${NEWER_RUN}`],
        html_url: `${RUN_URL}/300`,
      };
      entries[2].detailsUrl = `${RUN_URL}/300/job/2`;
    } else {
      fixture.runs[`${RUN_ENDPOINT}/${NEWER_RUN}`] = { error: "HTTP 502" };
    }
    const result = runWithFakeGh(fixture);
    assert.equal(result.status, scenario === "unreadable" ? 2 : 1, result.stdout + result.stderr);
  }
});

test("retired matrix placeholders cannot satisfy the minimum-check guard", fakeGhOptions, () => {
  const fixture = matrixReplacement();
  fixture.rollups.push({
    statusCheckRollup: [
      ...fixture.rollups[0].statusCheckRollup,
      checkRun("Lint", "COMPLETED", "SUCCESS"),
    ],
  });
  const result = runWithFakeGh(fixture, ["--min-checks", "3"]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(rollupReads(result.calls).length, 2);
});

test(
  "the CLI uses the newest same-workflow attempt in all-check and required modes",
  fakeGhOptions,
  () => {
    for (const args of [[], ["--required"]]) {
      for (const [older, newer] of [
        ["CANCELLED", "SUCCESS"],
        ["SUCCESS", "CANCELLED"],
      ]) {
        const result = runWithFakeGh(
          {
            selected: [check("Commit messages", "pass")],
            rollups: [
              { statusCheckRollup: [attempt(newer, NEWER_START), attempt(older, EARLIER_START)] },
            ],
          },
          args,
        );

        assert.equal(result.status, newer === "SUCCESS" ? 0 : 1, result.stdout + result.stderr);
      }
    }
  },
);

test(
  "the CLI retains failures across workflows and ambiguous duplicate attempts",
  fakeGhOptions,
  () => {
    for (const entries of [
      [attempt("CANCELLED", EARLIER_START, "Other workflow"), attempt("SUCCESS", NEWER_START)],
      [attempt("CANCELLED", NEWER_START), attempt("SUCCESS", NEWER_START)],
      [
        { ...attempt("CANCELLED", EARLIER_START), detailsUrl: "https://example.invalid/check" },
        attempt("SUCCESS", NEWER_START),
      ],
      [attempt("CANCELLED", EARLIER_START, null), attempt("SUCCESS", NEWER_START, null)],
      [attempt("CANCELLED", EARLIER_START, ""), attempt("SUCCESS", NEWER_START, "")],
    ]) {
      const result = runWithFakeGh({
        selected: [check("Commit messages", "pass")],
        rollups: [{ statusCheckRollup: entries }],
      });

      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
  },
);

test(
  "job starts and numeric run identifiers cannot reverse creation chronology",
  fakeGhOptions,
  () => {
    for (const args of [[], ["--required"]]) {
      const result = runWithFakeGh(
        {
          selected: [check("Commit messages", "pass")],
          rollups: [
            {
              statusCheckRollup: [
                attempt("SUCCESS", NEWER_START, "Commitlint", EARLIER_RUN),
                attempt("CANCELLED", EARLIER_START, "Commitlint", NEWER_RUN),
              ],
            },
          ],
        },
        args,
      );

      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
  },
);

test(
  "matching display names cannot hide a different workflow or commit failure",
  fakeGhOptions,
  () => {
    for (const changed of [
      { workflow_id: 8 },
      { head_sha: "b".repeat(40) },
      { created_at: EARLIER_START },
    ]) {
      const runs = workflowRuns();
      Object.assign(runs[`${RUN_ENDPOINT}/${NEWER_RUN}`], changed);
      const result = runWithFakeGh({
        selected: [check("Commit messages", "pass")],
        rollups: [
          {
            statusCheckRollup: [
              attempt("CANCELLED", EARLIER_START),
              attempt("SUCCESS", NEWER_START),
            ],
          },
        ],
        runs,
      });

      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
  },
);

test(
  "workflow metadata read failures cannot pass and optional metadata is not required",
  fakeGhOptions,
  () => {
    const runs = workflowRuns();
    runs[`${RUN_ENDPOINT}/${EARLIER_RUN}`] = { error: "HTTP 502" };
    const entries = [attempt("SUCCESS", EARLIER_START), attempt("SUCCESS", NEWER_START)];
    const failed = runWithFakeGh({
      selected: [check("Commit messages", "pass")],
      rollups: [{ statusCheckRollup: entries }],
      runs,
    });

    assert.equal(failed.status, 2, failed.stdout + failed.stderr);
    assert.match(failed.stderr, /HTTP 502/u);

    const optional = runWithFakeGh(
      {
        selected: [check("Lint", "pass")],
        rollups: [{ statusCheckRollup: [...entries, checkRun("Lint", "COMPLETED", "SUCCESS")] }],
        runs,
      },
      ["--required"],
    );
    assert.equal(optional.status, 0, optional.stdout + optional.stderr);
    assert.ok(optional.calls.every((args) => args[0] !== "api"));
  },
);

test("superseded attempts do not inflate the minimum-check guard", fakeGhOptions, () => {
  const attempts = [attempt("SUCCESS", EARLIER_START), attempt("SUCCESS", NEWER_START)];
  const result = runWithFakeGh(
    {
      selected: [check("Commit messages", "pass")],
      rollups: [
        { statusCheckRollup: attempts },
        { statusCheckRollup: [...attempts, checkRun("Lint", "COMPLETED", "SUCCESS")] },
      ],
    },
    ["--min-checks", "2"],
  );

  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(rollupReads(result.calls).length, 2, "superseded attempts are not distinct checks");
});

test("ambiguous attempts cannot inflate the minimum-check guard", fakeGhOptions, () => {
  for (const sameRun of [false, true]) {
    const runs = workflowRuns();
    runs[`${RUN_ENDPOINT}/${NEWER_RUN}`].created_at = EARLIER_START;
    const attempts = [
      attempt("SUCCESS", EARLIER_START),
      attempt("SUCCESS", NEWER_START, "Commitlint", sameRun ? EARLIER_RUN : NEWER_RUN),
    ];
    const result = runWithFakeGh(
      {
        selected: [check("Commit messages", "pass")],
        rollups: [
          { statusCheckRollup: attempts },
          { statusCheckRollup: [...attempts, checkRun("Lint", "COMPLETED", "SUCCESS")] },
        ],
        runs,
      },
      ["--min-checks", "2"],
    );

    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(rollupReads(result.calls).length, 2, "ambiguous attempts are one logical check");
  }
});

test(
  "confirmed different workflows with matching names remain distinct checks",
  fakeGhOptions,
  () => {
    const runs = workflowRuns();
    runs[`${RUN_ENDPOINT}/${NEWER_RUN}`].workflow_id = 8;
    const known = [attempt("SUCCESS", EARLIER_START), attempt("SUCCESS", NEWER_START)];
    const unknown = {
      ...attempt("SUCCESS", NEWER_START),
      detailsUrl: "https://example.invalid/check",
    };
    for (const extra of [[], [unknown]]) {
      const result = runWithFakeGh(
        {
          selected: [check("Commit messages", "pass")],
          rollups: [{ statusCheckRollup: [...known, ...extra] }, { statusCheckRollup: known }],
          runs,
        },
        ["--min-checks", "2"],
      );

      assert.equal(result.status, 0, result.stdout + result.stderr);
      assert.equal(
        rollupReads(result.calls).length,
        1,
        "unknown metadata cannot collapse known distinct workflows",
      );
    }
  },
);

test(
  "unknown identities cannot inflate a mixed group or hide blocking states",
  fakeGhOptions,
  () => {
    const runs = workflowRuns();
    runs[`${RUN_ENDPOINT}/${NEWER_RUN}`].workflow_id = 8;
    const known = [attempt("SUCCESS", EARLIER_START), attempt("SUCCESS", NEWER_START)];
    const unknown = {
      ...attempt("SUCCESS", NEWER_START),
      detailsUrl: "https://example.invalid/check",
    };
    const count = runWithFakeGh(
      {
        selected: [check("Commit messages", "pass")],
        rollups: [
          { statusCheckRollup: [...known, unknown] },
          { statusCheckRollup: [...known, unknown, checkRun("Lint", "COMPLETED", "SUCCESS")] },
        ],
        runs,
      },
      ["--min-checks", "3"],
    );
    assert.equal(count.status, 0, count.stdout + count.stderr);
    assert.equal(rollupReads(count.calls).length, 2, "unknown identity proves no additional check");

    for (const conclusion of ["CANCELLED", "FAILURE"]) {
      const result = runWithFakeGh(
        {
          selected: [check("Commit messages", "pass")],
          rollups: [{ statusCheckRollup: [...known, { ...unknown, conclusion }] }],
          runs,
        },
        ["--min-checks", "2"],
      );
      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
    const pending = runWithFakeGh(
      {
        selected: [check("Commit messages", "pass")],
        rollups: [
          { statusCheckRollup: [{ ...unknown, status: "QUEUED", conclusion: null }, ...known] },
          { statusCheckRollup: [unknown, ...known] },
        ],
        runs,
      },
      ["--min-checks", "2"],
    );
    assert.equal(pending.status, 0, pending.stdout + pending.stderr);
    assert.equal(rollupReads(pending.calls).length, 2, "unknown pending identity remains blocking");
  },
);

test(
  "ambiguous attempts preserve later cancellations, failures and pending states",
  fakeGhOptions,
  () => {
    const older = attempt("SUCCESS", EARLIER_START);
    for (const conclusion of ["CANCELLED", "FAILURE"]) {
      const result = runWithFakeGh({
        selected: [check("Commit messages", "pass")],
        rollups: [
          {
            statusCheckRollup: [older, attempt(conclusion, NEWER_START, "Commitlint", EARLIER_RUN)],
          },
        ],
      });
      assert.equal(result.status, 1, result.stdout + result.stderr);
    }
    const pending = { ...attempt(null, null, "Commitlint", EARLIER_RUN), status: "QUEUED" };
    const result = runWithFakeGh({
      selected: [check("Commit messages", "pass")],
      rollups: [
        { statusCheckRollup: [older, pending] },
        { statusCheckRollup: [older, attempt("SUCCESS", NEWER_START, "Commitlint", EARLIER_RUN)] },
      ],
    });
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(
      rollupReads(result.calls).length,
      2,
      "ambiguous pending attempt must stay visible",
    );
  },
);

test("queued attempts without a real start cannot be hidden by older passes", fakeGhOptions, () => {
  for (const startedAt of [null, "0001-01-01T00:00:00Z"]) {
    const older = attempt("SUCCESS", EARLIER_START);
    const result = runWithFakeGh({
      selected: [check("Commit messages", "pass")],
      rollups: [
        { statusCheckRollup: [older, { ...attempt(null, startedAt), status: "QUEUED" }] },
        { statusCheckRollup: [older, attempt("SUCCESS", NEWER_START)] },
      ],
    });

    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(
      rollupReads(result.calls).length,
      2,
      "unknown start must retain the pending attempt",
    );
    assert.equal(
      result.calls.filter((args) => args[0] === "api").length,
      2,
      "immutable creation metadata is cached between polls",
    );
  }
});

test(
  "the CLI rejects a newer same-head cancellation or failure hidden by an older pass",
  fakeGhOptions,
  () => {
    for (const conclusion of ["CANCELLED", "FAILURE"]) {
      const result = runWithFakeGh({
        selected: [check("Commit messages", "pass")],
        rollups: [{ statusCheckRollup: [checkRun("Commit messages", "COMPLETED", conclusion)] }],
      });

      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stdout, /checks failed/u);
      assert.match(result.stdout, /Commit messages https:\/\/example.invalid\/current/u);
    }
  },
);

test(
  "the CLI waits for a newer same-head pending check despite an older pass",
  fakeGhOptions,
  () => {
    const result = runWithFakeGh({
      selected: [check("Commit messages", "pass")],
      rollups: [
        { statusCheckRollup: [checkRun("Commit messages", "IN_PROGRESS", null)] },
        { statusCheckRollup: [checkRun("Commit messages", "COMPLETED", "SUCCESS")] },
      ],
    });

    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(
      rollupReads(result.calls).length,
      2,
      "pending rollup must be read again before passing",
    );
  },
);

test(
  "required-only checks ignore optional cancellations and use current required states",
  fakeGhOptions,
  () => {
    for (const conclusion of ["SUCCESS", "CANCELLED"]) {
      const result = runWithFakeGh(
        {
          selected: [check("Commit messages", "pass")],
          rollups: [
            {
              statusCheckRollup: [
                checkRun("Commit messages", "COMPLETED", conclusion),
                checkRun("Optional benchmark", "COMPLETED", "CANCELLED"),
              ],
            },
          ],
        },
        ["--required"],
      );

      assert.equal(result.status, conclusion === "SUCCESS" ? 0 : 1, result.stdout + result.stderr);
      assert.doesNotMatch(result.stdout, /Optional benchmark/u);
      assert.ok(result.calls.some((args) => args.includes("--required")));
    }
  },
);

test("required pending and missing selected checks cannot pass by omission", fakeGhOptions, () => {
  for (const requiredCheck of [[], [checkRun("Commit messages", "QUEUED", null)]]) {
    const result = runWithFakeGh(
      {
        selected: [check("CI", "pass"), check("Commit messages", "pass")],
        rollups: [
          { statusCheckRollup: [checkRun("CI", "COMPLETED", "SUCCESS"), ...requiredCheck] },
          {
            statusCheckRollup: [
              checkRun("CI", "COMPLETED", "SUCCESS"),
              checkRun("Commit messages", "COMPLETED", "SUCCESS"),
            ],
          },
        ],
      },
      ["--required"],
    );

    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(
      rollupReads(result.calls).length,
      2,
      "every selected required check must complete",
    );
  }
});

test(
  "--required falls back to all checks when the repository has no required checks",
  fakeGhOptions,
  () => {
    const result = runWithFakeGh(
      {
        selected: "no required checks reported on the 'chore/example' branch",
        rollups: [
          {
            statusCheckRollup: [
              checkRun("CI", "COMPLETED", "SUCCESS"),
              checkRun("Commit messages", "IN_PROGRESS", null),
            ],
          },
          {
            statusCheckRollup: [
              checkRun("CI", "COMPLETED", "SUCCESS"),
              checkRun("Commit messages", "COMPLETED", "SUCCESS"),
            ],
          },
        ],
      },
      ["--required", "--min-checks", "2"],
    );

    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(result.stdout, /no required checks[^\n]*all checks/iu);
    assert.equal(
      result.stdout.match(/no required checks/giu)?.length,
      1,
      "the fallback is printed once",
    );
    assert.match(result.stdout, /all checks passed/u);
    assert.equal(rollupReads(result.calls).length, 2, "the wait covers the pending check");
  },
);

test(
  "the CLI fails closed when the current-head rollup has invalid JSON or shape",
  fakeGhOptions,
  () => {
    for (const reply of ["{", { statusCheckRollup: {} }]) {
      const result = runWithFakeGh({ selected: [check("CI", "pass")], rollups: [reply] });

      assert.equal(result.status, 2, result.stdout + result.stderr);
      assert.match(result.stdout, /failed reads of the checks/u);
    }
  },
);

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

test("--head waits until the pull request reports the pushed commit", fakeGhOptions, () => {
  const oldHead = "b".repeat(40);
  const newHead = "c".repeat(40);
  const fixture = {
    selected: [],
    rollups: [
      { headRefOid: oldHead, statusCheckRollup: [checkRun("CI", "COMPLETED", "FAILURE")] },
      { headRefOid: newHead, statusCheckRollup: [checkRun("CI", "COMPLETED", "SUCCESS")] },
    ],
  };
  const stale = runWithFakeGh(fixture);
  assert.equal(stale.status, 1, "without --head, the rollup of the old head decides");
  const result = runWithFakeGh(fixture, ["--head", newHead]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(rollupReads(result.calls).length, 2);
});
