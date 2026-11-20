import { spawnSync } from "node:child_process";
import { closeSync, openSync, readFileSync } from "node:fs";

import { classifyRun } from "./tool-run.mjs";

const TIME_BIN = "/usr/bin/time";
const MAX_BUFFER = 50 * 1024 * 1024;
const BENCH_ENV = { NO_COLOR: "1", FORCE_COLOR: "0" };
const LINUX_PEAK_RSS_KB = /Maximum resident set size \(kbytes\): (\d+)/;
const MACOS_PEAK_RSS_BYTES = /(\d+)\s+maximum resident set size/;
const MS_PER_SECOND = 1000;

/** The min, max, mean and median of a list of times that is not empty. */
export const stats = (times) => {
  const sorted = [...times].toSorted((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
  return {
    min: sorted[0],
    max: sorted.at(-1),
    mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    median,
  };
};

/**
 * The outcome of one measured sample. `run` is a `classifyRun` result.
 * `report` is `{ ok: true, value }` or `{ ok: false, error }` for the report
 * of the tool. Only a valid sample can enter the times.
 */
export const sampleOutcome = (run, report) => {
  if (!run.ok) return { valid: false, reason: run.reason };
  if (!report.ok) return { valid: false, reason: report.error };
  return { valid: true, reason: null };
};

/**
 * Compares the successful samples with the requested count. `stats` is null
 * when no sample was successful. A speedup needs `complete` to be true.
 */
export const sampleSummary = (times, requested) => ({
  requested,
  successful: times.length,
  complete: requested > 0 && times.length === requested,
  stats: times.length > 0 ? stats(times) : null,
});

const peakRss = (stderr) => {
  const linux = stderr.match(LINUX_PEAK_RSS_KB);
  if (linux) return Number(linux[1]) * 1024;
  const macos = stderr.match(MACOS_PEAK_RSS_BYTES);
  return macos ? Number(macos[1]) : 0;
};

const timedCommand = (cmd, args, memory) => {
  if (!memory) return [cmd, args];
  return [TIME_BIN, [process.platform === "linux" ? "-v" : "-l", cmd, ...args]];
};

/**
 * Runs one benchmark sample and returns a `classifyRun` result (exit codes 0
 * and 1 are ok) plus `elapsed` in ms and `peakRssBytes`. With `memory: true`
 * the command runs under `/usr/bin/time` and `peakRssBytes` is its peak RSS,
 * else 0. With `stdoutFile`, stdout goes to that regular file and is read
 * back: a tool that calls `process.exit` before a pipe drains loses output.
 */
export const timeSample = (cmd, args, { cwd, timeout, memory = false, stdoutFile = null }) => {
  const [bin, binArgs] = timedCommand(cmd, args, memory);
  const stdoutFd = stdoutFile ? openSync(stdoutFile, "w") : null;
  const start = performance.now();
  let result;
  try {
    result = spawnSync(bin, binArgs, {
      cwd,
      stdio: ["pipe", stdoutFd ?? "pipe", "pipe"],
      timeout,
      maxBuffer: MAX_BUFFER,
      env: { ...process.env, ...BENCH_ENV },
    });
  } finally {
    if (stdoutFd !== null) closeSync(stdoutFd);
  }
  const elapsed = performance.now() - start;
  const run = classifyRun(result, { okStatuses: [0, 1] });
  const stdout = stdoutFile ? readFileSync(stdoutFile, "utf8") : run.stdout;
  return { ...run, stdout, elapsed, peakRssBytes: memory ? peakRss(run.stderr) : 0 };
};

/** A duration in ms as `850ms` or `1.25s`. */
export const formatMs = (ms) =>
  ms < MS_PER_SECOND ? `${ms.toFixed(0)}ms` : `${(ms / MS_PER_SECOND).toFixed(2)}s`;

/** The Min, Mean, Median and Max table cells of `stats`; `--` for null. */
export const timeCells = (times) =>
  times
    ? {
        Min: formatMs(times.min),
        Mean: formatMs(times.mean),
        Median: formatMs(times.median),
        Max: formatMs(times.max),
      }
    : { Min: "--", Mean: "--", Median: "--", Max: "--" };
