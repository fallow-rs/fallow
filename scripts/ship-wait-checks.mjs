#!/usr/bin/env node

// Wait until the checks of a pull request, or the workflow runs of a commit,
// are complete. Right after a push, GitHub can report no checks or only the
// first few, and "nothing pending" is then true too early. The --min-checks
// guard waits until at least that count of checks exists. GitHub starts no
// pull_request workflow for a pull request that conflicts with its base, so
// with --pr the wait also stops when GitHub reports a conflict. With
// --commit and --follow-main, a cancelled run moves the wait to the newest
// commit of origin/main when that commit contains the given commit.

import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as sleepMs } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const GH_PENDING_EXIT = 8;
const GH_FAILED_CHECK_EXIT = 1;
const GITHUB_RUN_URL_PATTERN =
  /^https:\/\/github\.com\/([\w.-]+)\/([\w.-]+)\/actions\/runs\/([1-9]\d*)(?:\/job\/[1-9]\d*)?$/u;
// gh prints "no required checks reported" for `--required` when no required
// check exists yet.
const NO_CHECKS_PATTERN = /no (?:required )?checks reported/iu;
const FAILED_BUCKETS = new Set(["fail", "cancel"]);
// gh run list matches --commit only against the full SHA. A short SHA gives
// an empty list and no error, so the wait would never see a run.
const FULL_SHA_PATTERN = /^[0-9a-f]{40}$/u;
const RUN_LIST_LIMIT = 100;
// gh pr view reports UNKNOWN until GitHub has computed the state, so only
// CONFLICTING stops the wait.
const CONFLICTING = "CONFLICTING";
// A push to main cancels the runs of the previous main commit through the
// concurrency group, so a cancelled run of a merge commit is usually no failure.
const CANCELLED_COMMIT_HINT =
  "A newer push to the branch can cancel the runs of this commit. Then wait for the newest commit that contains it, for example --commit $(git rev-parse origin/main).";
// The conclusions of a completed workflow run, as check buckets. Any other
// conclusion counts as a failure.
const RUN_BUCKETS = new Map([
  ["success", "pass"],
  ["neutral", "pass"],
  ["skipped", "skipping"],
  ["cancelled", "cancel"],
]);
// --follow-main reads the newest commit of this branch on this remote.
const FOLLOW_REMOTE = "origin";
const FOLLOW_BRANCH = "main";
const FOLLOW_REF = `${FOLLOW_REMOTE}/${FOLLOW_BRANCH}`;
// The maximum count of moves to a newer commit. Each move waits for a full
// CI run, so the --timeout budget usually stops the wait first.
const MAX_FOLLOW_HOPS = 10;
const GIT_NOT_ANCESTOR_EXIT = 1;
const SHORT_SHA_LENGTH = 12;
const SECOND = 1000;
const MINUTE = 60 * SECOND;
// Path filters decide which workflows a push to main starts, so one push can
// start fewer runs than --min-checks. When the runs of a commit stay complete
// and unchanged this long, no further run comes, and the wait ends.
const COMMIT_SETTLE_MS = 10 * MINUTE;
// A GitHub API outage of a few minutes must not stop the wait. After a failed
// read, the delay doubles from the interval up to this cap. The wait stops
// when the reads fail for the whole error window.
const READ_RETRY_MAX_MS = 5 * MINUTE;
const READ_ERROR_WINDOW_SECONDS = 600;

