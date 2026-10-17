import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  REQUIRED_WORKFLOWS,
  classifyRun,
  errorLines,
  evaluateRuns,
  parseArguments,
  parseRemoteRepo,
  resolveDefaults,
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

// Local defaults: a maintainer runs the gate from a checkout with gh logged in.

test("parseRemoteRepo reads owner/name from GitHub remote URLs", () => {
  for (const url of [
    "git@github.com:fallow-rs/fallow.git",
    "https://github.com/fallow-rs/fallow.git",
    "https://github.com/fallow-rs/fallow",
    "ssh://git@github.com/fallow-rs/fallow.git",
  ]) {
    assert.equal(parseRemoteRepo(url), "fallow-rs/fallow", url);
  }
  assert.equal(parseRemoteRepo("https://gitlab.com/fallow-rs/fallow.git"), null);
  assert.equal(parseRemoteRepo(""), null);
});

const fakeRun = (replies) => {
  const calls = [];
  const exec = (command, args) => {
    calls.push([command, ...args].join(" "));
    return replies[[command, ...args].join(" ")] ?? { status: 1, stdout: "" };
  };
  return { exec, calls };
};

test("resolveDefaults reads the gh token and the origin remote when run locally", () => {
  const { exec, calls } = fakeRun({
    "gh auth token": { status: 0, stdout: "gho_local\n" },
    "git remote get-url origin": { status: 0, stdout: "git@github.com:fallow-rs/fallow.git\n" },
  });
  assert.deepEqual(resolveDefaults({ env: {}, run: exec }), {
    token: "gho_local",
    repo: "fallow-rs/fallow",
  });
  assert.deepEqual(calls, ["gh auth token", "git remote get-url origin"]);
});

test("resolveDefaults prefers the environment over gh and git", () => {
  const { exec, calls } = fakeRun({});
  assert.deepEqual(
    resolveDefaults({ env: { GH_TOKEN: "env", GITHUB_REPOSITORY: "o/r" }, run: exec }),
    {
      token: "env",
      repo: "o/r",
    },
  );
  assert.deepEqual(calls, []);
});

test("resolveDefaults in GitHub Actions never calls gh or git", () => {
  const { exec, calls } = fakeRun({
    "gh auth token": { status: 0, stdout: "gho_local\n" },
    "git remote get-url origin": { status: 0, stdout: "git@github.com:o/r.git\n" },
  });
  assert.deepEqual(resolveDefaults({ env: { GITHUB_ACTIONS: "true" }, run: exec }), {
    token: undefined,
    repo: undefined,
  });
  assert.deepEqual(calls, []);
});

test("parseArguments reads --dispatch-missing and uses the default repo", () => {
  const options = parseArguments(["--sha", SHA, "--dispatch-missing"], { repo: "o/r" });
  assert.equal(options.dispatchMissing, true);
  assert.equal(options.repo, "o/r");
  assert.equal(parseArguments(["--sha", SHA], { repo: "o/r" }).dispatchMissing, false);
  assert.equal(parseArguments(["--sha", SHA, "--repo", "x/y"], { repo: "o/r" }).repo, "x/y");
});

/** Runs the gate against a scripted list of run lists, one per poll. */
const runGate = async (t, responses, { dispatchMissing = true, mainSha = SHA, required } = {}) => {
  t.mock.method(globalThis, "fetch", async () => {
    const workflowRuns = responses.length > 1 ? responses.shift() : responses[0];
    return {
      ok: true,
      json: async () => ({ total_count: workflowRuns.length, workflow_runs: workflowRuns }),
    };
  });
  let clock = 0;
  const dispatched = [];
  const lines = [];
  const code = await verifyReleaseCi(
    {
      sha: SHA,
      repo: "o/r",
      timeoutMinutes: 30,
      pollSeconds: 60,
      missingGraceMinutes: 2,
      dispatchMissing,
    },
    {
      token: "t",
      required,
      now: () => clock,
      wait: async (ms) => {
        clock += ms;
      },
      log: (line) => lines.push(line),
      readMainSha: async () => mainSha,
      dispatch: (workflow) => {
        dispatched.push(workflow.file);
        return { ok: true };
      },
    },
  );
  return { code, dispatched, lines };
};

