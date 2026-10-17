// Release gate: a release must not start unless every check passed on the
// release commit. Pull requests run only part of CI; the rest runs on push to
// main. This script reads the workflow runs for the release SHA and blocks the
// release until each required workflow has a successful run and no other run
// on that SHA has failed.
//
// Usage: node scripts/verify-release-ci.mjs --sha <sha> [--timeout-minutes 150]
//        [--poll-seconds 60] [--missing-grace-minutes 5] [--repo owner/name]
//        [--dispatch-missing]
// Token: GITHUB_TOKEN or GH_TOKEN (needs `actions: read`). Outside GitHub
// Actions, the token defaults to `gh auth token` and the repository to the
// `origin` remote.
//
// --dispatch-missing: after the grace period, start each required workflow
// that has no run for the SHA with `gh workflow run <file> --ref main`, once,
// and wait for it. The script starts runs only when main on GitHub is the SHA.

import { spawnSync } from "node:child_process";
import { appendFileSync, realpathSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

/**
 * Workflows that must have a successful run on the release SHA. Most of them
 * start on push to main. Some have path filters that a release commit does not
 * always match, so they can have no push run. Start those with
 * `--dispatch-missing`, or with `gh workflow run <file> --ref main`.
 *
 * `dispatchable: false` marks a workflow without a `workflow_dispatch`
 * trigger. `gh workflow run` cannot start it, so the fix hint differs.
 *
 * UPDATE THIS LIST when you add a workflow that runs on push to main, move a
 * check from pull requests to main, or change a path filter so that a release
 * commit no longer starts the workflow.
 */
export const REQUIRED_WORKFLOWS = Object.freeze([
  { name: "CI", file: "ci.yml", dispatchable: false },
  { name: "Commitlint", file: "commitlint.yml", dispatchable: false },
  { name: "Coverage", file: "coverage.yml" },
  { name: "Ecosystem CI", file: "ecosystem.yml" },
  { name: "Cross-Architecture", file: "cross-arch.yml" },
  { name: "Fuzz Smoke", file: "fuzz-smoke.yml" },
  { name: "Benchmarks", file: "bench.yml" },
  { name: "Type-aware Benchmarks", file: "bench-type-aware.yml" },
  { name: "Protocol parity", file: "protocol-parity.yml" },
  { name: "Scorecard", file: "scorecard.yml" },
]);

const WORKFLOW_DIR = ".github/workflows/";

/**
 * Runs that do not test the release commit as it lands on main. Pull request
 * runs test a merge ref, and GitHub-managed dynamic runs (Dependabot updates)
 * are not repository checks.
 */
const IGNORED_EVENTS = new Set(["pull_request", "pull_request_target", "merge_group", "dynamic"]);

/** The release workflows themselves. An earlier failed release attempt must not block a retry. */
const IGNORED_FILES = new Set(["release.yml", "release-published.yml"]);

const PASSING_CONCLUSIONS = new Set(["success", "skipped", "neutral"]);

const DEFAULT_TIMEOUT_MINUTES = 150;
const DEFAULT_POLL_SECONDS = 60;
const DEFAULT_MISSING_GRACE_MINUTES = 5;
const MAX_CONSECUTIVE_API_ERRORS = 5;
const PER_PAGE = 100;
const FETCH_TIMEOUT_MS = 30_000;
const MAIN_BRANCH = "main";
const SHORT_SHA_LENGTH = 12;
const GITHUB_REMOTE_PATTERN =
  /^(?:git@github\.com:|ssh:\/\/git@github\.com\/|https:\/\/github\.com\/)([\w.-]+\/[\w.-]+?)(?:\.git)?\/?$/u;

/** Returns the workflow file name of a run, for example `ci.yml`. */
export const workflowFile = (run) => {
  const path = String(run.path ?? "").split("@")[0];
  return path.startsWith(WORKFLOW_DIR) ? path.slice(WORKFLOW_DIR.length) : path;
};

/**
 * Keeps the newest run per workflow file. A re-run keeps its run id, and the
 * API reports the state of its latest attempt, so re-runs count. A later
 * separate run (for example a manual dispatch) replaces an older one.
 */
export const latestRunPerWorkflow = (runs) => {
  const latest = new Map();
  for (const run of runs) {
    if (IGNORED_EVENTS.has(run.event)) continue;
    const file = workflowFile(run);
    if (IGNORED_FILES.has(file)) continue;
    const current = latest.get(file);
    if (current === undefined || run.id > current.id) latest.set(file, run);
  }
  return latest;
};

/** Classifies one run as `pending`, `pass` or `fail`. Required runs pass only on success. */
export const classifyRun = (run, required) => {
  if (run.status !== "completed") return "pending";
  if (required) return run.conclusion === "success" ? "pass" : "fail";
  return PASSING_CONCLUSIONS.has(run.conclusion) ? "pass" : "fail";
};

/**
 * Decides the gate state from the runs on the release SHA.
 * `allowMissing` is true while new push runs can still appear.
 * Returns `{ state: "pass" | "fail" | "wait", passed, pending, failed, missing }`.
 */
export const evaluateRuns = (
  runs,
  { required = REQUIRED_WORKFLOWS, allowMissing = false } = {},
) => {
  const latest = latestRunPerWorkflow(runs);
  const requiredFiles = new Set(required.map((workflow) => workflow.file));
  const result = { passed: [], pending: [], failed: [], missing: [] };

  for (const workflow of required) {
    if (!latest.has(workflow.file)) result.missing.push(workflow);
  }
  for (const [file, run] of latest) {
    const entry = { file, name: run.name, run, required: requiredFiles.has(file) };
    const state = classifyRun(run, entry.required);
    if (state === "pass") result.passed.push(entry);
    else if (state === "pending") result.pending.push(entry);
    else result.failed.push(entry);
  }

  let state = "pass";
  if (result.failed.length > 0) state = "fail";
  else if (result.pending.length > 0) state = "wait";
  else if (result.missing.length > 0) state = allowMissing ? "wait" : "fail";
  return { state, ...result };
};

/** Returns how to start a missing run. Only workflows with `workflow_dispatch` accept `gh workflow run`. */
const missingRunHint = (workflow) =>
  workflow.dispatchable === false
    ? "It has no workflow_dispatch trigger. Push a new commit to main and prepare the release from that commit."
    : `Start it with: gh workflow run ${workflow.file} --ref main, or run this gate locally with --dispatch-missing`;

/** Formats the `::error::` lines for a failed gate. */
export const errorLines = (evaluation, sha) => {
  const lines = [];
  for (const workflow of evaluation.missing) {
    lines.push(
      `::error title=Missing CI run::${workflow.name} (${workflow.file}) has no run for ${sha}. ` +
        missingRunHint(workflow),
    );
  }
  for (const entry of evaluation.failed) {
    const conclusion = entry.run.conclusion ?? entry.run.status;
    lines.push(
      `::error title=Failed CI run::${entry.name} (${entry.file}) ended with '${conclusion}' on ${sha}: ` +
        `${entry.run.html_url}. Fix main, or re-run it with: gh run rerun ${entry.run.id} --failed`,
    );
  }
  return lines;
};

const summaryTable = (evaluation, sha) => {
  const rows = [
    ...evaluation.failed.map((entry) => [
      entry.name,
      entry.run.conclusion ?? entry.run.status,
      "fail",
    ]),
    ...evaluation.missing.map((workflow) => [workflow.name, "no run", "missing"]),
    ...evaluation.pending.map((entry) => [entry.name, entry.run.status, "pending"]),
    ...evaluation.passed.map((entry) => [entry.name, entry.run.conclusion, "pass"]),
  ];
  return [
    `### Release CI gate for \`${sha}\`: ${evaluation.state}`,
    "",
    "| Workflow | Result | Gate |",
    "| --- | --- | --- |",
    ...rows.map((row) => `| ${row.join(" | ")} |`),
    "",
  ].join("\n");
};

/** Returns `owner/name` of a GitHub remote URL, or null for another host. */
export const parseRemoteRepo = (url) => GITHUB_REMOTE_PATTERN.exec(url.trim())?.[1] ?? null;

const runCommand = (command, args) =>
  spawnSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });

