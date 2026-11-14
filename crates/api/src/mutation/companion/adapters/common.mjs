// Generated from tools/mutation-runner/src/adapters/common.mjs. Do not edit.
import { isAbsolute, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import { RUNNERS } from "../generated-protocol.mjs";
import { CompanionError, expectRequest, isPlainObject } from "../protocol.mjs";
import { escapeRegExp, realPath } from "../runtime/child.mjs";
import { RECORD_LIMIT, decodeValue } from "../runtime/replay-value.mjs";

export { escapeRegExp, realPath };

/** The separator between the parts of a test id. */
export const TEST_ID_SEPARATOR = "::";

export const toPosix = (path) => path.split(sep).join("/");

/** Compares two strings by UTF-16 code unit, independent of the host locale. */
export const byCodeUnit = (left, right) => (left < right ? -1 : left > right ? 1 : 0);

const isInside = (parent, child) => {
  const rel = relative(parent, child);
  return rel !== "" && !rel.startsWith("..") && !isAbsolute(rel);
};

const isAbsolutePath = (value) => typeof value === "string" && isAbsolute(value);

const isStringList = (value) =>
  Array.isArray(value) && value.every((entry) => typeof entry === "string");

const isNonEmptyStringList = (value) => isStringList(value) && value.length > 0;

const isStringMap = (value) =>
  isPlainObject(value) && Object.values(value).every((entry) => typeof entry === "string");

const isOptional = (value, check) => value === undefined || check(value);

const isOptionalPath = (value) =>
  value === undefined || value === null || typeof value === "string";

const isPositiveInteger = (value) => Number.isSafeInteger(value) && value > 0;

export const parseVersion = (version) => {
  const match = /^v?(\d+)\.(\d+)\.(\d+)/u.exec(version);
  return match ? match.slice(1, 4).map(Number) : null;
};

const compareVersions = (left, right) => {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] - right[index];
  }
  return 0;
};

/** Gives the manifest range of the runner `name` as `>=<min> <<max>`. */
const supportedRange = (name) => {
  const runner = RUNNERS.find((entry) => entry.name === name);
  return runner === undefined ? "none" : `>=${runner.min_version} <${runner.max_version_exclusive}`;
};

/** Gives the `runner-version` error for `label`, with the supported range of `name`. */
export const unsupportedVersion = (name, label) =>
  new CompanionError(
    "runner-version",
    `${label} is not supported; the supported range is ${supportedRange(name)}`,
  );

/** True when `version` is in the manifest range of the runner `name`. */
export const isSupportedVersion = (name, version) => {
  const runner = RUNNERS.find((entry) => entry.name === name);
  const parsed = parseVersion(version);
  return (
    runner !== undefined &&
    parsed !== null &&
    compareVersions(parsed, parseVersion(runner.min_version)) >= 0 &&
    compareVersions(parsed, parseVersion(runner.max_version_exclusive)) < 0
  );
};

export const validateInit = (init) => {
  expectRequest(isAbsolutePath(init.root), "root must be absolute");
  expectRequest(isAbsolutePath(init.run_dir), "run_dir must be absolute");
  expectRequest(isOptionalPath(init.config_path), "config_path must be a string or null");
  expectRequest(isPlainObject(init.path_map), "path_map must be an object");
  const options = init.options ?? {};
  expectRequest(
    isOptional(options.test_files, isStringList),
    "options.test_files must be a list of paths",
  );
  expectRequest(
    isOptional(options.extra_test_files, isStringList),
    "options.extra_test_files must be a list of paths",
  );
  expectRequest(isOptional(options.env, isStringMap), "options.env must map names to strings");
  expectRequest(
    isOptional(options.node_args, isStringList),
    "options.node_args must be a list of strings",
  );
};

/**
 * The factor on the project test timeout in a dry run. The dry run executes
 * the instrumented code, which is slower than the original code. A test close
 * to its timeout must not become a baseline failure for that reason. A mutant
 * run sets its own timeout from the dry-run durations.
 */
const DRY_RUN_TIMEOUT_FACTOR = 3;

/** True for a timeout that sets a limit. Vitest reads 0 as no limit. */
export const hasTimeoutLimit = (timeoutMs) => Number.isFinite(timeoutMs) && timeoutMs > 0;

