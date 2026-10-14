#!/usr/bin/env node
// Compares the JSON reports of repeated `fallow mutation` runs on one project
// and scope. The result is the flaky rate: the part of the executed mutants
// that did not get the same status in each run. See docs/mutation-testing.md.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const EXECUTED = new Set(["killed", "timeout", "survived", "flaky", "runtime_error"]);
const DETECTED = new Set(["killed", "timeout"]);
const MIN_RUNS = 2;

/** The status of each executed mutant of one report. */
const executedStatuses = (report) =>
  new Map(
    (report.mutants ?? [])
      .filter((mutant) => EXECUTED.has(mutant.status))
      .map((mutant) => [mutant.mutant_id, mutant.status]),
  );

/** The ids that each run executed, sorted. */
const comparedIds = (runs) => {
  const [first, ...rest] = runs;
  return [...first.keys()].filter((id) => rest.every((run) => run.has(id))).toSorted();
};

const isStable = (distinct) => distinct.size === 1 && !distinct.has("flaky");

/**
 * The entry of one compared mutant, or null when it is stable.
 * @param {string} id
 * @param {string[]} statuses
 */
const unstableEntry = (id, statuses) => {
  const distinct = new Set(statuses);
  if (isStable(distinct)) return null;
  const scoreFlip = statuses.some((status) => DETECTED.has(status)) && distinct.has("survived");
  return {
    mutant_id: id,
    statuses,
    score_flip: scoreFlip,
    timeout_flip: scoreFlip && distinct.has("timeout"),
    runtime_error_flip: distinct.has("runtime_error"),
  };
};

/**
 * Compares the reports of repeated runs.
 *
 * A mutant is compared when each run executed it. A compared mutant is
 * unstable when its status is not the same in each run, or when one run gave
 * it `flaky`. A score flip is an unstable mutant that one run detected
 * (`killed` or `timeout`) and another run gave `survived`. A timeout flip is a
 * score flip with `timeout` in one run: a retry of timed-out mutants removes
 * only this kind of flip. A runtime-error flip is an unstable mutant with
 * `runtime_error` in one run: the companion failed, not the tests.
 * @param {Array<{ mutants?: Array<{ mutant_id: string, status: string }> }>} reports
 */
export const compareRuns = (reports) => {
  if (reports.length < MIN_RUNS) {
    throw new Error(`compare at least ${MIN_RUNS} reports, got ${reports.length}`);
  }
  const runs = reports.map(executedStatuses);
  const compared = comparedIds(runs);
  const unstable = compared
    .map((id) =>
      unstableEntry(
        id,
        runs.map((run) => run.get(id)),
      ),
    )
    .filter((entry) => entry !== null);
  const count = (key) => unstable.filter((entry) => entry[key]).length;
  const rate = (value) => (compared.length === 0 ? null : value / compared.length);
  const scoreFlips = count("score_flip");
  const timeoutFlips = count("timeout_flip");
  const runtimeErrorFlips = count("runtime_error_flip");
  return {
    runs: reports.length,
    counts: reports.map((report) => report.summary?.counts ?? null),
    compared: compared.length,
    unstable: unstable.length,
    unstable_rate: rate(unstable.length),
    score_flips: scoreFlips,
    score_flip_rate: rate(scoreFlips),
    timeout_flips: timeoutFlips,
    timeout_flip_rate: rate(timeoutFlips),
    runtime_error_flips: runtimeErrorFlips,
    runtime_error_flip_rate: rate(runtimeErrorFlips),
    mutants: unstable,
  };
};

const main = (paths) => {
  const reports = paths.map((path) => JSON.parse(readFileSync(path, "utf8")));
  process.stdout.write(`${JSON.stringify(compareRuns(reports), null, 2)}\n`);
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
