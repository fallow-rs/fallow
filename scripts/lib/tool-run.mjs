import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const TIMEOUT_CODE = "ETIMEDOUT";
const OVERFLOW_CODE = "ENOBUFS";
const BYTE_ORDER_MARK = String.fromCodePoint(0xfeff);
const PATH_SEPARATOR = /[\\/]/;

const text = (value) => (value === undefined || value === null ? "" : value.toString());

const failureReason = ({ timedOut, overflow, spawnError, signal, status }) => {
  if (timedOut) return "timed out";
  if (overflow) return `output exceeded maxBuffer: ${spawnError}`;
  if (spawnError) return `failed to start: ${spawnError}`;
  if (signal) return `stopped by ${signal}`;
  return `exited with status ${status}`;
};

/**
 * Classifies the result of `spawnSync`. `okStatuses` lists the exit codes
 * that mean "the tool ran": for a tool that exits 1 on findings, pass [0, 1].
 * `stdout` and `stderr` are always strings. `reason` is null when `ok` is
 * true, else one short line.
 */
export const classifyRun = (proc, { okStatuses = [0] } = {}) => {
  const timedOut = proc.error?.code === TIMEOUT_CODE;
  const spawnError = proc.error && !timedOut ? (proc.error.message ?? String(proc.error)) : null;
  const signal = proc.signal ?? null;
  const status = proc.status ?? null;
  const ok = !proc.error && signal === null && status !== null && okStatuses.includes(status);
  return {
    ok,
    status,
    signal,
    timedOut,
    spawnError,
    stdout: text(proc.stdout),
    stderr: text(proc.stderr),
    reason: ok
      ? null
      : failureReason({
          timedOut,
          overflow: proc.error?.code === OVERFLOW_CODE,
          spawnError,
          signal,
          status,
        }),
  };
};

/** Runs a command with `spawnSync` (UTF-8 output) and classifies the result. */
export const runTool = (cmd, args, { okStatuses, ...spawnOptions } = {}) =>
  classifyRun(spawnSync(cmd, args, { encoding: "utf8", ...spawnOptions }), { okStatuses });

/**
 * Parses the JSON report of a tool. Returns `{ ok: true, value }` or
 * `{ ok: false, error }`. Empty output is an error, not an empty object.
 */
export const parseJsonReport = (stdout) => {
  const trimmed = (stdout.startsWith(BYTE_ORDER_MARK) ? stdout.slice(1) : stdout).trim();
  if (!trimmed) return { ok: false, error: "no JSON output" };
  try {
    const value = JSON.parse(trimmed);
    if (value === null || typeof value !== "object") {
      return { ok: false, error: "unexpected JSON shape" };
    }
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: `invalid JSON output (${String(error.message).split("\n")[0]})` };
  }
};

/**
 * Returns the first candidate that answers `--version` with exit code 0.
 * A candidate with a path separator comes back as an absolute path. Empty
 * candidates are skipped. Throws when no candidate passes.
 */
export const resolveFallowBin = (candidates) => {
  for (const candidate of candidates.filter(Boolean)) {
    if (runTool(candidate, ["--version"]).ok) {
      return PATH_SEPARATOR.test(candidate) ? resolve(candidate) : candidate;
    }
  }
  throw new Error("fallow binary not found. Build fallow or pass --fallow-bin PATH");
};