const USAGE = `Usage: node scripts/ship-wait-checks.mjs (--pr <number> | --commit <sha>) [options]

Wait until a pull request has at least --min-checks checks and none of them
is pending. Then print a summary of the checks. With --commit, wait for the
workflow runs of a commit instead, for example a merge commit on main.

Options:
  --pr <number>           The pull request.
  --commit <sha>          The full 40-character commit SHA. Each workflow
                          run of the commit counts as one check.
  --repo <owner/name>     The repository (default: the repository of the
                          working directory).
  --min-checks <count>    The count of checks that must exist (default: 1).
                          Use the check count of a recent complete run.
  --required              Wait for the required checks only (--pr only).
  --follow-main           When the only failures are cancelled runs, wait
                          for the newest commit of ${FOLLOW_REF} instead,
                          if it contains the given commit (--commit only).
  --interval <seconds>    The time between two reads (default: 30).
  --timeout <minutes>     The maximum wait (default: 60). With
                          --follow-main, the maximum wait for all commits.
  --error-window <seconds>
                          The maximum time of failed reads in a row
                          (default: ${READ_ERROR_WINDOW_SECONDS}). After each failed read, the
                          delay to the next read doubles, from --interval
                          up to ${READ_RETRY_MAX_MS / MINUTE} minutes.
  -h, --help              Show this help.

With --pr, the wait stops when the pull request conflicts with its base
branch, because GitHub then starts no pull_request workflows.

With --commit, path filters can start fewer runs than --min-checks. When the
runs stay complete and unchanged for ${COMMIT_SETTLE_MS / MINUTE} minutes, the wait ends below the count.

With --follow-main, the script runs "git fetch ${FOLLOW_REMOTE} ${FOLLOW_BRANCH}" in the
working directory, so it does not work with --repo. The wait moves to a
newer commit at most ${MAX_FOLLOW_HOPS} times. It stops with exit code 0 when all
runs of a commit that contains the given commit pass or skip.

Exit codes: 0 when all checks pass or skip, 1 when a check fails or is
cancelled, or when --follow-main reaches the maximum count of moves, 2 for
invalid input, failed reads for the full --error-window, a timeout, a failed read
of ${FOLLOW_REF}, or an ${FOLLOW_REF} that does not contain the given commit,
3 when the pull request conflicts with its base branch.`;

const formatSeconds = (ms) => {
  const seconds = Math.round(ms / SECOND);
  return `${seconds} ${seconds === 1 ? "second" : "seconds"}`;
};
const shortSha = (sha) => sha.slice(0, SHORT_SHA_LENGTH);
const commitLabel = (sha) => `Commit ${shortSha(sha)}`;

const bucketSummary = (checks) => {
  const counts = new Map();
  for (const { bucket } of checks) {
    counts.set(bucket, (counts.get(bucket) ?? 0) + 1);
  }
  return [...counts.entries()]
    .toSorted(([left], [right]) => left.localeCompare(right))
    .map(([bucket, count]) => `${bucket}=${count}`)
    .join(" ");
};

const checkSignature = (checks) =>
  checks
    .map(({ name, bucket }) => `${name}:${bucket}`)
    .toSorted()
    .join("\n");

/**
 * Read the checks until at least `minChecks` exist and none is pending.
 *
 * `readChecks()` returns `{ ok: true, checks }` or `{ ok: false, error }`.
 * `readMergeable()`, when given, returns `{ ok: true, mergeable }` or
 * `{ ok: false, error }`. It runs only while the checks are incomplete.
 * With `settleMs`, checks that stay complete and unchanged for that time end
 * the wait below `minChecks` with the status `settled`. Without it, too few
 * checks wait until the timeout.
 * Returns `{ status, checks }` where `status` is `pass`, `settled`, `fail`,
 * `conflict`, `timeout` or `error`. After a failed read of the checks, the
 * delay to the next read doubles from `intervalMs` up to `READ_RETRY_MAX_MS`.
 * The status is `error` (with `readErrors` and `errorMs`) when the planned
 * delays of the failed reads reach `errorWindowMs`, or when the deadline
 * comes during failed reads. A failed read of the mergeable state only warns.
 */
