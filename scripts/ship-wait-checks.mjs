#!/usr/bin/env node

// Wait until the checks of a pull request, or the workflow runs of a commit,
// are complete. Right after a push, GitHub can report no checks or only the
// first few, and "nothing pending" is then true too early. For a pull request,
// include queued workflows before they register any checks. Known optional
// workflows remain excluded in required-only mode.
// The --min-checks guard waits until at least that count of checks exists. For
// a commit, the runs that GitHub created are the expected set, and the wait
// ends when they stay complete for a short settle time. GitHub starts no
// pull_request workflow for a pull request that conflicts with its base, so
// with --pr the wait also stops when GitHub reports a conflict. With
// --commit and --follow-main, a cancelled run moves the wait to the newest
// commit of origin/main when that commit contains the given commit. With
// --pr, a failed check that also fails on main gets its own exit code, so a
// ship step can wait for the fix on main instead of a fix on the branch.
// GitHub does not start the workflows of a pull request from a fork until a
// maintainer approves them. Such a run is complete with the conclusion
// action_required and has no checks, so with --pr the wait stops at once
// instead of a pass on the checks of the other workflows. When a change touches
// the released skills, the wait prints the companion sync step: after a pass
// with --pr, and on any result with --commit.

import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as sleepMs } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { RELEASED_SKILLS_ROOT } from "./released-skills.mjs";

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
// A failed check of a pull request is compared with the newest completed run
// of the same check name on these newest commits of main. The newest commit
// can still run, so older commits are a fallback.
const MAIN_COMMIT_LIMIT = 10;
const SHORT_SHA_LENGTH = 12;
const SECOND = 1000;
const MINUTE = 60 * SECOND;
// Path filters decide which workflows a push starts, so a commit or a pull
// request can get fewer checks than --min-checks. When the checks stay
// complete and unchanged this long, no further check comes, and the wait ends.
const SETTLE_MS = 10 * MINUTE;
// A pull request check set without a pass is not complete. The
// dependabot-auto-merge workflow skips at once on each pull request, so
// right after a push the only check can be a skip, before ci.yml registers
// its jobs.
const PASS_BUCKET = "pass";
// A pull-request workflow run that waits for maintainer approval.
const APPROVAL_BUCKET = "approval";
const APPROVAL_CONCLUSION = "action_required";
// GitHub creates all push runs of a commit together, and a new run is pending.
// So a complete set of runs contains all push runs. The short settle time catches a
// run that a completed run starts, for example through workflow_run.
export const COMMIT_SETTLE_MS = 2 * MINUTE;
// A GitHub API outage of a few minutes must not stop the wait. After a failed
// read, the delay doubles from the interval up to this cap. The wait stops
// when the reads fail for the whole error window.
const READ_RETRY_MAX_MS = 5 * MINUTE;
const READ_ERROR_WINDOW_SECONDS = 600;