/** Gives the dry-run value of a timeout. A value that sets no limit stays as it is. */
export const dryRunTimeout = (timeoutMs) =>
  hasTimeoutLimit(timeoutMs) ? timeoutMs * DRY_RUN_TIMEOUT_FACTOR : timeoutMs;

export const validateDryRun = (request) => {
  expectRequest(isOptional(request.test_files, isStringList), "test_files must be a list of paths");
};

/** Checks a `record` request: the optional test files, as for a dry run. */
export const validateRecord = validateDryRun;

/**
 * Adds the argument records of one test or one file to `records`, a map
 * from a function key to its distinct inputs and its counts of reasons. A
 * function keeps at most `RECORD_LIMIT` inputs, in first record order.
 */
export const addRecords = (records, entries = []) => {
  for (const { key, inputs, not_replayable: refused } of entries) {
    let entry = records.get(key);
    if (entry === undefined) {
      entry = { inputs: new Map(), refused: new Map() };
      records.set(key, entry);
    }
    for (const input of inputs) {
      const text = JSON.stringify(input);
      if (entry.inputs.size < RECORD_LIMIT && !entry.inputs.has(text))
        entry.inputs.set(text, input);
    }
    for (const { reason, count } of refused) {
      entry.refused.set(reason, (entry.refused.get(reason) ?? 0) + count);
    }
  }
};

/**
 * Checks a `run` request. A `mutant_id` of null is a control run: the tests
 * run with no active mutant, through the same path as a mutant run.
 */
export const validateRun = (request) => {
  expectRequest(
    request.mutant_id === null ||
      (typeof request.mutant_id === "string" && request.mutant_id.length > 0),
    "mutant_id must be a string or null",
  );
  expectRequest(isNonEmptyStringList(request.test_ids), "test_ids must be a non-empty list");
  expectRequest(isPositiveInteger(request.timeout_ms), "timeout_ms must be a positive integer");
  expectRequest(typeof request.bail === "boolean", "bail must be a boolean");
  expectRequest(
    request.loop_budget_ms === undefined || isPositiveInteger(request.loop_budget_ms),
    "loop_budget_ms must be a positive integer when present",
  );
};

/** The most calls of each side that one replay request can ask for. */
const MAX_REPLAY_CALLS = 10;
/** The longest call limit: 10 minutes, well inside the range of a timer. */
const MAX_REPLAY_CALL_TIMEOUT_MS = 600_000;

/**
 * Checks the body of a `replay` request: one export of one source file,
 * called with `args` on the original and on `mutant_id` (replay contract 1).
 */
export const validateReplay = (request) => {
  for (const field of ["file", "export", "mutant_id"]) {
    expectRequest(
      typeof request[field] === "string" && request[field].length > 0,
      `${field} must be a non-empty string`,
    );
  }
  expectRequest(Array.isArray(request.args), "args must be a list");
  try {
    decodeValue(request.args);
  } catch (error) {
    expectRequest(false, `args do not decode: ${error.message}`);
  }
  expectRequest(
    isPositiveInteger(request.calls) && request.calls <= MAX_REPLAY_CALLS,
    `calls must be an integer from 1 to ${MAX_REPLAY_CALLS}`,
  );
  expectRequest(
    isPositiveInteger(request.call_timeout_ms) &&
      request.call_timeout_ms <= MAX_REPLAY_CALL_TIMEOUT_MS,
    `call_timeout_ms must be an integer from 1 to ${MAX_REPLAY_CALL_TIMEOUT_MS}`,
  );
  expectRequest(Number.isSafeInteger(request.clock_ms), "clock_ms must be an integer");
  expectRequest(Number.isSafeInteger(request.seed), "seed must be an integer");
};

/** The overlay entry of the replay file. The file must be in the path map. */
export const replayEntry = (overlay, file) => {
  const entry = [...overlay.values()].find((candidate) => candidate.source === file);
  expectRequest(entry !== undefined, `the replay file ${file} is not in the path_map`);
  return entry;
};

/**
 * Maps the real path of each project source file to its instrumented copy in
 * the run directory, as `{ source, target }`. The comparison uses real paths,
 * so a symlink cannot select a file outside the map.
 */