test("--dispatch-missing starts each missing workflow once and then waits for it", async (t) => {
  const required = [
    { name: "CI", file: "ci.yml", dispatchable: false },
    { name: "Benchmarks", file: "bench.yml" },
  ];
  const ci = run("ci.yml", { id: 1 });
  const { code, dispatched, lines } = await runGate(
    t,
    [[ci], [ci], [ci], [ci], [ci, run("bench.yml", { id: 2, event: "workflow_dispatch" })]],
    { required },
  );
  assert.equal(code, 0, lines.join("\n"));
  assert.deepEqual(dispatched, ["bench.yml"]);
  assert.ok(lines.some((line) => line.includes("gh workflow run bench.yml --ref main")));
});

test("--dispatch-missing refuses to start runs when main is not the release commit", async (t) => {
  const required = [{ name: "Benchmarks", file: "bench.yml" }];
  const { code, dispatched, lines } = await runGate(t, [[]], {
    required,
    mainSha: "b".repeat(40),
  });
  assert.equal(code, 1);
  assert.deepEqual(dispatched, []);
  assert.ok(
    lines.some((line) => /main is at b{12}/u.test(line)),
    lines.join("\n"),
  );
});

test("--dispatch-missing does not start runs when a missing workflow cannot be dispatched", async (t) => {
  const required = [
    { name: "CI", file: "ci.yml", dispatchable: false },
    { name: "Benchmarks", file: "bench.yml" },
  ];
  const { code, dispatched, lines } = await runGate(t, [[]], { required });
  assert.equal(code, 1);
  assert.deepEqual(dispatched, []);
  assert.ok(lines.some((line) => line.includes("Missing CI run::CI (ci.yml)")));
});

test("--dispatch-missing starts nothing while another run failed", async (t) => {
  const required = [
    { name: "CI", file: "ci.yml" },
    { name: "Benchmarks", file: "bench.yml" },
  ];
  const { code, dispatched } = await runGate(t, [[run("ci.yml", { conclusion: "failure" })]], {
    required,
  });
  assert.equal(code, 1);
  assert.deepEqual(dispatched, []);
});