const commandOutput = (run, command, args) => {
  const result = run(command, args);
  return result.status === 0 ? result.stdout.trim() : "";
};

const localToken = (run) => commandOutput(run, "gh", ["auth", "token"]) || undefined;

const localRepo = (run) =>
  parseRemoteRepo(commandOutput(run, "git", ["remote", "get-url", "origin"])) ?? undefined;

const withLocalFallback = ({ token, repo }, run) => ({
  token: token ?? localToken(run),
  repo: repo ?? localRepo(run),
});

/**
 * Returns the token and the repository from the environment. Outside GitHub
 * Actions, a missing token comes from `gh auth token` and a missing repository
 * from the `origin` remote. In GitHub Actions nothing falls back.
 */
export const resolveDefaults = ({ env = process.env, run = runCommand } = {}) => {
  const fromEnv = {
    token: [env.GITHUB_TOKEN, env.GH_TOKEN].find(Boolean),
    repo: [env.GITHUB_REPOSITORY].find(Boolean),
  };
  return env.GITHUB_ACTIONS === "true" ? fromEnv : withLocalFallback(fromEnv, run);
};

const NUMERIC_OPTIONS = Object.freeze({
  "timeout-minutes": "timeoutMinutes",
  "poll-seconds": "pollSeconds",
  "missing-grace-minutes": "missingGraceMinutes",
});