export const waitForChecks = async ({
  readChecks,
  readMergeable = null,
  settleMs = null,
  minChecks,
  intervalMs,
  timeoutMs,
  errorWindowMs = READ_ERROR_WINDOW_SECONDS * SECOND,
  sleep = sleepMs,
  now = Date.now,
  warn = console.error,
}) => {
  const deadline = now() + timeoutMs;
  let checks = [];
  let readErrors = 0;
  // The sum of the planned delays since the first failed read. A wall-clock
  // difference would count the time of a machine in sleep mode as failed reads.
  let errorMs = 0;
  let signature = "";
  let changedAt = now();
  for (;;) {
    let delayMs = intervalMs;
    const read = readChecks();
    if (read.ok) {
      readErrors = 0;
      errorMs = 0;
      checks = read.checks;
      const nextSignature = checkSignature(checks);
      if (nextSignature !== signature) {
        signature = nextSignature;
        changedAt = now();
      }
      const pending = checks.some(({ bucket }) => bucket === "pending");
      const failed = checks.some(({ bucket }) => FAILED_BUCKETS.has(bucket));
      if (checks.length >= minChecks && !pending) {
        return { status: failed ? "fail" : "pass", checks };
      }
      const settled =
        settleMs !== null && checks.length > 0 && !pending && now() - changedAt >= settleMs;
      if (settled) {
        return { status: failed ? "fail" : "settled", checks };
      }
    } else {
      readErrors += 1;
      const windowLeftMs = errorWindowMs - errorMs;
      delayMs = Math.min(intervalMs * 2 ** (readErrors - 1), READ_RETRY_MAX_MS, windowLeftMs);
      if (windowLeftMs <= 0 || now() + delayMs > deadline) {
        warn(`Read of the checks failed: ${read.error}`);
        return { status: "error", checks, readErrors, errorMs };
      }
      warn(`Read of the checks failed: ${read.error}. Next read in ${formatSeconds(delayMs)}.`);
    }
    if (readMergeable !== null) {
      const state = readMergeable();
      if (!state.ok) {
        warn(`Read of the mergeable state failed: ${state.error}`);
      } else if (state.mergeable === CONFLICTING) {
        return { status: "conflict", checks };
      }
    }
    if (now() + delayMs > deadline) {
      return { status: "timeout", checks };
    }
    await sleep(delayMs);
    if (readErrors > 0) {
      errorMs += delayMs;
    }
  }
};

/** True when the checks contain a cancelled run and no real failure. */
const onlyCancelled = (checks) =>
  checks.some(({ bucket }) => bucket === "cancel") &&
  checks.every(({ bucket }) => bucket !== "fail");

/**
 * Wait for the runs of `commit`. While the only failures are cancelled runs,
 * read the newest commit of the base branch, and wait for that commit when it
 * contains `commit`.
 *
 * `waitCommit(sha, timeoutMs)` returns the result of `waitForChecks` for one
 * commit. `readHead()` returns `{ ok: true, sha }` or `{ ok: false, error }`.
 * `isAncestor(ancestor, descendant)` returns `{ ok: true, contains }` or
 * `{ ok: false, error }`. The timeout is one budget for all commits.
 * Returns the result of the last wait with the fields `commit` (the commit
 * of that wait) and `origin` (the given commit). The `status` can also be
 * `hops`, `diverged` (with `head`) or `head-error` (with `error`).
 */
export const followMain = async ({
  commit,
  waitCommit,
  readHead,
  isAncestor,
  timeoutMs,
  maxHops = MAX_FOLLOW_HOPS,
  now = Date.now,
  log = console.log,
}) => {
  const deadline = now() + timeoutMs;
  let current = commit;
  for (let hops = 0; ; hops += 1) {
    const result = await waitCommit(current, Math.max(deadline - now(), 0));
    const done = { ...result, commit: current, origin: commit };
    if (result.status !== "fail" || !onlyCancelled(result.checks)) {
      return done;
    }
    if (hops >= maxHops) {
      return { ...done, status: "hops", maxHops };
    }
    const head = readHead();
    if (!head.ok) {
      return { ...done, status: "head-error", error: head.error };
    }
    if (head.sha === current) {
      log(
        `${commitLabel(current)} is the newest commit of ${FOLLOW_REF}, so no newer push cancelled its runs.`,
      );
      return done;
    }
    const contained = isAncestor(commit, head.sha);
    if (!contained.ok) {
      return { ...done, status: "head-error", error: contained.error };
    }
    if (!contained.contains) {
      return { ...done, status: "diverged", head: head.sha };
    }
    log(
      `${commitLabel(current)}: runs cancelled (${bucketSummary(result.checks)}). Wait for ${shortSha(head.sha)} of ${FOLLOW_REF} (move ${hops + 1} of ${maxHops}).`,
    );
    current = head.sha;
  }
};

/**
 * Turn the result of a `git rev-parse` run into `{ ok: true, sha }` or
 * `{ ok: false, error }`.
 */
