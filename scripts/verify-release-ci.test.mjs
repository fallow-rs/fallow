import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";

import {
  REQUIRED_WORKFLOWS,
  classifyRun,
  errorLines,
  evaluateRuns,
  parseArguments,
  verifyReleaseCi,
} from "./verify-release-ci.mjs";

const SHA = "a".repeat(40);

const REQUIRED = [
  { name: "CI", file: "ci.yml" },
  { name: "Coverage", file: "coverage.yml" },
];

let nextId = 100;

const run = (
  file,
  { status = "completed", conclusion = "success", event = "push", id, attempt = 1 } = {},
) => ({
  id: id ?? (nextId += 1),
  name: file.replace(/\.yml$/u, ""),
  path: `.github/workflows/${file}`,
  event,
  status,
  conclusion: status === "completed" ? conclusion : null,
  run_attempt: attempt,
  html_url: `https://github.com/o/r/actions/runs/${id ?? nextId}`,
});

const green = () => [run("ci.yml"), run("coverage.yml")];

test("passes when every required workflow succeeded", () => {
  const result = evaluateRuns(green(), { required: REQUIRED });
  assert.equal(result.state, "pass");
  assert.equal(result.passed.length, 2);
});

test("fails when a required workflow failed", () => {
  const runs = [run("ci.yml", { conclusion: "failure" }), run("coverage.yml")];
  const result = evaluateRuns(runs, { required: REQUIRED });
  assert.equal(result.state, "fail");
  assert.deepEqual(
    result.failed.map((entry) => entry.file),
    ["ci.yml"],
  );
  assert.match(
    errorLines(result, SHA)[0],
    /^::error title=Failed CI run::.*gh run rerun \d+ --failed/u,
  );
});

test("a required run that was skipped does not count as a pass", () => {
  const runs = [run("ci.yml", { conclusion: "skipped" }), run("coverage.yml")];
  assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "fail");
});

test("fails on a missing required run and prints the dispatch command", () => {
  const result = evaluateRuns([run("ci.yml")], { required: REQUIRED });
  assert.equal(result.state, "fail");
  assert.deepEqual(result.missing, [REQUIRED[1]]);
  assert.match(errorLines(result, SHA)[0], /gh workflow run coverage\.yml --ref main/u);
});

test("a missing run without workflow_dispatch gets a push hint, not a dispatch command", () => {
  const required = [{ name: "CI", file: "ci.yml", dispatchable: false }];
  const [line] = errorLines(evaluateRuns([], { required }), SHA);
  assert.doesNotMatch(line, /gh workflow run/u);
  assert.match(line, /no workflow_dispatch trigger\. Push a new commit to main/u);
});

test("waits for a missing run during the grace period", () => {
  assert.equal(
    evaluateRuns([run("ci.yml")], { required: REQUIRED, allowMissing: true }).state,
    "wait",
  );
});

test("waits while a required run is queued or in progress", () => {
  for (const status of ["queued", "in_progress", "waiting"]) {
    const runs = [run("ci.yml", { status }), run("coverage.yml")];
    assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "wait", status);
  }
});

test("a successful re-run attempt counts", () => {
  // The API reports the latest attempt of a run under the same run id.
  const runs = [run("ci.yml", { attempt: 2, conclusion: "success" }), run("coverage.yml")];
  assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "pass");
});

test("a newer separate run replaces an older failed run", () => {
  const runs = [
    run("ci.yml", { id: 1, conclusion: "failure" }),
    run("ci.yml", { id: 2, event: "workflow_dispatch" }),
    run("coverage.yml"),
  ];
  assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "pass");
});

test("fails when a non-required push run failed", () => {
  const runs = [...green(), run("review-electron.yml", { conclusion: "failure" })];
  const result = evaluateRuns(runs, { required: REQUIRED });
  assert.equal(result.state, "fail");
  assert.equal(result.failed[0].required, false);
});

test("a non-required run may be skipped or neutral", () => {
  const runs = [
    ...green(),
    run("review-electron.yml", { conclusion: "skipped" }),
    run("hawk.yml", { conclusion: "neutral" }),
  ];
  assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "pass");
});

test("ignores pull request runs and the release workflows", () => {
  const runs = [
    ...green(),
    run("bench.yml", { event: "pull_request", conclusion: "failure" }),
    run("release.yml", { event: "workflow_dispatch", conclusion: "failure" }),
    run("release-published.yml", { event: "release", conclusion: "failure" }),
  ];
  assert.equal(evaluateRuns(runs, { required: REQUIRED }).state, "pass");
});

test("classifies cancelled and timed out runs as failures", () => {
  for (const conclusion of ["cancelled", "timed_out", "action_required", "startup_failure"]) {
    assert.equal(classifyRun(run("x.yml", { conclusion }), false), "fail", conclusion);
  }
});

test("the required list names CI and unique workflow files", () => {
  const files = REQUIRED_WORKFLOWS.map((workflow) => workflow.file);
  assert.ok(files.includes("ci.yml"));
  assert.equal(new Set(files).size, files.length);
});

test("parseArguments rejects a short SHA and reads the timeout", () => {
  assert.throws(() => parseArguments(["--sha", "abc", "--repo", "o/r"]), /40-character/u);
  const options = parseArguments(["--sha", SHA, "--repo", "o/r", "--timeout-minutes", "1"]);
  assert.equal(options.timeoutMinutes, 1);
  assert.equal(options.pollSeconds, 60);
});