export const buildOverlay = (root, runDir, pathMap) => {
  const realRunDir = realPath(runDir);
  const overlay = new Map();
  for (const [source, target] of Object.entries(pathMap)) {
    expectRequest(typeof target === "string", `path_map value for ${source} must be a string`);
    const sourcePath = resolve(root, source);
    const targetPath = realPath(resolve(runDir, target));
    expectRequest(isInside(root, sourcePath), `path_map key ${source} is outside root`);
    expectRequest(isInside(realRunDir, targetPath), `path_map value ${target} is outside run_dir`);
    overlay.set(realPath(sourcePath), { source, path: sourcePath, target: targetPath });
  }
  return overlay;
};

/**
 * Names each test. A repeated full name gets a ` (n)` suffix. A lone UTF-16
 * surrogate becomes U+FFFD before the suffix, because the protocol carries
 * only well-formed text. A run request then sends back the same name.
 */
export const uniqueNames = (fullNames) => {
  const seen = new Map();
  return fullNames.map((rawName) => {
    const fullName = rawName.toWellFormed();
    const count = (seen.get(fullName) ?? 0) + 1;
    seen.set(fullName, count);
    return count === 1 ? fullName : `${fullName} (${count})`;
  });
};

/** True for a test that ran to a result. A skipped or cancelled test did not run. */
export const hasRun = (status) => status === "pass" || status === "fail";

/**
 * Flags each test that ran and whose full name another test that ran repeats.
 * `uniqueNames` gives such tests a ` (n)` suffix, which depends on the report
 * order, so the evidence of a repeated name is not reliable. A skipped or
 * todo twin gives no evidence, so it does not count. `ran` has one boolean
 * for each test. The flags use the same well-formed names as `uniqueNames`.
 */
export const repeatedNameFlags = (fullNames, ran) => {
  const wellFormed = fullNames.map((name) => name.toWellFormed());
  const counts = new Map();
  wellFormed.forEach((name, index) => {
    if (ran[index]) counts.set(name, (counts.get(name) ?? 0) + 1);
  });
  return wellFormed.map((name, index) => ran[index] && counts.get(name) > 1);
};

export const errorMessage = (error) => String(error?.message ?? error);

/**
 * The longest test error that a run result carries, in UTF-16 code units. A
 * run result goes over the pipe for each failed test of each mutant run.
 */
export const MAX_TEST_ERROR_LENGTH = 4096;