export const parseGitHead = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  const sha = stdout.trim();
  if (status !== 0 || !FULL_SHA_PATTERN.test(sha)) {
    return { ok: false, error: stderr.trim() || `git exited with ${status}` };
  }
  return { ok: true, sha };
};

/**
 * Turn the result of a `git merge-base --is-ancestor` run into
 * `{ ok: true, contains }` or `{ ok: false, error }`. Git exits with 0 when
 * the descendant contains the ancestor, and with 1 when it does not.
 */
export const parseGitAncestor = ({ error, status, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status === 0 || status === GIT_NOT_ANCESTOR_EXIT) {
    return { ok: true, contains: status === 0 };
  }
  return { ok: false, error: stderr.trim() || `git exited with ${status}` };
};

const runGit = (args) => spawnSync("git", args, { encoding: "utf8" });

/** Fetch the base branch, then read its newest commit. */
const gitHeadReader = (git) => () => {
  const fetched = git(["fetch", "--quiet", FOLLOW_REMOTE, FOLLOW_BRANCH]);
  if (fetched.error) {
    return { ok: false, error: fetched.error.message };
  }
  if (fetched.status !== 0) {
    return { ok: false, error: fetched.stderr.trim() || `git exited with ${fetched.status}` };
  }
  return parseGitHead(git(["rev-parse", "--verify", `refs/remotes/${FOLLOW_REF}`]));
};

/** Check with git that `descendant` contains `ancestor`. */
const gitAncestorChecker = (git) => (ancestor, descendant) =>
  parseGitAncestor(git(["merge-base", "--is-ancestor", ancestor, descendant]));

/**
 * Turn the result of a `gh pr checks` run (`{ error, status, stdout,
 * stderr }`, as `spawnSync` returns it) into `{ ok: true, checks }` or
 * `{ ok: false, error }`.
 */
export const parseGhChecks = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  // Pending and failed checks still have complete JSON for name selection.
  const failedChecks = status === GH_FAILED_CHECK_EXIT && stdout.trim() !== "";
  if (status === 0 || status === GH_PENDING_EXIT || failedChecks) {
    try {
      return { ok: true, checks: JSON.parse(stdout) };
    } catch (parseError) {
      return { ok: false, error: `gh printed invalid JSON: ${parseError.message}` };
    }
  }
  if (NO_CHECKS_PATTERN.test(stderr)) {
    return { ok: true, checks: [] };
  }
  return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
};

const rollupCheck = (entry) => {
  if (
    entry?.__typename === "CheckRun" &&
    typeof entry.name === "string" &&
    typeof entry.status === "string"
  ) {
    return {
      name: entry.name,
      bucket:
        entry.status === "COMPLETED"
          ? (RUN_BUCKETS.get(entry.conclusion?.toLowerCase()) ?? "fail")
          : "pending",
      link: entry.detailsUrl,
    };
  }
  if (
    entry?.__typename === "StatusContext" &&
    typeof entry.context === "string" &&
    typeof entry.state === "string"
  ) {
    const pending = entry.state === "PENDING" || entry.state === "EXPECTED";
    return {
      name: entry.context,
      bucket: pending ? "pending" : entry.state === "SUCCESS" ? "pass" : "fail",
      link: entry.targetUrl,
    };
  }
  throw new Error("invalid statusCheckRollup entry");
};

const worstCheck = (checks) =>
  checks.find(({ bucket }) => bucket === "pending") ??
  checks.find(({ bucket }) => bucket === "fail") ??
  checks.find(({ bucket }) => bucket === "cancel") ??
  checks[0];