const USAGE = `Usage: node scripts/ship-wait-checks.mjs (--pr <number> | --commit <sha>) [options]

Wait until a pull request has at least --min-checks checks and none of them
is pending. Then print a summary of the checks. With --commit, wait for the
workflow runs of a commit instead, for example a merge commit on main.

Pull request waits also discover unfinished workflows on the reported head,
including workflows queued before any jobs exist. With --required, a workflow
with registered checks is ignored when none of its checks is selected.

With --commit, the expected set is the set of runs that GitHub created for
the commit. Path filters decide which workflows a push starts, so the count
changes from commit to commit. The wait ends when all runs are complete and
no run started or changed for ${COMMIT_SETTLE_MS / MINUTE} minutes. A read without runs never ends
the wait.

When the change touches ${RELEASED_SKILLS_ROOT.join("/")}/, the script prints the step for
scripts/sync-skills-companion.mjs: after a pass with --pr, and on any result
with --commit. A failed read of the changed files only prints a warning.

Options:
  --pr <number>           The pull request.
  --commit <sha>          The full 40-character commit SHA. Each workflow
                          run of the commit counts as one check.
  --repo <owner/name>     The repository (default: the repository of the
                          working directory).
  --min-checks <count>    The count of checks that must exist. Use the
                          check count of a recent complete run. With --pr
                          and no --min-checks, the count is 1, and the
                          checks must stay complete and unchanged for
                          ${COMMIT_SETTLE_MS / MINUTE} minutes before the wait passes. With
                          --commit, the option is not necessary. When that
                          count of runs is complete, the wait ends before
                          the settle time.
  --required              Wait for the required checks only (--pr only).
                          When the repository reports no required checks,
                          wait for all checks.
  --head <sha>            Wait until the pull request head is this full
                          40-character SHA (--pr only). Right after a push,
                          GitHub can still report the checks of the old head.
                          Without --head, the script prints a warning.
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

With --pr, path filters can start fewer checks than --min-checks. When the
checks stay complete and unchanged for ${SETTLE_MS / MINUTE} minutes, the wait ends below the
count.

With --pr, the wait stops when a workflow run waits for maintainer approval,
which GitHub requires for some pull requests from a fork. No wait ends this
state: read the code of the pull request, approve the runs, and wait again.

With --pr, a set of checks without a passed check is not complete. Some
workflows skip at once on each pull request, before the CI workflow starts
its jobs. Such a set never passes. When it stays unchanged for ${SETTLE_MS / MINUTE} minutes,
the wait ends with exit code 0 and says that no check passed.

With --follow-main, the script runs "git fetch ${FOLLOW_REMOTE} ${FOLLOW_BRANCH}" in the
working directory, so it does not work with --repo. The wait moves to a
newer commit at most ${MAX_FOLLOW_HOPS} times. It stops with exit code 0 when all
runs of a commit that contains the given commit pass or skip.

Exit codes: 0 when all checks pass or skip, 1 when a check fails or is
cancelled, or when --follow-main reaches the maximum count of moves, 2 for
invalid input, failed reads for the full --error-window, a timeout, a failed read
of ${FOLLOW_REF}, or an ${FOLLOW_REF} that does not contain the given commit,
3 when the pull request conflicts with its base branch, 4 when each failed
check also fails on ${FOLLOW_BRANCH}, 5 when a workflow run waits for maintainer approval.

With --pr, the script compares each failed check with the newest completed
run of the same check name on the newest ${MAIN_COMMIT_LIMIT} commits of ${FOLLOW_BRANCH}. A check that
also fails on ${FOLLOW_BRANCH} gets the line "FAIL (also on ${FOLLOW_BRANCH} <sha>)". When all failed
checks fail on ${FOLLOW_BRANCH} too, the exit code is 4: wait for the fix on ${FOLLOW_BRANCH}, merge
${FOLLOW_BRANCH} into the branch, push, and wait again. A cancelled check, or a failed
check that ${FOLLOW_BRANCH} does not have, keeps exit code 1.

With --commit, the script compares each failed check in the same way with
the ${MAIN_COMMIT_LIMIT} commits before the given commit. With --follow-main, these are
still the commits before the given commit, not before the newer commit that
the wait moved to. A check that also failed before the commit gets the same
line, and when all failed checks failed before, the exit code is 4: the
failure does not come from the commit. A cancelled or running run on ${FOLLOW_BRANCH} does not count,
so the comparison moves to an older commit.`;

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

const hasBucket = (checks, bucket) => checks.some((check) => check.bucket === bucket);
const hasFailure = (checks) => checks.some(({ bucket }) => FAILED_BUCKETS.has(bucket));

/** The status of a complete set that reached `minChecks`, or null. */
const countedStatus = (checks, unchangedMs, { requirePass, stableMs }) => {
  if (hasFailure(checks)) {
    return "fail";
  }
  const passed = !requirePass || hasBucket(checks, PASS_BUCKET);
  return passed && unchangedMs >= stableMs ? "pass" : null;
};

/**
 * The end status of one read of the checks, or null when the wait goes on.
 * `unchangedMs` is the time since the check set last changed.
 */
const completedStatus = (checks, unchangedMs, options) => {
  if (hasBucket(checks, APPROVAL_BUCKET)) {
    return hasFailure(checks) ? "fail" : "approval";
  }
  if (hasBucket(checks, "pending")) {
    return null;
  }
  const counted =
    checks.length >= options.minChecks ? countedStatus(checks, unchangedMs, options) : null;
  if (counted !== null) {
    return counted;
  }
  const settled = options.settleMs !== null && checks.length > 0 && unchangedMs >= options.settleMs;
  if (!settled) {
    return null;
  }
  return hasFailure(checks) ? "fail" : "settled";
};