const nonNegativeNumber = (name, value) => {
  const number = Number(value);
  if (value === "" || !Number.isFinite(number) || number < 0)
    throw new Error(`--${name} must be a number >= 0`);
  return number;
};

export const parseArguments = (argv, { repo = process.env.GITHUB_REPOSITORY } = {}) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      sha: { type: "string" },
      repo: { type: "string", default: repo },
      "timeout-minutes": { type: "string", default: String(DEFAULT_TIMEOUT_MINUTES) },
      "poll-seconds": { type: "string", default: String(DEFAULT_POLL_SECONDS) },
      "missing-grace-minutes": { type: "string", default: String(DEFAULT_MISSING_GRACE_MINUTES) },
      "dispatch-missing": { type: "boolean", default: false },
    },
    strict: true,
  });
  if (!/^[0-9a-f]{40}$/u.test(values.sha ?? ""))
    throw new Error("--sha must be a full 40-character commit SHA");
  if (!/^[\w.-]+\/[\w.-]+$/u.test(values.repo ?? ""))
    throw new Error("--repo or GITHUB_REPOSITORY must be owner/name");
  const options = {
    sha: values.sha,
    repo: values.repo,
    dispatchMissing: values["dispatch-missing"],
  };
  for (const [name, key] of Object.entries(NUMERIC_OPTIONS))
    options[key] = nonNegativeNumber(name, values[name]);
  return options;
};

const apiHeaders = (token) => ({
  Accept: "application/vnd.github+json",
  Authorization: `Bearer ${token}`,
  "X-GitHub-Api-Version": "2022-11-28",
});

/** Reads the SHA of main on GitHub, which is what `gh workflow run --ref main` runs. */
const fetchMainSha = async ({ repo, token }) => {
  const url = `https://api.github.com/repos/${repo}/git/ref/heads/${MAIN_BRANCH}`;
  const response = await fetch(url, {
    headers: apiHeaders(token),
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  });
  if (!response.ok) throw new Error(`GET ${url} returned HTTP ${response.status}`);
  return (await response.json()).object?.sha;
};

const dispatchCommand = (workflow) => `gh workflow run ${workflow.file} --ref ${MAIN_BRANCH}`;

const commandError = (result) =>
  result.error?.message ?? (result.stderr.trim() || `gh exited with ${result.status}`);