const supersededCancellations = (rollup, checks, readRun, names) => {
  const removed = new Set();
  // A replacement with a different job name cannot satisfy a required check.
  if (readRun === null || names !== null) {
    return { ok: true, removed };
  }
  const workflows = new Map();
  for (const [index, entry] of rollup.entries()) {
    if (
      entry.__typename !== "CheckRun" ||
      typeof entry.workflowName !== "string" ||
      entry.workflowName === ""
    ) {
      continue;
    }
    const group = workflows.get(entry.workflowName) ?? [];
    group.push(index);
    workflows.set(entry.workflowName, group);
  }
  for (const group of workflows.values()) {
    if (
      !group.some((index) => checks[index].bucket === "cancel") ||
      !group.some((index) => checks[index].bucket === "pass") ||
      new Set(group.map((index) => checks[index].name)).size < 2
    ) {
      continue;
    }
    const identities = new Map();
    for (const index of group) {
      const result = readRun(rollup[index].detailsUrl);
      if (!result.ok) {
        return result;
      }
      if (result.run === null) {
        continue;
      }
      const key = JSON.stringify([result.run.workflowId, result.run.head]);
      const runs = identities.get(key) ?? [];
      runs.push({ index, ...result.run });
      identities.set(key, runs);
    }
    for (const runs of identities.values()) {
      const newest = Math.max(...runs.map(({ createdAt }) => createdAt));
      const latest = runs.filter(({ createdAt }) => createdAt === newest);
      const runIds = new Set(latest.map(({ id }) => id));
      if (
        runIds.size !== 1 ||
        latest[0].id === undefined ||
        !latest.some(({ index }) => checks[index].bucket === "pass") ||
        latest.some(({ index }) => !["pass", "skipping"].includes(checks[index].bucket))
      ) {
        continue;
      }
      // Matrix expansion can rename every job. Only a completed successful
      // workflow proves replacement; a newer skipped label event does not.
      const replacement = readRun(rollup[latest[0].index].detailsUrl, { fresh: true });
      if (!replacement.ok) {
        return replacement;
      }
      if (
        replacement.run?.id !== latest[0].id ||
        replacement.run?.createdAt !== newest ||
        replacement.run?.workflowId !== latest[0].workflowId ||
        replacement.run?.head !== latest[0].head ||
        replacement.run?.status !== "completed" ||
        replacement.run?.conclusion !== "success"
      ) {
        continue;
      }
      for (const { index, createdAt } of runs) {
        if (createdAt < newest && checks[index].bucket === "cancel") {
          removed.add(index);
        }
      }
    }
  }
  return { ok: true, removed };
};

const currentRollupChecks = (rollup, readRun, names) => {
  const checks = rollup.map(rollupCheck);
  const superseded = supersededCancellations(rollup, checks, readRun, names);
  if (!superseded.ok) {
    return superseded;
  }
  const removed = superseded.removed;
  const groups = new Map();
  for (const [index, entry] of rollup.entries()) {
    if (
      removed.has(index) ||
      entry.__typename !== "CheckRun" ||
      typeof entry.workflowName !== "string" ||
      entry.workflowName === "" ||
      (names !== null && !names.has(entry.name))
    ) {
      continue;
    }
    const key = JSON.stringify([entry.workflowName, entry.name]);
    const group = groups.get(key) ?? [];
    group.push(index);
    groups.set(key, group);
  }
  for (const group of groups.values()) {
    if (group.length < 2) {
      continue;
    }
    const identities = new Map();
    const unknown = [];
    for (const index of group) {
      const result = readRun === null ? { ok: true, run: null } : readRun(rollup[index].detailsUrl);
      if (!result.ok) {
        return result;
      }
      if (result.run === null) {
        unknown.push(index);
        continue;
      }
      const key = JSON.stringify([result.run.workflowId, result.run.head]);
      const runs = identities.get(key) ?? [];
      runs.push({ index, ...result.run });
      identities.set(key, runs);
    }
    // Job start and completion order can differ from workflow creation order.
    // Display names alone cannot distinguish different workflows or commits.
    const representatives = [];
    for (const runs of identities.values()) {
      const newest = Math.max(...runs.map(({ createdAt }) => createdAt));
      const latest = runs.filter(({ createdAt }) => createdAt === newest);
      const active = latest.length === 1 ? latest : runs;
      const first = runs[0].index;
      checks[first] = worstCheck(active.map(({ index }) => checks[index]));
      representatives.push(first);
      for (const { index } of runs.slice(1)) {
        removed.add(index);
      }
    }
    // Unknown entries prove no additional identity, but must keep blocking.
    if (unknown.length > 0) {
      const first = representatives[0] ?? unknown[0];
      const candidates = unknown.map((index) => checks[index]);
      if (representatives.length > 0) {
        candidates.unshift(checks[first]);
      }
      checks[first] = worstCheck(candidates);
      for (const index of unknown) {
        if (index !== first) {
          removed.add(index);
        }
      }
    }
  }
  return {
    ok: true,
    checks: checks.filter(
      ({ name }, index) => !removed.has(index) && (names === null || names.has(name)),
    ),
  };
};