test("without --dispatch-missing a missing run still fails the gate", async (t) => {
  const required = [{ name: "Benchmarks", file: "bench.yml" }];
  const { code, dispatched } = await runGate(t, [[]], { required, dispatchMissing: false });
  assert.equal(code, 1);
  assert.deepEqual(dispatched, []);
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

const assertReleaseJobsGated = (jobs) => {
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
  for (const [name, job] of jobs) {
    if (name === "publisher-auth-check") {
      assert.match(job, /^ {4}if: inputs.auth_check && github.ref == 'refs\/heads\/main'$/mu);
      assert.match(job, /^ {4}environment: release$/mu);
      assert.deepEqual(needsOf(job), []);
      const permissions = /^ {4}permissions:\n((?: {6}.*\n)*)/mu.exec(job)?.[1];
      assert.deepEqual(
        permissions
          ?.trim()
          .split("\n")
          .map((line) => line.trim()),
        ["contents: read", "id-token: write"],
      );
      const commands = [...job.matchAll(/^ +run: (.+)$/gmu)].map((match) => match[1]);
      assert.deepEqual(commands, ["node .github/scripts/publisher-auth-check.mjs"]);
      const actions = [...job.matchAll(/^ +(?:- )?uses: (\S+)/gmu)].map((match) => match[1]);
      assert.equal(actions.length, 2);
      assert.match(actions[0], /^actions\/checkout@[a-f0-9]{40}$/u);
      assert.match(actions[1], /^actions\/setup-node@[a-f0-9]{40}$/u);
      assert.match(job, /ref: \$\{\{ github.sha \}\}/u);
      assert.match(job, /persist-credentials: false/u);
      assert.match(job, /sparse-checkout: .github\/scripts\/publisher-auth-check.mjs/u);
      assert.doesNotMatch(
        job,
        /secrets\.|\b(?:npm|npx|cargo|publish|stage|upload-artifact|download-artifact)\b/u,
      );
      continue;
    }
    assert.match(job, /^ {4}if: \$\{\{ !inputs.auth_check \}\}$/mu, name);
    assert.ok(reachesContext(name), `job ${name} can start before release-context passes`);
  }
};

test("release.yml runs the CI gate before every publication job", () => {
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

  assertReleaseJobsGated(jobs);
});

test("release CI gate exception rejects unsafe diagnostic and normal jobs", () => {
  const jobs = jobBlocks(readWorkflowFile("release.yml"));
  const auth = jobs.get("publisher-auth-check");
  assert.ok(auth);
  for (const [before, after] of [
    ["if: inputs.auth_check && github.ref == 'refs/heads/main'", "if: true"],
    ["environment: release", "environment: other"],
    ["contents: read", "contents: write"],
    ["node .github/scripts/publisher-auth-check.mjs", "npm publish"],
    [
      "sparse-checkout: .github/scripts/publisher-auth-check.mjs",
      "sparse-checkout: .github/scripts/other.mjs",
    ],
  ]) {
    assert.ok(auth.includes(before));
    const changed = new Map(jobs).set("publisher-auth-check", auth.replace(before, after));
    assert.throws(() => assertReleaseJobsGated(changed));
  }
  const publish = jobs.get("npm-publish");
  assert.ok(publish);
  for (const changed of [
    publish.replace("if: ${{ !inputs.auth_check }}", "if: true"),
    publish.replace(/^ {4}needs:.*\n/mu, ""),
  ])
    assert.throws(() => assertReleaseJobsGated(new Map(jobs).set("npm-publish", changed)));
  assert.throws(() =>
    assertReleaseJobsGated(
      new Map(jobs).set("other-root", "other-root:\n    if: ${{ !inputs.auth_check }}\n"),
    ),
  );
  assertReleaseJobsGated(jobs);
});

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const runScriptWithoutToken = (extraEnv, path = process.env.PATH) => {
  const env = { ...process.env, ...extraEnv, PATH: path };
  delete env.GITHUB_TOKEN;
  delete env.GH_TOKEN;
  if (extraEnv.GITHUB_ACTIONS === undefined) delete env.GITHUB_ACTIONS;
  // The token check comes first, so this run cannot reach the GitHub API.
  return spawnSync(
    process.execPath,
    ["scripts/verify-release-ci.mjs", "--sha", SHA, "--timeout-minutes", "150"],
    { cwd: REPO_ROOT, encoding: "utf8", env },
  );
};

test("node scripts/verify-release-ci.mjs in GitHub Actions rejects a run without a token", () => {
  const result = runScriptWithoutToken({ GITHUB_ACTIONS: "true" });
  assert.equal(result.status, 1, result.stderr);
  assert.equal(result.stdout, "::error::set GITHUB_TOKEN or GH_TOKEN\n");
});

test(
  "node scripts/verify-release-ci.mjs locally asks for gh auth login when gh has no token",
  { skip: process.platform === "win32" },
  () => {
    const bin = mkdtempSync(join(tmpdir(), "verify-release-ci-"));
    try {
      writeFileSync(join(bin, "gh"), "#!/bin/sh\nexit 1\n", { mode: 0o755 });
      const result = runScriptWithoutToken({}, `${bin}${delimiter}${process.env.PATH}`);
      assert.equal(result.status, 1, result.stderr);
      assert.equal(
        result.stdout,
        "::error::set GITHUB_TOKEN or GH_TOKEN, or log in with gh auth login\n",
      );
    } finally {
      rmSync(bin, { recursive: true, force: true });
    }
  },
);
