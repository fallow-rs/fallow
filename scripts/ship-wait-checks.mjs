#!/usr/bin/env node

// Wait until the checks of a pull request are complete. Right after a push,
// GitHub can report no checks or only the first few, and "nothing pending"
// is then true too early. The --min-checks guard waits until at least that
// count of checks exists.

import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { setTimeout as sleepMs } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const GH_PENDING_EXIT = 8;
// gh prints "no required checks reported" for `--required` when no required
// check exists yet.
const NO_CHECKS_PATTERN = /no (?:required )?checks reported/iu;
const MAX_READ_ERRORS = 3;
const FAILED_BUCKETS = new Set(["fail", "cancel"]);
const SECOND = 1000;
const MINUTE = 60 * SECOND;

const USAGE = `Usage: node scripts/ship-wait-checks.mjs --pr <number> [options]

Wait until a pull request has at least --min-checks checks and none of them
is pending. Then print a summary of the checks.

Options:
  --pr <number>           The pull request.
  --repo <owner/name>     The repository (default: the repository of the
                          working directory).
  --min-checks <count>    The count of checks that must exist (default: 1).
                          Use the check count of a recent complete run.
  --required              Wait for the required checks only.
  --interval <seconds>    The time between two reads (default: 30).
  --timeout <minutes>     The maximum wait (default: 60).
  -h, --help              Show this help.

Exit codes: 0 when all checks pass or skip, 1 when a check fails or is
cancelled, 2 for invalid input, ${MAX_READ_ERRORS} failed reads in a row or a
timeout.`;

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

/**
 * Read the checks until at least `minChecks` exist and none is pending.
 *
 * `readChecks()` returns `{ ok: true, checks }` or `{ ok: false, error }`.
 * Returns `{ status, checks }` where `status` is `pass`, `fail`, `timeout`
 * or `error`. The loop stops after `MAX_READ_ERRORS` read errors in a row.
 */
export const waitForChecks = async ({
  readChecks,
  minChecks,
  intervalMs,
  timeoutMs,
  sleep = sleepMs,
  now = Date.now,
  warn = console.error,
}) => {
  const deadline = now() + timeoutMs;
  let checks = [];
  let readErrors = 0;
  for (;;) {
    const read = readChecks();
    if (read.ok) {
      readErrors = 0;
      checks = read.checks;
      const pending = checks.some(({ bucket }) => bucket === "pending");
      if (checks.length >= minChecks && !pending) {
        const failed = checks.some(({ bucket }) => FAILED_BUCKETS.has(bucket));
        return { status: failed ? "fail" : "pass", checks };
      }
    } else {
      readErrors += 1;
      warn(`Read of the checks failed: ${read.error}`);
      if (readErrors >= MAX_READ_ERRORS) {
        return { status: "error", checks };
      }
    }
    if (now() + intervalMs > deadline) {
      return { status: "timeout", checks };
    }
    await sleep(intervalMs);
  }
};

/**
 * Turn the result of a `gh pr checks` run (`{ error, status, stdout,
 * stderr }`, as `spawnSync` returns it) into `{ ok: true, checks }` or
 * `{ ok: false, error }`.
 */
export const parseGhChecks = ({ error, status, stdout, stderr }) => {
  if (error) {
    return { ok: false, error: error.message };
  }
  // gh exits with 8 while a check is pending. The JSON is still complete.
  if (status === 0 || status === GH_PENDING_EXIT) {
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

/** Read the checks of `pr` with `gh pr checks`. */
const ghChecksReader =
  ({ pr, repo, required }) =>
  () => {
    const args = ["pr", "checks", pr, "--json", "name,bucket,link"];
    if (repo !== null) {
      args.push("--repo", repo);
    }
    if (required) {
      args.push("--required");
    }
    return parseGhChecks(spawnSync("gh", args, { encoding: "utf8" }));
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
      repo: { type: "string" },
      "min-checks": { type: "string", default: "1" },
      required: { type: "boolean", default: false },
      interval: { type: "string", default: "30" },
      timeout: { type: "string", default: "60" },
      help: { type: "boolean", short: "h", default: false },
    },
    strict: true,
  });
  if (values.help) {
    return { help: true };
  }
  if (values.pr === undefined) {
    throw new Error("--pr is required.");
  }
  return {
    help: false,
    pr: String(positiveInteger("--pr", values.pr)),
    repo: values.repo ?? null,
    required: values.required,
    minChecks: positiveInteger("--min-checks", values["min-checks"]),
    intervalMs: positiveInteger("--interval", values.interval) * SECOND,
    timeoutMs: positiveInteger("--timeout", values.timeout) * MINUTE,
  };
};

const EXIT_CODES = { pass: 0, fail: 1, timeout: 2, error: 2 };

/** Print the result of `waitForChecks` and return the exit code. */
export const reportChecks = ({ status, checks }, { pr, minChecks }, log = console.log) => {
  const summary = checks.length === 0 ? "no checks" : bucketSummary(checks);
  const headline = {
    pass: `PR ${pr}: all checks passed (${summary}).`,
    fail: `PR ${pr}: checks failed (${summary}).`,
    timeout: `PR ${pr}: timed out with ${checks.length} of at least ${minChecks} checks (${summary}).`,
    error: `PR ${pr}: stopped after ${MAX_READ_ERRORS} failed reads of the checks.`,
  }[status];
  log(headline);
  for (const check of checks.filter(({ bucket }) => FAILED_BUCKETS.has(bucket))) {
    log(`  ${check.bucket.toUpperCase()}: ${check.name} ${check.link ?? ""}`.trimEnd());
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
  const result = await waitForChecks({ ...options, readChecks: ghChecksReader(options) });
  return reportChecks(result, options);
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main();
}