/**
 * Read the authoritative current-head checks returned by `gh pr view`.
 * Optional `readRun(url)` supplies workflow identity and creation metadata
 * for duplicate or canceled workflow checks; `{ fresh: true }` also requests
 * current run status. `names` limits checks to the required selection.
 */
export const parseGhPrRollup = (
  { error, status, stdout, stderr },
  { readRun = null, names = null } = {},
) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status !== 0) {
    return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
  }
  let rollup;
  try {
    rollup = JSON.parse(stdout)?.statusCheckRollup;
  } catch (parseError) {
    return { ok: false, error: `gh printed invalid JSON: ${parseError.message}` };
  }
  if (rollup === null) {
    return { ok: true, checks: [] };
  }
  if (!Array.isArray(rollup)) {
    return { ok: false, error: "gh printed invalid statusCheckRollup" };
  }
  try {
    return currentRollupChecks(rollup, readRun, names);
  } catch (parseError) {
    return { ok: false, error: `gh printed invalid check rollup: ${parseError.message}` };
  }
};

const ghRunMetadataReader = () => {
  const cached = new Map();
  return (detailsUrl, { fresh = false } = {}) => {
    const match = typeof detailsUrl === "string" ? GITHUB_RUN_URL_PATTERN.exec(detailsUrl) : null;
    if (match === null) {
      return { ok: true, run: null };
    }
    const [, owner, repo, id] = match;
    const endpoint = `repos/${owner}/${repo}/actions/runs/${id}`;
    if (!fresh && cached.has(endpoint)) {
      return { ok: true, run: cached.get(endpoint) };
    }
    const result = spawnSync("gh", ["api", endpoint, "--hostname", "github.com"], {
      encoding: "utf8",
    });
    if (result.error || result.status !== 0) {
      return {
        ok: false,
        error: result.error?.message ?? (result.stderr.trim() || `gh exited with ${result.status}`),
      };
    }
    let data;
    try {
      data = JSON.parse(result.stdout);
    } catch (parseError) {
      return { ok: false, error: `gh printed invalid workflow-run JSON: ${parseError.message}` };
    }
    const createdAt = typeof data?.created_at === "string" ? Date.parse(data.created_at) : NaN;
    if (
      !(createdAt > 0) ||
      data.html_url !== `https://github.com/${owner}/${repo}/actions/runs/${id}` ||
      !Number.isSafeInteger(data.workflow_id) ||
      data.workflow_id <= 0 ||
      typeof data.head_sha !== "string" ||
      !FULL_SHA_PATTERN.test(data.head_sha)
    ) {
      return { ok: false, error: "gh printed invalid workflow-run metadata" };
    }
    const run = { id: endpoint, createdAt, workflowId: data.workflow_id, head: data.head_sha };
    cached.set(endpoint, run);
    return {
      ok: true,
      run: fresh ? { ...run, status: data.status, conclusion: data.conclusion } : run,
    };
  };
};

/**
 * Turn the result of a `gh run list` run into `{ ok: true, checks }` or
 * `{ ok: false, error }`. Each workflow run becomes one check.
 */
export const parseGhRuns = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status !== 0) {
    return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
  }
  let runs;
  try {
    runs = JSON.parse(stdout);
  } catch (parseError) {
    return { ok: false, error: `gh printed invalid JSON: ${parseError.message}` };
  }
  const checks = runs.map(({ workflowName, status: runStatus, conclusion, url }) => ({
    name: workflowName,
    bucket: runStatus === "completed" ? (RUN_BUCKETS.get(conclusion) ?? "fail") : "pending",
    link: url,
  }));
  return { ok: true, checks };
};

/**
 * Turn the result of a `gh pr view --json mergeable` run into
 * `{ ok: true, mergeable }` or `{ ok: false, error }`.
 */
