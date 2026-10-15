import { existsSync, globSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { CompanionError } from "../protocol.mjs";
import {
  TEST_ID_SEPARATOR,
  attributeHits,
  byCodeUnit,
  dryRunResult,
  escapeRegExp,
  groupRequestedTests,
  hasRun,
  runResult,
  toPosix,
  uniqueNames,
  validateDryRun,
  validateRun,
} from "./common.mjs";

const SKIPPED_DIRECTORIES = new Set(["node_modules", ".git"]);
const NAME_SEPARATOR = " > ";

const inSkippedDirectory = (path) =>
  String(path)
    .split(/[\\/]/u)
    .some((part) => SKIPPED_DIRECTORIES.has(part));

const sortedPaths = (paths) => [...new Set(paths.map(toPosix))].toSorted(byCodeUnit);

/**
 * Lists the files below the root that match the default patterns of a
 * runner, as sorted root-relative paths. Files in `node_modules` and `.git`
 * are not listed.
 */
const discoverTestFiles = (root, patterns) =>
  sortedPaths(
    globSync(patterns, { cwd: root, exclude: inSkippedDirectory }).filter(
      (path) => !inSkippedDirectory(path),
    ),
  );

const listedTestFiles = (init) => init.options?.test_files ?? [];

const assertTestFilesExist = (root, files) => {
  const missing = files.find((file) => !existsSync(resolve(root, file)));
  if (missing !== undefined) {
    throw new CompanionError("test-file-missing", `test file not found: ${missing}`);
  }
  return files;
};

/**
 * Gives the test files of `init`: the `options.test_files` list when it is
 * not empty, otherwise the files that the default patterns match. A listed
 * file that does not exist is an error.
 */
export const initTestFiles = (root, init, patterns) => {
  const listed = listedTestFiles(init);
  if (listed.length === 0) return discoverTestFiles(root, patterns);
  return assertTestFilesExist(root, sortedPaths(listed));
};

/**
 * Selects the test files of a dry run: the requested files, or each test
 * file of the adapter. A requested file that the adapter does not list is
 * an error.
 */
const selectTestFiles = (requested, testFiles, runnerName) => {
  if (requested === undefined || requested.length === 0) return testFiles;
  const known = new Set(testFiles);
  const files = sortedPaths(requested);
  const unknown = files.find((file) => !known.has(file));
  if (unknown !== undefined) {
    throw new CompanionError("test-file-missing", `not a ${runnerName} test file: ${unknown}`);
  }
  return files;
};

/**
 * Creates a private work directory for the files that the adapter gives to
 * its test processes, and writes the overlay file there. `remove` deletes
 * the directory.
 */
export const createWorkDir = (prefix, overlay) => {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  const overlayPath = join(dir, "overlay.json");
  writeFileSync(overlayPath, JSON.stringify(Object.fromEntries(overlay)));
  return {
    dir,
    overlayPath,
    remove: () => rmSync(dir, { recursive: true, force: true }),
  };
};

/**
 * Gives each test of one file its name and test id, in report order. The
 * name joins the names of the suites and the test with ` > `. Each test is
 * `{ parts, ... }`.
 */
export const withTestIds = (file, tests) => {
  const names = uniqueNames(tests.map((test) => test.parts.join(NAME_SEPARATOR)));
  return tests.map((test, index) => ({
    ...test,
    name: names[index],
    testId: `${file}${TEST_ID_SEPARATOR}${names[index]}`,
  }));
};

/**
 * The pattern text of one test: the names of its suites and its own name,
 * joined with spaces. Each whitespace run matches any whitespace, because a
 * Node test process matches a name without its trailing whitespace. A
 * pattern that selects more tests than requested costs time only: the run
 * drops each result that it did not request.
 */
const namePatternText = (parts) => parts.map(escapeRegExp).join(" ").replace(/\s+/gu, "\\s*");

/** The source of the name pattern that selects the tests of `entries`. */
const namePatternSource = (entries) =>
  `^(?:${[...new Set(entries.map((entry) => namePatternText(entry.parts)))].join("|")})$`;

const runEntry = (test) =>
  test.status === "fail"
    ? { test_id: test.testId, status: "fail", failure_kind: test.failureKind }
    : { test_id: test.testId, status: "pass" };

/** Gives a timeout failure for each requested test of a file with no result. */
const timedOutEntries = (entries, reported) => {
  const done = new Set(reported.map((test) => test.testId));
  return entries
    .filter((entry) => !done.has(entry.testId))
    .map((entry) => ({ test_id: entry.testId, status: "fail", failure_kind: "timeout" }));
};

/** The report entries of one file in a run. */
const runEntries = (result, entries, wanted) => {
  const reported = result.tests.filter((test) => wanted.has(test.testId) && hasRun(test.status));
  const timedOut = result.timedOut ? timedOutEntries(entries, reported) : [];
  return [...reported.map(runEntry), ...timedOut];
};

const hasFailure = (tests, errors) =>
  errors.length > 0 || tests.some((test) => test.status === "fail");

const dryRunEntry = (file, test, shared) => ({
  shared,
  hits: test.hits,
  report: {
    test_id: test.testId,
    file,
    name: test.name,
    line: test.line,
    duration_ms: test.durationMs,
    status: test.status,
  },
});

const addAll = (target, values) => {
  for (const value of values) target.add(value);
};

const durationOf = (value) => value ?? 0;

/** Gives the time of a file process outside its tests. */
const setupMs = (result) => {
  const testMs = result.tests.reduce((sum, test) => sum + durationOf(test.durationMs), 0);
  return Math.max(0, durationOf(result.durationMs) - testMs);
};

/** Adds the result of one dry-run file to the summary. */
const recordDryRunFile = (file, result, summary) => {
  const entries = result.tests.map((test) => dryRunEntry(file, test, result.shared));
  summary.tests.push(...entries.map((entry) => entry.report));
  addAll(summary.staticHits, result.staticHits);
  addAll(summary.loaded, result.loaded);
  Object.assign(summary.hits, Object.fromEntries(attributeHits(entries)));
  summary.errors.push(...result.errors);
  summary.setup[file] = setupMs(result);
};

const emptySummary = () => ({
  tests: [],
  hits: {},
  staticHits: new Set(),
  loaded: new Set(),
  setup: {},
  errors: [],
});

/**
 * Creates the dry run and the run of an adapter that starts one test process
 * for each test file. `runFile(file, options)` runs one file and gives
 * `{ tests, staticHits, loaded, shared, timedOut, errors, durationMs }`. Each
 * test has `testId`, `name`, `parts`, `line`, `durationMs`, `status`,
 * `failureKind` and `hits`. `shared` is true when a hit cannot name its test.
 * `timedOut` is true when the timeout stopped the whole file process, so a
 * requested test with no result timed out. The options are `mutantId` (null
 * in a dry run), `timeoutMs`, `pattern` (the source of a name pattern) and
 * `bail`.
 */
const createChildProcessRunner = ({ runnerName, init, testFiles, runFile }) => {
  const testIndex = new Map();

  const dryRun = async (request) => {
    validateDryRun(request);
    const files = selectTestFiles(request.test_files, testFiles, runnerName);
    const started = performance.now();
    const summary = emptySummary();
    for (const file of files) {
      const result = await runFile(file, { mutantId: null });
      for (const test of result.tests) testIndex.set(test.testId, { file, parts: test.parts });
      recordDryRunFile(file, result, summary);
    }
    return dryRunResult({
      started,
      summary,
      errors: summary.errors,
      pathMap: init.path_map,
      loaded: summary.loaded,
    });
  };

  // A run starts one process for each file, in file order. With `bail`, a
  // failure stops the run before the next file.
  const run = async (request) => {
    validateRun(request);
    const requested = [...new Set(request.test_ids)];
    const wanted = new Set(requested);
    const started = performance.now();
    const tests = [];
    const errors = [];
    for (const { file, entries } of groupRequestedTests(requested, testIndex)) {
      const result = await runFile(file, {
        mutantId: request.mutant_id,
        timeoutMs: request.timeout_ms,
        pattern: namePatternSource(entries),
        bail: request.bail,
      });
      tests.push(...runEntries(result, entries, wanted));
      errors.push(...result.errors);
      if (request.bail && hasFailure(tests, errors)) break;
    }
    return runResult({
      mutantId: request.mutant_id,
      tests,
      errors,
      requestedCount: requested.length,
      durationMs: performance.now() - started,
    });
  };

  return { dryRun, run };
};

/**
 * Creates an adapter that starts one test process for each test file. Each
 * run starts new test processes, so no module state remains and a reload
 * needs no action. A run stops the test process at the first failure.
 */
export const childProcessAdapter = ({ runnerName, init, version, testFiles, workDir, runFile }) => {
  const { dryRun, run } = createChildProcessRunner({ runnerName, init, testFiles, runFile });
  return {
    runnerVersion: version,
    testFiles,
    capabilities: {
      per_test_attribution: true,
      test_filter: true,
      hot_switch: false,
      bail: true,
      fresh_modules_per_run: true,
      batch_mutants: false,
    },
    notes: [],
    dryRun,
    run,
    reload: async () => ({ method: "noop" }),
    close: async () => workDir.remove(),
  };
};
