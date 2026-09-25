// Release gate: a release must not start unless every check passed on the
// release commit. Pull requests run only part of CI; the rest runs on push to
// main. This script reads the workflow runs for the release SHA and blocks the
// release until each required workflow has a successful run and no other run
// on that SHA has failed.
//
// Usage: node scripts/verify-release-ci.mjs --sha <sha> [--timeout-minutes 150]
//        [--poll-seconds 60] [--missing-grace-minutes 5] [--repo owner/name]
// Token: GITHUB_TOKEN or GH_TOKEN (needs `actions: read`).

import { appendFileSync, realpathSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

/**
 * Workflows that a release commit starts on push to main and that must have a
 * successful run on the release SHA. A release commit bumps Cargo.toml,
 * Cargo.lock, crates/*\/Cargo.toml and tools/type-aware-sidecar/package.json,
 * so every path filter below matches it.
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
  { name: "Module Coupling", file: "coupling.yml" },
  { name: "Fuzz Smoke", file: "fuzz-smoke.yml" },
  { name: "Binary Size", file: "bloat.yml" },
  { name: "Allocation Tracking", file: "allocs.yml" },
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
    : `Start it with: gh workflow run ${workflow.file} --ref main`;

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

export const parseArguments = (argv) => {
  const options = {
    sha: undefined,
    repo: process.env.GITHUB_REPOSITORY,
    timeoutMinutes: DEFAULT_TIMEOUT_MINUTES,
    pollSeconds: DEFAULT_POLL_SECONDS,
    missingGraceMinutes: DEFAULT_MISSING_GRACE_MINUTES,
  };
  const numeric = {
    "--timeout-minutes": "timeoutMinutes",
    "--poll-seconds": "pollSeconds",
    "--missing-grace-minutes": "missingGraceMinutes",
  };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === "--sha") options.sha = value;
    else if (flag === "--repo") options.repo = value;
    else if (flag in numeric) {
      const number = Number(value);
      if (!Number.isFinite(number) || number < 0) throw new Error(`${flag} must be a number >= 0`);
      options[numeric[flag]] = number;
    } else throw new Error(`unknown argument: ${flag}`);
  }
  if (!/^[0-9a-f]{40}$/u.test(options.sha ?? ""))
    throw new Error("--sha must be a full 40-character commit SHA");
  if (!/^[\w.-]+\/[\w.-]+$/u.test(options.repo ?? ""))
    throw new Error("--repo or GITHUB_REPOSITORY must be owner/name");
  return options;
};

const fetchRuns = async ({ repo, sha, token }) => {
  const runs = [];
  for (let page = 1; ; page += 1) {
    const url = `https://api.github.com/repos/${repo}/actions/runs?head_sha=${sha}&per_page=${PER_PAGE}&page=${page}`;
    const response = await fetch(url, {
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
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

/** Polls until the gate passes, fails or times out. Returns the process exit code. */
export const verifyReleaseCi = async (
  options,
  { token, now = Date.now, wait = sleep, log = console.log } = {},
) => {
  const started = now();
  const deadline = started + options.timeoutMinutes * 60_000;
  let apiErrors = 0;
  let evaluation;

  for (;;) {
    try {
      const runs = await fetchRuns({ repo: options.repo, sha: options.sha, token });
      apiErrors = 0;
      const allowMissing = now() - started < options.missingGraceMinutes * 60_000;
      evaluation = evaluateRuns(runs, { allowMissing });
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
      log(
        `::error::Timed out after ${options.timeoutMinutes} min while CI runs on ${options.sha} did not finish.`,
      );
      if (evaluation !== undefined) {
        log(describe(evaluation));
        for (const entry of evaluation.pending) {
          log(
            `::error title=Unfinished CI run::${entry.name} is still ${entry.run.status}: ${entry.run.html_url}`,
          );
        }
        for (const line of errorLines(evaluation, options.sha)) log(line);
      }
      return 1;
    }
    if (evaluation !== undefined) log(`Waiting for CI on ${options.sha}: ${describe(evaluation)}`);
    await wait(options.pollSeconds * 1000);
  }

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

// `process.argv[1]` keeps symlinks while Node realpaths the ESM entry, so
// compare real paths. An unresolvable value means this module was imported.
const realPathOrNull = (value) => {
  try {
    return value === undefined ? null : realpathSync(value);
  } catch {
    return null;
  }
};

if (realPathOrNull(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
    if (!token) throw new Error("set GITHUB_TOKEN or GH_TOKEN");
    process.exitCode = await verifyReleaseCi(parseArguments(process.argv.slice(2)), { token });
  } catch (error) {
    console.log(`::error::${error.message}`);
    process.exitCode = 1;
  }
}