test("verifyReleaseCi polls until pending runs finish", async (t) => {
  const responses = [
    [run("ci.yml", { id: 1, status: "in_progress" }), run("coverage.yml", { id: 2 })],
    [run("ci.yml", { id: 1 }), run("coverage.yml", { id: 2 })],
  ];
  t.mock.method(globalThis, "fetch", async () => {
    const workflowRuns = responses.shift();
    return {
      ok: true,
      json: async () => ({ total_count: workflowRuns.length, workflow_runs: workflowRuns }),
    };
  });
  let clock = 0;
  const waits = [];
  const code = await verifyReleaseCi(
    { sha: SHA, repo: "o/r", timeoutMinutes: 10, pollSeconds: 60, missingGraceMinutes: 0 },
    {
      token: "t",
      now: () => clock,
      wait: async (ms) => {
        waits.push(ms);
        clock += ms;
      },
      log: () => {},
    },
  );
  // Missing workflows from the full REQUIRED list fail the gate after the wait.
  assert.equal(code, 1);
  assert.deepEqual(waits, [60_000]);
});

test("verifyReleaseCi times out while a run stays queued", async (t) => {
  t.mock.method(globalThis, "fetch", async () => ({
    ok: true,
    json: async () => ({
      total_count: 1,
      workflow_runs: [run("ci.yml", { id: 1, status: "queued" })],
    }),
  }));
  let clock = 0;
  const lines = [];
  const code = await verifyReleaseCi(
    { sha: SHA, repo: "o/r", timeoutMinutes: 3, pollSeconds: 60, missingGraceMinutes: 0 },
    {
      token: "t",
      now: () => clock,
      wait: async (ms) => {
        clock += ms;
      },
      log: (line) => lines.push(line),
    },
  );
  assert.equal(code, 1);
  assert.ok(lines.some((line) => line.startsWith("::error::Timed out after 3 min")));
});

// Policy checks on the real workflow files. The release gate is the only
// barrier between a red release commit and a published release, so these
// tests fail when an edit weakens it.

const WORKFLOWS = new URL("../.github/workflows/", import.meta.url);
const readWorkflowFile = (file) => readFileSync(new URL(file, WORKFLOWS), "utf8");

/** Returns the top-level `on:` block of a workflow. */
const triggerBlock = (source) => {
  const match = /^on:\n((?:[ #].*\n|\n)*)/mu.exec(source);
  return match?.[1] ?? "";
};

/** Returns the text of each job, keyed by job id. */
const jobBlocks = (source) => {
  const jobs = new Map();
  const body = source.slice(source.indexOf("\njobs:\n") + "\njobs:\n".length);
  const parts = body.split(/^ {2}(?=[\w-]+:\s*$)/mu).filter((part) => part.trim() !== "");
  for (const part of parts) jobs.set(part.slice(0, part.indexOf(":")), part);
  return jobs;
};

const needsOf = (job) => {
  const match = /^ {4}needs:\s*(.+)$/mu.exec(job);
  if (match === null) return [];
  return match[1]
    .replace(/[[\]]/gu, "")
    .split(",")
    .map((name) => name.trim())
    .filter((name) => name !== "");
};

test("each required workflow exists, runs on push to main, and has the right dispatch flag", () => {
  for (const workflow of REQUIRED_WORKFLOWS) {
    assert.ok(existsSync(new URL(workflow.file, WORKFLOWS)), `${workflow.file} does not exist`);
    const triggers = triggerBlock(readWorkflowFile(workflow.file));
    const push = /^ {2}push:\n((?: {4}.*\n)*)/mu.exec(triggers);
    assert.ok(push, `${workflow.file} has no push trigger`);
    assert.match(push[1], /branches:.*\bmain\b/u, `${workflow.file} push does not target main`);
    const dispatchable = /^ {2}workflow_dispatch:/mu.test(triggers);
    assert.equal(
      workflow.dispatchable !== false,
      dispatchable,
      `${workflow.file}: dispatchable flag does not match its workflow_dispatch trigger`,
    );
  }
});

test("release.yml runs the CI gate before every other job", () => {
  const jobs = jobBlocks(readWorkflowFile("release.yml"));
  const context = jobs.get("release-context");
  assert.ok(context, "release.yml has no release-context job");

  const gate =
    /node scripts\/verify-release-ci\.mjs --sha "\$GITHUB_SHA" --timeout-minutes (\d+)/u.exec(
      context,
    );
  assert.ok(gate, "release-context does not run the CI gate on $GITHUB_SHA");
  assert.match(context, /^ {6}actions: read$/mu, "release-context lacks actions: read");
  assert.match(context, /GITHUB_TOKEN: \$\{\{ secrets\.GITHUB_TOKEN \}\}/u);
  assert.doesNotMatch(context, /continue-on-error/u, "the CI gate must not be optional");

  const jobTimeout = /^ {4}timeout-minutes: (\d+)$/mu.exec(context);
  assert.ok(jobTimeout, "release-context has no job timeout");
  assert.ok(
    Number(jobTimeout[1]) > Number(gate[1]),
    "the release-context timeout must be longer than the gate timeout",
  );

  // A job is safe when it is release-context, or when it needs at least one
  // job and every needed job is safe. The path set stops a cycle.
  const reachesContext = (name, path = new Set()) => {
    if (name === "release-context") return true;
    if (path.has(name)) return false;
    const job = jobs.get(name);
    assert.ok(job, `release.yml needs an unknown job: ${name}`);
    const needs = needsOf(job);
    const next = new Set(path).add(name);
    return needs.length > 0 && needs.every((need) => reachesContext(need, next));
  };
  for (const name of jobs.keys()) {
    assert.ok(reachesContext(name), `job ${name} can start before release-context passes`);
  }
});
