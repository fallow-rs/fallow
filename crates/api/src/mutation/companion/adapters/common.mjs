// Generated from tools/mutation-runner/src/adapters/common.mjs. Do not edit.
import { isAbsolute, relative, resolve, sep } from "node:path";

import { RUNNERS } from "../generated-protocol.mjs";
import { CompanionError, expectRequest, isPlainObject } from "../protocol.mjs";
import { escapeRegExp, realPath } from "../runtime/child.mjs";

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
  expectRequest(isOptional(options.env, isStringMap), "options.env must map names to strings");
  expectRequest(
    isOptional(options.node_args, isStringList),
    "options.node_args must be a list of strings",
  );
};

export const validateDryRun = (request) => {
  expectRequest(isOptional(request.test_files, isStringList), "test_files must be a list of paths");
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

/** Names each test. A repeated full name gets a ` (n)` suffix. */
export const uniqueNames = (fullNames) => {
  const seen = new Map();
  return fullNames.map((fullName) => {
    const count = (seen.get(fullName) ?? 0) + 1;
    seen.set(fullName, count);
    return count === 1 ? fullName : `${fullName} (${count})`;
  });
};

/** True for a test that ran to a result. A skipped or cancelled test did not run. */
export const hasRun = (status) => status === "pass" || status === "fail";

export const errorMessage = (error) => String(error?.message ?? error);

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

/** Builds a `run_result` body from the tests and the errors of a run. */
export const runResult = ({ mutantId, tests, errors, requestedCount, durationMs }) => {
  const status = runStatus(tests, errors, requestedCount);
  return {
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
 * `staticHits` and `setup`. A path of `pathMap` that is not in `loaded` is
 * an unloaded path.
 */
export const dryRunResult = ({ started, summary, errors, pathMap, loaded }) => ({
  duration_ms: Math.round(performance.now() - started),
  tests: summary.tests,
  hits: summary.hits,
  static_hits: [...summary.staticHits].toSorted(),
  file_setup_ms: roundedByKey(summary.setup),
  unhandled_errors: errors,
  unloaded_paths: Object.keys(pathMap)
    .filter((path) => !loaded.has(path))
    .toSorted(),
});