/**
 * Read the checks until at least `minChecks` exist and none is pending.
 *
 * `readChecks()` returns `{ ok: true, checks }` or `{ ok: false, error }`.
 * `readMergeable()`, when given, returns `{ ok: true, mergeable }` or
 * `{ ok: false, error }`. It runs only while the checks are incomplete.
 * With `settleMs`, checks that stay complete and unchanged for that time end
 * the wait below `minChecks` with the status `settled`. Without it, too few
 * checks wait until the timeout.
 * With `requirePass`, a complete set without a `pass` check does not pass. It
 * can only end `settled` through `settleMs`. With `stableMs`, a complete set
 * passes only after it stays unchanged for that time. A failed check fails
 * the wait at once.
 * A check in the `approval` bucket ends the wait at once with the status
 * `approval`, because only a maintainer can start that run. A failed check
 * still wins.
 * Returns `{ status, checks }` where `status` is `pass`, `settled`, `fail`,
 * `approval`, `conflict`, `timeout` or `error`. After a failed read of the checks, the
 * delay to the next read doubles from `intervalMs` up to `READ_RETRY_MAX_MS`.
 * The status is `error` (with `readErrors` and `errorMs`) when the planned
 * delays of the failed reads reach `errorWindowMs`, or when the deadline
 * comes during failed reads. A failed read of the mergeable state only warns.
 */