const ghDispatch = (repo) => (workflow) => {
  const result = spawnSync(
    "gh",
    ["workflow", "run", workflow.file, "--ref", MAIN_BRANCH, "--repo", repo],
    { encoding: "utf8" },
  );
  return result.status === 0 ? { ok: true } : { ok: false, error: commandError(result) };
};

const shortSha = (sha) => String(sha).slice(0, SHORT_SHA_LENGTH);

/** Starts each workflow once. Returns false when a start fails. */
const startWorkflows = (workflows, { sha, dispatched, dispatch, log }) => {
  for (const workflow of workflows) {
    const result = dispatch(workflow);
    if (!result.ok) {
      log(
        `::error::Could not start ${workflow.name} with ${dispatchCommand(workflow)}: ${result.error}`,
      );
      return false;
    }
    dispatched.add(workflow.file);
    log(`Started ${workflow.name}, which had no run for ${sha}: ${dispatchCommand(workflow)}`);
  }
  return true;
};

/**
 * Returns the missing workflows to start: none when a missing workflow has no
 * `workflow_dispatch` trigger, because the gate then fails anyway.
 */
const startableWorkflows = (evaluation, dispatched) =>
  evaluation.missing.some((workflow) => workflow.dispatchable === false)
    ? []
    : evaluation.missing.filter((workflow) => !dispatched.has(workflow.file));

/** True when main on GitHub is the release commit, so `--ref main` runs that commit. */
const mainIsRelease = async (sha, readMainSha, log) => {
  const mainSha = await readMainSha();
  if (mainSha === sha) return true;
  log(
    `::error::--dispatch-missing starts runs only while ${MAIN_BRANCH} is the release commit ` +
      `${shortSha(sha)}, but ${MAIN_BRANCH} is at ${shortSha(mainSha)}. ` +
      "Prepare the release again from the new commit.",
  );
  return false;
};

/**
 * Starts each missing required workflow that was not started before. Starts
 * nothing when a missing workflow cannot be dispatched, because the gate then
 * fails anyway. Returns false when the gate must fail now.
 */
const dispatchMissingRuns = async (evaluation, context) => {
  const missing = startableWorkflows(evaluation, context.dispatched);
  if (missing.length === 0) return { ok: true, started: 0 };
  if (!(await mainIsRelease(context.sha, context.readMainSha, context.log)))
    return { ok: false, started: 0 };
  const ok = startWorkflows(missing, context);
  return { ok, started: ok ? missing.length : 0 };
};