export const parseGhMergeable = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status !== 0) {
    return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
  }
  try {
    return { ok: true, mergeable: JSON.parse(stdout).mergeable };
  } catch (parseError) {
    return { ok: false, error: `gh printed invalid JSON: ${parseError.message}` };
  }
};

/** Read the mergeable state of `pr` with `gh pr view`. */
const ghMergeableReader =
  ({ pr, repo }) =>
  () => {
    const args = ["pr", "view", pr, "--json", "mergeable,mergeStateStatus"];
    if (repo !== null) {
      args.push("--repo", repo);
    }
    return parseGhMergeable(spawnSync("gh", args, { encoding: "utf8" }));
  };

/** Read the workflow runs of `commit` with `gh run list`. */
const ghRunsReader =
  ({ commit, repo }) =>
  () => {
    const args = [
      "run",
      "list",
      "--commit",
      commit,
      "--limit",
      String(RUN_LIST_LIMIT),
      "--json",
      "workflowName,status,conclusion,url",
    ];
    if (repo !== null) {
      args.push("--repo", repo);
    }
    return parseGhRuns(spawnSync("gh", args, { encoding: "utf8" }));
  };

/** Read current-head states; `gh pr checks --required` selects names only. */
const ghChecksReader = ({ pr, repo, required }) => {
  const readRun = ghRunMetadataReader();
  return () => {
    const args = ["pr", "view", pr, "--json", "statusCheckRollup"];
    if (repo !== null) {
      args.push("--repo", repo);
    }
    let selected = null;
    if (required) {
      const selection = ["pr", "checks", pr, "--required", "--json", "name,bucket,link"];
      if (repo !== null) {
        selection.push("--repo", repo);
      }
      selected = parseGhChecks(spawnSync("gh", selection, { encoding: "utf8" }));
      if (!selected.ok) {
        return selected;
      }
      if (
        !Array.isArray(selected.checks) ||
        selected.checks.some((check) => typeof check?.name !== "string")
      ) {
        return { ok: false, error: "gh printed invalid required-check selection" };
      }
      if (selected.checks.length === 0) {
        return selected;
      }
    }
    const names = selected === null ? null : new Set(selected.checks.map(({ name }) => name));
    const current = parseGhPrRollup(spawnSync("gh", args, { encoding: "utf8" }), {
      readRun,
      names,
    });
    if (!current.ok || selected === null) {
      return current;
    }
    const checks = current.checks;
    const present = new Set(checks.map(({ name }) => name));
    checks.push(
      ...selected.checks
        .filter(({ name }) => !present.has(name))
        .map((check) => ({ ...check, bucket: "pending" })),
    );
    return { ok: true, checks };
  };
};

const positiveInteger = (name, value) => {
  if (!/^[1-9]\d*$/u.test(value)) {
    throw new Error(`${name} must be a positive integer, not "${value}".`);
  }
  return Number(value);
};

const parseOptions = (argv) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      pr: { type: "string" },
      commit: { type: "string" },
      repo: { type: "string" },
      "min-checks": { type: "string", default: "1" },
      required: { type: "boolean", default: false },
      "follow-main": { type: "boolean", default: false },
      interval: { type: "string", default: "30" },
      timeout: { type: "string", default: "60" },
      "error-window": { type: "string", default: String(READ_ERROR_WINDOW_SECONDS) },
      help: { type: "boolean", short: "h", default: false },
    },
    strict: true,
  });
  if (values.help) {
    return { help: true };
  }
  if ((values.pr === undefined) === (values.commit === undefined)) {
    throw new Error("Give exactly one of --pr and --commit.");
  }
  if (values["follow-main"]) {
    if (values.commit === undefined) {
      throw new Error("--follow-main works only with --commit.");
    }
    if (values.repo !== undefined) {
      throw new Error(
        `--follow-main reads ${FOLLOW_REF} of the working directory, so it does not work with --repo.`,
      );
    }
  }
  if (values.commit !== undefined) {
    if (!FULL_SHA_PATTERN.test(values.commit)) {
      throw new Error(`--commit must be a full 40-character SHA, not "${values.commit}".`);
    }
    if (values.required) {
      throw new Error("--required works only with --pr.");
    }
  }
  const pr = values.pr === undefined ? null : String(positiveInteger("--pr", values.pr));
  return {
    help: false,
    pr,
    commit: values.commit ?? null,
    label: pr === null ? commitLabel(values.commit) : `PR ${pr}`,
    repo: values.repo ?? null,
    required: values.required,
    followMain: values["follow-main"],
    minChecks: positiveInteger("--min-checks", values["min-checks"]),
    intervalMs: positiveInteger("--interval", values.interval) * SECOND,
    timeoutMs: positiveInteger("--timeout", values.timeout) * MINUTE,
    errorWindowMs: positiveInteger("--error-window", values["error-window"]) * SECOND,
  };
};