// oxlint-disable-next-line no-control-regex -- the pattern matches terminal color codes
const ANSI_ESCAPE = /\u001b\[[0-9;?]*[A-Za-z]/gu;
const STACK_FRAME = /^\s+at /u;
const RUNNER_FRAME = /node_modules[\\/]|node:internal|\(node:|at node:|\(<anonymous>\)/u;

const errorHead = (error) => {
  const message = errorMessage(error);
  const name = typeof error?.name === "string" ? error.name : "";
  return name.length === 0 || message.startsWith(`${name}:`) ? message : `${name}: ${message}`;
};

/** The stack frames of `error` outside the runner, Node, and dependencies. */
const projectFrames = (error) =>
  typeof error?.stack === "string"
    ? error.stack.split("\n").filter((line) => STACK_FRAME.test(line) && !RUNNER_FRAME.test(line))
    : [];

/**
 * The directories whose prefix a test error loses: the run directory and the
 * project root, each also as its real path, the longest first.
 */
export const errorDirectories = (root, runDir) =>
  [
    ...new Set([runDir, realPath(runDir), root, realPath(root)].map((dir) => resolve(dir))),
  ].toSorted((left, right) => right.length - left.length);

const withoutDirectories = (text, directories) =>
  directories.reduce(
    (current, dir) =>
      current.replaceAll(`${pathToFileURL(dir).href}/`, "").replaceAll(`${dir}${sep}`, ""),
    text,
  );

/**
 * Gives the text of the error of a failed test: the name and message of the
 * error, then its stack frames outside the runner. Paths become relative to
 * the project, color codes go, and the text is at most
 * `MAX_TEST_ERROR_LENGTH` long. Gives null for no error.
 */
export const testErrorText = (error, directories) => {
  if (error === undefined || error === null) return null;
  const text = withoutDirectories(
    [errorHead(error), ...projectFrames(error)].join("\n").replace(ANSI_ESCAPE, ""),
    directories,
  ).trim();
  if (text.length === 0) return null;
  return text.length <= MAX_TEST_ERROR_LENGTH ? text : `${text.slice(0, MAX_TEST_ERROR_LENGTH)}...`;
};

/** Rounds each value of a map up to a whole millisecond, with the keys sorted. */
const roundedByKey = (values) =>
  Object.fromEntries(
    Object.keys(values)
      .toSorted(byCodeUnit)
      .map((key) => [key, Math.ceil(values[key])]),
  );

/**
 * Gives the hits of each test that ran, as `[test_id, hits]` pairs. Each
 * entry is `{ shared, hits, report }`. When one entry of the list is
 * `shared`, a hit cannot name its test (for example, concurrent tests
 * overlap). Each test that ran then gets the union of the hits of the list.
 * The scheduler then runs a mutant against every test that can kill it.
 */
export const attributeHits = (entries) => {
  const ran = entries.filter((entry) => hasRun(entry.report.status));
  const shared = entries.some((entry) => entry.shared)
    ? [...new Set(ran.flatMap((entry) => entry.hits))].toSorted()
    : null;
  return ran
    .map((entry) => [entry.report.test_id, shared ?? [...entry.hits].toSorted()])
    .filter(([, hits]) => hits.length > 0);
};

const failureStatus = (failure) => (failure.failure_kind === "timeout" ? "timeout" : "killed");

/**
 * Gives the status of a run. An error outside a test fails the run, so the
 * mutant is detected. Zero executed tests, or fewer than requested with no
 * failure, is a fault of the run and never evidence that the mutant
 * survived.
 */
const runStatus = (tests, errors, requestedCount) => {
  const firstFailure = tests.find((test) => test.status === "fail");
  if (firstFailure !== undefined) return failureStatus(firstFailure);
  if (errors.length > 0) return "killed";
  return tests.length === requestedCount ? "survived" : "runtime_error";
};

/**
 * Builds a `run_result` body from the tests and the errors of a run.
 * `loopTimeout` is true when the loop budget failed the first failing test.
 * The body then has `loop_timeout: true` when the status is `timeout`.
 */
export const runResult = ({
  mutantId,
  tests,
  errors,
  requestedCount,
  durationMs,
  loopTimeout = false,
}) => {
  const status = runStatus(tests, errors, requestedCount);
  return {
    ...(loopTimeout && status === "timeout" ? { loop_timeout: true } : {}),
    mutant_id: mutantId,
    status,
    // With no failing test, a kill comes from an error outside a test, so
    // no test is the killer.
    killed_by:
      status === "killed" ? (tests.find((test) => test.status === "fail")?.test_id ?? null) : null,
    duration_ms: Math.round(durationMs),
    tests_executed: tests.length,
    tests,
    unhandled_errors: errors,
  };
};

/**
 * Reads the requested test ids of a run against the test index of the dry
 * run, and groups them by test file in file order. Each group is
 * `{ file, entries }`, and each entry is the index value.
 */
export const groupRequestedTests = (requested, testIndex) => {
  const groups = new Map();
  for (const testId of requested) {
    const entry = testIndex.get(testId);
    expectRequest(entry !== undefined, `unknown test id (send dry_run first): ${testId}`);
    groups.set(entry.file, [...(groups.get(entry.file) ?? []), { ...entry, testId }]);
  }
  return [...groups.keys()]
    .toSorted(byCodeUnit)
    .map((file) => ({ file, entries: groups.get(file) }));
};

/**
 * Builds a `dry_run_result` body. `summary` has `tests`, `hits`,
 * `staticHits`, `duplicates` (the ids of tests with a repeated name) and
 * `setup`. A path of `pathMap` that is not in `loaded` is
 * an unloaded path.
 */
export const dryRunResult = ({ started, summary, errors, pathMap, loaded }) => ({
  duration_ms: Math.round(performance.now() - started),
  tests: summary.tests,
  hits: summary.hits,
  static_hits: [...summary.staticHits].toSorted(),
  file_setup_ms: roundedByKey(summary.setup),
  duplicate_test_ids: [...summary.duplicates].toSorted(),
  unhandled_errors: errors,
  unloaded_paths: Object.keys(pathMap)
    .filter((path) => !loaded.has(path))
    .toSorted(),
});