const fetchRuns = async ({ repo, sha, token }) => {
  const runs = [];
  for (let page = 1; ; page += 1) {
    const url = `https://api.github.com/repos/${repo}/actions/runs?head_sha=${sha}&per_page=${PER_PAGE}&page=${page}`;
    const response = await fetch(url, {
      headers: apiHeaders(token),
      // A hung connection must not stall the poll loop past its deadline.
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
    if (!response.ok) throw new Error(`GET ${url} returned HTTP ${response.status}`);
    const body = await response.json();
    runs.push(...body.workflow_runs);
    if (body.workflow_runs.length < PER_PAGE || runs.length >= body.total_count) return runs;
  }
};

const describe = (evaluation) =>
  `pass ${evaluation.passed.length}, pending ${evaluation.pending.length} ` +
  `(${evaluation.pending.map((entry) => entry.name).join(", ") || "none"}), ` +
  `missing ${evaluation.missing.length} (${evaluation.missing.map((workflow) => workflow.name).join(", ") || "none"})`;

const reportTimeout = (options, evaluation, log) => {
  log(
    `::error::Timed out after ${options.timeoutMinutes} min while CI runs on ${options.sha} did not finish.`,
  );
  if (evaluation === undefined) return;
  log(describe(evaluation));
  for (const entry of evaluation.pending) {
    log(
      `::error title=Unfinished CI run::${entry.name} is still ${entry.run.status}: ${entry.run.html_url}`,
    );
  }
  for (const line of errorLines(evaluation, options.sha)) log(line);
};

const reportResult = (options, evaluation, log) => {
  if (process.env.GITHUB_STEP_SUMMARY)
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, summaryTable(evaluation, options.sha));
  if (evaluation.state === "fail") {
    for (const line of errorLines(evaluation, options.sha)) log(line);
    log("A release must not start unless every check passed on the release commit.");
    return 1;
  }
  log(
    `Every CI run on ${options.sha} passed: ${evaluation.passed.map((entry) => entry.name).join(", ")}.`,
  );
  return 0;
};

/** True when --dispatch-missing can start runs: only missing runs block the gate. */
const shouldDispatch = (options, evaluation, allowMissing) =>
  options.dispatchMissing &&
  !allowMissing &&
  evaluation.failed.length === 0 &&
  evaluation.missing.length > 0;

/** Polls until the gate passes, fails or times out. Returns the process exit code. */
export const verifyReleaseCi = async (
  options,
  {
    token,
    required = REQUIRED_WORKFLOWS,
    now = Date.now,
    wait = sleep,
    log = console.log,
    readMainSha = () => fetchMainSha({ repo: options.repo, token }),
    dispatch = ghDispatch(options.repo),
  } = {},
) => {
  const started = now();
  const deadline = started + options.timeoutMinutes * 60_000;
  const dispatched = new Set();
  // A dispatched run needs a moment to appear, so a dispatch starts a new grace period.
  let graceStart = started;
  let apiErrors = 0;
  let evaluation;

  /** Reads and evaluates the runs once. Returns an exit code when the gate must stop now. */
  const poll = async () => {
    const runs = await fetchRuns({ repo: options.repo, sha: options.sha, token });
    apiErrors = 0;
    const allowMissing = now() - graceStart < options.missingGraceMinutes * 60_000;
    evaluation = evaluateRuns(runs, { required, allowMissing });
    if (!shouldDispatch(options, evaluation, allowMissing)) return null;
    const context = { sha: options.sha, dispatched, readMainSha, dispatch, log };
    const result = await dispatchMissingRuns(evaluation, context);
    if (!result.ok) return 1;
    if (result.started > 0) {
      graceStart = now();
      evaluation = { ...evaluation, state: "wait" };
    }
    return null;
  };

  for (;;) {
    try {
      const exit = await poll();
      if (exit !== null) return exit;
    } catch (error) {
      apiErrors += 1;
      log(
        `::warning::Could not read workflow runs (${apiErrors}/${MAX_CONSECUTIVE_API_ERRORS}): ${error.message}`,
      );
      if (apiErrors >= MAX_CONSECUTIVE_API_ERRORS) {
        log(
          "::error::The GitHub API failed too often. The release CI state is unknown, so the gate fails.",
        );
        return 1;
      }
    }

    if (evaluation !== undefined && evaluation.state !== "wait") break;
    if (now() + options.pollSeconds * 1000 > deadline) {
      reportTimeout(options, evaluation, log);
      return 1;
    }
    if (evaluation !== undefined) log(`Waiting for CI on ${options.sha}: ${describe(evaluation)}`);
    await wait(options.pollSeconds * 1000);
  }

  return reportResult(options, evaluation, log);
};

// `process.argv[1]` keeps symlinks while Node realpaths the ESM entry, so
// compare real paths. An unresolvable value means this module was imported.
const realPathOrNull = (value) => {
  try {
    return value === undefined ? null : realpathSync(value);
  } catch {
    return null;
  }
};

/** Returns the defaults, or throws when no token exists. */
const requireDefaults = () => {
  const defaults = resolveDefaults();
  if (defaults.token) return defaults;
  const hint = process.env.GITHUB_ACTIONS === "true" ? "" : ", or log in with gh auth login";
  throw new Error(`set GITHUB_TOKEN or GH_TOKEN${hint}`);
};

if (realPathOrNull(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const { token, repo } = requireDefaults();
    process.exitCode = await verifyReleaseCi(parseArguments(process.argv.slice(2), { repo }), {
      token,
    });
  } catch (error) {
    console.log(`::error::${error.message}`);
    process.exitCode = 1;
  }
}