const EXIT_CODES = {
  pass: 0,
  settled: 0,
  fail: 1,
  hops: 1,
  timeout: 2,
  error: 2,
  "head-error": 2,
  diverged: 2,
  conflict: 3,
};

/**
 * Print the result of `waitForChecks` or `followMain` and return the exit
 * code.
 */
export const reportChecks = (
  {
    status,
    checks,
    origin = "",
    head = "",
    error = "",
    maxHops = MAX_FOLLOW_HOPS,
    readErrors = 0,
    errorMs = 0,
  },
  { label, minChecks, settleMs = COMMIT_SETTLE_MS, commit = null, followMain: following = false },
  log = console.log,
) => {
  const summary = checks.length === 0 ? "no checks" : bucketSummary(checks);
  const headline = {
    pass: `${label}: all checks passed (${summary}).`,
    settled: `${label}: all ${checks.length} runs passed, fewer than --min-checks ${minChecks}, and no run started or changed for ${settleMs / MINUTE} minutes (${summary}).`,
    fail: `${label}: checks failed (${summary}).`,
    timeout: `${label}: timed out with ${checks.length} of at least ${minChecks} checks (${summary}).`,
    error: `${label}: stopped after ${readErrors} failed reads of the checks in ${formatSeconds(errorMs)}.`,
    hops: `${label}: runs cancelled again after ${maxHops} moves to a newer commit (${summary}).`,
    "head-error": `${label}: read of ${FOLLOW_REF} failed: ${error}`,
    diverged: `${label}: ${FOLLOW_REF} at ${shortSha(head)} does not contain commit ${shortSha(origin)}, so the wait stops.`,
    conflict: `${label}: the pull request conflicts with its base branch, so GitHub starts no pull_request workflows (${summary}). Update the branch, push, and wait again.`,
  }[status];
  log(headline);
  for (const check of checks.filter(({ bucket }) => FAILED_BUCKETS.has(bucket))) {
    log(`  ${check.bucket.toUpperCase()}: ${check.name} ${check.link ?? ""}`.trimEnd());
  }
  if (!following && commit !== null && checks.some(({ bucket }) => bucket === "cancel")) {
    log(CANCELLED_COMMIT_HINT);
  }
  return EXIT_CODES[status];
};

const main = async () => {
  let options;
  try {
    options = parseOptions(process.argv.slice(2));
  } catch (error) {
    console.error(`${error.message}\n\n${USAGE}`);
    return 2;
  }
  if (options.help) {
    console.log(USAGE);
    return 0;
  }
  if (options.followMain) {
    const result = await followMain({
      commit: options.commit,
      timeoutMs: options.timeoutMs,
      waitCommit: (sha, timeoutMs) =>
        waitForChecks({
          ...options,
          settleMs: COMMIT_SETTLE_MS,
          timeoutMs,
          readChecks: ghRunsReader({ commit: sha, repo: options.repo }),
        }),
      readHead: gitHeadReader(runGit),
      isAncestor: gitAncestorChecker(runGit),
    });
    return reportChecks(result, { ...options, label: commitLabel(result.commit) });
  }
  const isPr = options.commit === null;
  const readChecks = isPr ? ghChecksReader(options) : ghRunsReader(options);
  const readMergeable = isPr ? ghMergeableReader(options) : null;
  const settleMs = isPr ? null : COMMIT_SETTLE_MS;
  const result = await waitForChecks({ ...options, readChecks, readMergeable, settleMs });
  return reportChecks(result, options);
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main();
}