export const waitForChecks = async ({
  readChecks,
  readMergeable = null,
  settleMs = null,
  requirePass = false,
  stableMs = 0,
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
      const status = completedStatus(checks, now() - changedAt, {
        minChecks,
        requirePass,
        stableMs,
        settleMs,
      });
      if (status !== null) {
        return { status, checks };
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

/**
 * Wait for the workflow runs of a commit. The expected set is the set of runs
 * that GitHub created for the commit, so no count is necessary. The wait
 * passes when that set is complete and unchanged for `COMMIT_SETTLE_MS`.
 * A read without runs never passes. With `minChecks`, the wait also passes
 * as soon as that count of runs is complete. The other options and the
 * result are the same as for `waitForChecks`.
 */
export const waitForCommitRuns = ({ minChecks = null, ...options }) =>
  waitForChecks({
    ...options,
    minChecks: minChecks ?? Number.POSITIVE_INFINITY,
    settleMs: COMMIT_SETTLE_MS,
  });

/**
 * Wait for the checks of a pull request. A complete set needs at least one
 * `pass` check, because a set of skips only is not complete. Without
 * `minChecks`, the count is 1, and a complete set passes only after it stays
 * unchanged for `COMMIT_SETTLE_MS`. An explicit `minChecks` passes as soon as
 * that count is complete. The settle time is `SETTLE_MS` by default. The
 * other options and the result are the same as for `waitForChecks`.
 */
export const waitForPullRequestChecks = ({ minChecks = null, settleMs = SETTLE_MS, ...options }) =>
  waitForChecks({
    ...options,
    minChecks: minChecks ?? 1,
    settleMs,
    requirePass: true,
    stableMs: minChecks === null ? COMMIT_SETTLE_MS : 0,
  });

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
const parseGhPrRollup = (
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
 * Turn the result of a `gh` run with JSON output into `{ ok: true, data }` with the parsed
 * JSON, or `{ ok: false, error }`.
 */
const parseGhApi = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status !== 0) {
    return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
  }
  try {
    return { ok: true, data: JSON.parse(stdout) };
  } catch (parseError) {
    return { ok: false, error: `gh printed invalid JSON: ${parseError.message}` };
  }
};

/**
 * Turn the result of a `gh run list` run into `{ ok: true, checks }` or
 * `{ ok: false, error }`. Each workflow run becomes one check.
 */
export const parseGhRuns = (result) => {
  const read = parseGhApi(result);
  if (!read.ok) {
    return read;
  }
  const checks = read.data.map(({ workflowName, status: runStatus, conclusion, url }) => ({
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
export const parseGhMergeable = (result) => {
  const read = parseGhApi(result);
  return read.ok ? { ok: true, mergeable: read.data.mergeable } : read;
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

/** The `headRefOid` of `gh pr view` JSON, or null when it is unreadable. */
const reportedHead = (stdout) => {
  try {
    return JSON.parse(stdout)?.headRefOid ?? null;
  } catch {
    return null;
  }
};

/**
 * True when a successful `gh pr view` result names another head than `head`.
 * A failed or unreadable result is not stale: the rollup parser reports it.
 */
const isStaleHead = ({ status, stdout }, head) => {
  const reported = status === 0 ? reportedHead(stdout) : null;
  return typeof reported === "string" && reported !== head;
};

/** The pending check that keeps the wait going until GitHub shows the push. */
const HEAD_UPDATE_PENDING = {
  ok: true,
  checks: [{ name: "pull request head update", bucket: "pending" }],
};

/** Read PR workflows that can be queued before any check is registered. */
const readPrWorkflows = ({ head, repo }) => {
  const args = [
    "run",
    "list",
    "--commit",
    head,
    "--event",
    "pull_request",
    "--limit",
    String(RUN_LIST_LIMIT),
    "--json",
    "databaseId,workflowDatabaseId,workflowName,status,conclusion,url,headSha,event",
  ];
  if (repo !== null) args.push("--repo", repo);
  const read = parseGhApi(spawnSync("gh", args, { encoding: "utf8" }));
  if (!read.ok) return read;
  if (
    !Array.isArray(read.data) ||
    read.data.some(
      (run) =>
        !Number.isSafeInteger(run?.databaseId) ||
        run.databaseId <= 0 ||
        !Number.isSafeInteger(run.workflowDatabaseId) ||
        run.workflowDatabaseId <= 0 ||
        typeof run.workflowName !== "string" ||
        typeof run.status !== "string" ||
        typeof run.event !== "string" ||
        !FULL_SHA_PATTERN.test(run.headSha) ||
        typeof run.url !== "string" ||
        GITHUB_RUN_URL_PATTERN.exec(run.url)?.[3] !== String(run.databaseId),
    )
  )
    return { ok: false, error: "gh printed invalid pull-request workflow runs" };
  return {
    ok: true,
    runs: read.data.filter((run) => run.headSha === head && run.event === "pull_request"),
  };
};

/** Match required checks across attempts of the same workflow, using immutable IDs. */
const requiredWorkflowSelector = ({ checks, registered, readRun, head }) => {
  let selectedWorkflows = null;
  return (run) => {
    const belongs = (link) =>
      typeof link === "string" && (link === run.url || link.startsWith(`${run.url}/`));
    const known = registered.some((check) => belongs(check.detailsUrl ?? check.targetUrl));
    if (!known || checks.some((check) => belongs(check.link))) return { ok: true, required: true };
    if (selectedWorkflows === null) {
      selectedWorkflows = new Set();
      for (const check of checks) {
        const metadata = readRun(check.link);
        if (!metadata.ok) return metadata;
        if (metadata.run?.head === head) selectedWorkflows.add(metadata.run.workflowId);
      }
    }
    return { ok: true, required: selectedWorkflows.has(run.workflowDatabaseId) };
  };
};

/**
 * Keep unregistered jobs pending without adding completed passes to the check
 * count. A run that waits for approval registers no checks, so it counts in
 * every mode.
 */
const pendingWorkflowChecks = (runs, { names, ...context }) => {
  const selectRequired = requiredWorkflowSelector(context);
  const checks = [];
  for (const run of runs) {
    if (run.status === "completed" && run.conclusion === APPROVAL_CONCLUSION) {
      checks.push({
        name: `${run.workflowName} workflow (${run.databaseId})`,
        bucket: APPROVAL_BUCKET,
        link: run.url,
      });
      continue;
    }
    if (run.status === "completed") continue;
    if (names !== null) {
      const selected = selectRequired(run);
      if (!selected.ok) return selected;
      if (!selected.required) continue;
    }
    checks.push({
      name: `${run.workflowName} workflow (${run.databaseId})`,
      bucket: "pending",
      link: run.url,
    });
  }
  return { ok: true, checks };
};

/** Read current-head checks together with workflows that have not registered checks. */
const readHeadRollup = (args, { head, readRun, names, repo }) => {
  const view = spawnSync("gh", args, { encoding: "utf8" });
  if (head !== null && isStaleHead(view, head)) {
    return HEAD_UPDATE_PENDING;
  }
  const current = parseGhPrRollup(view, { readRun, names });
  if (!current.ok) return current;
  const actualHead = reportedHead(view.stdout);
  if (!FULL_SHA_PATTERN.test(actualHead)) {
    return { ok: false, error: "gh printed an invalid pull-request head" };
  }
  const workflows = readPrWorkflows({ head: actualHead, repo });
  if (!workflows.ok) return workflows;
  const pending = pendingWorkflowChecks(workflows.runs, {
    names,
    checks: current.checks,
    registered: JSON.parse(view.stdout).statusCheckRollup ?? [],
    readRun,
    head: actualHead,
  });
  return pending.ok ? { ok: true, checks: [...current.checks, ...pending.checks] } : pending;
};

/**
 * A repository without required checks makes `gh pr checks --required` report
 * none forever. Then wait for all checks, and say so once.
 */
const allChecksFallback = ({ pr, log }) => {
  let announced = false;
  return (read) => {
    const all = read();
    if (all.ok && all.checks.length > 0 && !announced) {
      announced = true;
      log(`PR ${pr} reports no required checks, so the wait covers all checks.`);
    }
    return all;
  };
};

/** Read current-head states; `gh pr checks --required` selects names only. */
const ghChecksReader = ({ pr, repo, required, head = null, log = console.log }) => {
  const readRun = ghRunMetadataReader();
  const fallback = allChecksFallback({ pr, log });
  return () => {
    const args = ["pr", "view", pr, "--json", "statusCheckRollup,headRefOid"];
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
        return fallback(() => readHeadRollup(args, { head, readRun, names: null, repo }));
      }
    }
    const names = selected === null ? null : new Set(selected.checks.map(({ name }) => name));
    const current = readHeadRollup(args, { head, readRun, names, repo });
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

/** Check `--head`: a full SHA, and only together with `--pr`. */
const validateHead = ({ head, pr }) => {
  if (head === undefined) {
    return;
  }
  if (pr === undefined) {
    throw new Error("--head works only with --pr.");
  }
  if (!FULL_SHA_PATTERN.test(head)) {
    throw new Error(`--head must be a full 40-character SHA, not "${head}".`);
  }
};

/**
 * The --min-checks count, or null when the option is absent. A pull request
 * wait then uses a count of 1 and a stable set (`waitForPullRequestChecks`).
 * A commit wait uses the runs that GitHub created as the expected set.
 */
const minChecksOption = (value) =>
  value === undefined ? null : positiveInteger("--min-checks", value);

const parseOptions = (argv) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      pr: { type: "string" },
      commit: { type: "string" },
      head: { type: "string" },
      repo: { type: "string" },
      "min-checks": { type: "string" },
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
  validateHead(values);
  const pr = values.pr === undefined ? null : String(positiveInteger("--pr", values.pr));
  return {
    help: false,
    pr,
    commit: values.commit ?? null,
    label: pr === null ? commitLabel(values.commit) : `PR ${pr}`,
    repo: values.repo ?? null,
    // Absent stays undefined, and `ghChecksReader` defaults it to null.
    head: values.head,
    required: values.required,
    followMain: values["follow-main"],
    minChecks: minChecksOption(values["min-checks"]),
    intervalMs: positiveInteger("--interval", values.interval) * SECOND,
    timeoutMs: positiveInteger("--timeout", values.timeout) * MINUTE,
    errorWindowMs: positiveInteger("--error-window", values["error-window"]) * SECOND,
  };
};

/** The SHAs of a `gh api .../commits` result, newest first. */
const parseGhCommits = (result) => {
  const read = parseGhApi(result);
  if (!read.ok) {
    return read;
  }
  const commits = read.data;
  if (!Array.isArray(commits) || commits.some((commit) => !FULL_SHA_PATTERN.test(commit?.sha))) {
    return { ok: false, error: "gh printed an invalid commit list" };
  }
  return { ok: true, shas: commits.map(({ sha }) => sha) };
};

/** The completed runs of `name` in a `gh api .../check-runs` result, as checks. */
const parseGhCheckRuns = (result, name) => {
  const read = parseGhApi(result);
  if (!read.ok) {
    return read;
  }
  const runs = read.data?.check_runs;
  if (!Array.isArray(runs)) {
    return { ok: false, error: "gh printed invalid check runs" };
  }
  const checks = runs
    .filter((run) => run?.name === name && run.status === "completed")
    .map(({ conclusion, html_url: link }) => ({
      name,
      bucket: RUN_BUCKETS.get(conclusion) ?? "fail",
      link,
    }));
  return { ok: true, checks };
};

const ghApi = (endpoint) => spawnSync("gh", ["api", endpoint], { encoding: "utf8" });

/**
 * The commits before `commit`, newest first, from a `gh api .../commits`
 * result that starts at `commit`.
 */
const parseEarlierCommits = (result, commit) => {
  const read = parseGhCommits(result);
  if (!read.ok) {
    return read;
  }
  if (read.shas[0] !== commit) {
    return { ok: false, error: `the history of ${shortSha(commit)} does not start at it` };
  }
  return { ok: true, shas: read.shas.slice(1) };
};

/** The completed workflow runs of `name` on `sha`, as checks. */
const completedRuns = (sha, name, readRuns) => {
  const read = readRuns(sha);
  if (!read.ok) {
    return read;
  }
  return {
    ok: true,
    checks: read.checks.filter((check) => check.name === name && check.bucket !== "pending"),
  };
};

/**
 * The readers of main for `compareWithMain`. With --pr, the comparison reads
 * the job check runs of the newest commits of main. With --commit, it reads
 * the workflow runs of the commits before the given commit, because a commit
 * wait reports workflow runs. So a failure that main had before the merge
 * does not count against the merge. Without --repo, gh fills in the
 * `{owner}/{repo}` placeholder from the repository of the working directory.
 */
export const ghMainReaders = (
  { repo = null, commit = null },
  api = ghApi,
  readRuns = (sha) => ghRunsReader({ commit: sha, repo })(),
) => {
  const base = `repos/${repo ?? "{owner}/{repo}"}`;
  if (commit !== null) {
    return {
      readMainCommits: () =>
        parseEarlierCommits(
          api(`${base}/commits?sha=${commit}&per_page=${MAIN_COMMIT_LIMIT + 1}`),
          commit,
        ),
      readMainChecks: (sha, name) => completedRuns(sha, name, readRuns),
    };
  }
  return {
    readMainCommits: () =>
      parseGhCommits(api(`${base}/commits?sha=${FOLLOW_BRANCH}&per_page=${MAIN_COMMIT_LIMIT}`)),
    readMainChecks: (sha, name) =>
      parseGhCheckRuns(
        api(
          `${base}/commits/${sha}/check-runs?check_name=${encodeURIComponent(name)}&filter=latest&per_page=100`,
        ),
        name,
      ),
  };
};

/**
 * The newest of `shas` with a completed run of `name`, when all its completed
 * runs of `name` fail, else null. A cancelled run does not count, because a
 * newer push to main cancels the runs of the older commits. Returns
 * `{ ok: true, sha }` or `{ ok: false, error }`.
 */
const mainFailure = (name, shas, readMainChecks) => {
  for (const sha of shas) {
    const read = readMainChecks(sha, name);
    if (!read.ok) {
      return read;
    }
    const finished = read.checks.filter(({ bucket }) => bucket !== "cancel");
    if (finished.length > 0) {
      return { ok: true, sha: finished.every(({ bucket }) => bucket === "fail") ? sha : null };
    }
  }
  return { ok: true, sha: null };
};

/**
 * Compare the failed checks of a failed pull request or commit wait with main.
 *
 * `readMainCommits()` returns `{ ok: true, shas }`, newest first.
 * `readMainChecks(sha, name)` returns the completed runs of `name` on `sha`
 * as `{ ok: true, checks }`. A check with the bucket `fail` gets `main` (a
 * SHA) when the same check fails on main. The result has the status
 * `shared-with-main` when every failed or cancelled check has `main`. A
 * failed read keeps the result and adds `mainError`.
 */
export const compareWithMain = (result, { readMainCommits, readMainChecks }) => {
  if (result.status !== "fail") {
    return result;
  }
  const commits = readMainCommits();
  if (!commits.ok) {
    return { ...result, mainError: commits.error };
  }
  const checks = [];
  for (const check of result.checks) {
    const failure =
      check.bucket === "fail"
        ? mainFailure(check.name, commits.shas, readMainChecks)
        : { ok: true, sha: null };
    if (!failure.ok) {
      return { ...result, mainError: failure.error };
    }
    checks.push(failure.sha === null ? check : { ...check, main: failure.sha });
  }
  const shared = checks
    .filter(({ bucket }) => FAILED_BUCKETS.has(bucket))
    .every(({ main }) => main !== undefined);
  return { ...result, checks, status: shared ? "shared-with-main" : "fail" };
};

/** The headline for runs that wait for maintainer approval. */
const approvalHeadline = (label, checks, summary) => {
  const count = checks.filter(({ bucket }) => bucket === APPROVAL_BUCKET).length;
  const runs = count === 1 ? "1 workflow run waits" : `${count} workflow runs wait`;
  return `${label}: ${runs} for maintainer approval, so GitHub does not start the jobs (${summary}). Read the code of the pull request, approve each run with gh api -X POST repos/<owner>/<repo>/actions/runs/<run id>/approve, and wait again.`;
};

const HEAD_WARNING =
  "Warning: --pr without --head. Right after a push, GitHub can still report the checks of the old head. Give --head <full sha> of the pushed tip.";

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
  "shared-with-main": 4,
  approval: 5,
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
    mainError = null,
  },
  { label, minChecks, settleMs = SETTLE_MS, commit = null, followMain: following = false },
  log = console.log,
  warn = console.error,
) => {
  const summary = checks.length === 0 ? "no checks" : bucketSummary(checks);
  const settleMinutes = settleMs / MINUTE;
  const headline = {
    pass: `${label}: all checks passed (${summary}).`,
    settled:
      commit === null && !checks.some(({ bucket }) => bucket === PASS_BUCKET)
        ? `${label}: no check passed, and no check started or changed for ${settleMinutes} minutes (${summary}). The CI workflow can still be missing.`
        : minChecks === null
          ? `${label}: all ${checks.length} runs passed, and no run started or changed for ${settleMinutes} minutes (${summary}).`
          : `${label}: all ${checks.length} checks passed, fewer than --min-checks ${minChecks}, and no check started or changed for ${settleMinutes} minutes (${summary}).`,
    fail: `${label}: checks failed (${summary}).`,
    "shared-with-main":
      commit === null
        ? `${label}: checks failed, and each failed check also fails on ${FOLLOW_BRANCH} (${summary}). Wait for the fix on ${FOLLOW_BRANCH}, merge ${FOLLOW_BRANCH} into the branch, push, and wait again.`
        : `${label}: checks failed, and each failed check also failed on an earlier commit of ${FOLLOW_BRANCH} (${summary}). The failure does not come from this commit.`,
    timeout:
      minChecks === null
        ? `${label}: timed out with ${checks.length} checks (${summary}).`
        : `${label}: timed out with ${checks.length} of at least ${minChecks} checks (${summary}).`,
    error: `${label}: stopped after ${readErrors} failed reads of the checks in ${formatSeconds(errorMs)}.`,
    hops: `${label}: runs cancelled again after ${maxHops} moves to a newer commit (${summary}).`,
    "head-error": `${label}: read of ${FOLLOW_REF} failed: ${error}`,
    diverged: `${label}: ${FOLLOW_REF} at ${shortSha(head)} does not contain commit ${shortSha(origin)}, so the wait stops.`,
    conflict: `${label}: the pull request conflicts with its base branch, so GitHub starts no pull_request workflows (${summary}). Update the branch, push, and wait again.`,
    approval: approvalHeadline(label, checks, summary),
  }[status];
  log(headline);
  for (const check of checks.filter(
    ({ bucket }) => FAILED_BUCKETS.has(bucket) || bucket === APPROVAL_BUCKET,
  )) {
    const also =
      check.main === undefined ? "" : ` (also on ${FOLLOW_BRANCH} ${shortSha(check.main)})`;
    log(`  ${check.bucket.toUpperCase()}${also}: ${check.name} ${check.link ?? ""}`.trimEnd());
  }
  if (mainError !== null) {
    warn(
      `Read of the ${FOLLOW_BRANCH} checks failed, so no check counts as shared with ${FOLLOW_BRANCH}: ${mainError}`,
    );
  }
  if (!following && commit !== null && checks.some(({ bucket }) => bucket === "cancel")) {
    log(CANCELLED_COMMIT_HINT);
  }
  return EXIT_CODES[status];
};

const SKILLS_PATH_PREFIX = `${RELEASED_SKILLS_ROOT.join("/")}/`;
const SKILLS_SYNC_COMMAND = "node scripts/sync-skills-companion.mjs --pin";
const SKILLS_PUBLISH_STEP = "and publish its commit with the commands that it prints.";
const SKILLS_CONTRACT_NOTE =
  'The "Public skills contract" job fails on main until the sync lands. On a pull request the job checks the base commit, so a green pull request does not prove the sync.';
const CHANGED_PATHS = ".filename, .previous_filename // empty";

/**
 * Read changed paths from a paginated REST endpoint. `gh pr view --json files`
 * stops at 100 files. A rename counts for its old path too, because the old
 * file stays in the companion until the sync.
 */
const ghChangedPathsReader = (repo, resource, items) => () => {
  const { error, status, stdout, stderr } = spawnSync(
    "gh",
    [
      "api",
      `repos/${repo ?? "{owner}/{repo}"}/${resource}`,
      "--paginate",
      "--jq",
      `${items} | ${CHANGED_PATHS}`,
    ],
    { encoding: "utf8" },
  );
  if (error) {
    return { ok: false, error: error.message };
  }
  if (status !== 0) {
    return { ok: false, error: stderr.trim() || `gh exited with ${status}` };
  }
  return { ok: true, paths: stdout.split("\n").filter(Boolean) };
};

/**
 * Print the companion sync step when the change touches the released skills.
 * A failed read only warns, so it never changes the exit code of the wait.
 */
const reportSkillsSync = (read, { label, step }, log = console.log, warn = console.error) => {
  if (!read.ok) {
    warn(`${label}: read of the changed files failed: ${read.error}`);
    warn(`When the change touches ${SKILLS_PATH_PREFIX}: ${step}`);
    return;
  }
  if (read.paths.some((path) => path.startsWith(SKILLS_PATH_PREFIX))) {
    log(`${label}: the change touches ${SKILLS_PATH_PREFIX}. ${step}`);
    log(SKILLS_CONTRACT_NOTE);
  }
};

const reportPrSkillsSync = ({ pr, repo, label }) =>
  reportSkillsSync(ghChangedPathsReader(repo, `pulls/${pr}/files`, ".[]")(), {
    label,
    step: `After the merge, run ${SKILLS_SYNC_COMMAND} <merge commit> ${SKILLS_PUBLISH_STEP}`,
  });

// The merged commit needs the step on any result: the missing sync is itself
// a reason for a failed contract job on main.
const reportCommitSkillsSync = ({ commit, repo }) =>
  reportSkillsSync(ghChangedPathsReader(repo, `commits/${commit}`, ".files[]")(), {
    label: commitLabel(commit),
    step: `Run ${SKILLS_SYNC_COMMAND} ${commit} ${SKILLS_PUBLISH_STEP}`,
  });

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
        waitForCommitRuns({
          ...options,
          timeoutMs,
          readChecks: ghRunsReader({ commit: sha, repo: options.repo }),
        }),
      readHead: gitHeadReader(runGit),
      isAncestor: gitAncestorChecker(runGit),
    });
    const code = reportChecks(compareWithMain(result, ghMainReaders(options)), {
      ...options,
      label: commitLabel(result.commit),
      settleMs: COMMIT_SETTLE_MS,
    });
    reportCommitSkillsSync(options);
    return code;
  }
  if (options.commit !== null) {
    const result = await waitForCommitRuns({ ...options, readChecks: ghRunsReader(options) });
    const code = reportChecks(compareWithMain(result, ghMainReaders(options)), {
      ...options,
      settleMs: COMMIT_SETTLE_MS,
    });
    reportCommitSkillsSync(options);
    return code;
  }
  if (options.head === undefined) {
    console.error(HEAD_WARNING);
  }
  const result = await waitForPullRequestChecks({
    ...options,
    readChecks: ghChecksReader(options),
    readMergeable: ghMergeableReader(options),
  });
  const code = reportChecks(compareWithMain(result, ghMainReaders(options)), options);
  if (code === 0) {
    reportPrSkillsSync(options);
  }
  return code;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main();
}
