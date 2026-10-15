// Generated from tools/mutation-runner/src/adapters/node-test.mjs. Do not edit.
import { resolve } from "node:path";
import { run as runNodeTests } from "node:test";
import { pathToFileURL } from "node:url";

import { CompanionError } from "../protocol.mjs";
import { CHILD_ENV, HITS_DIAGNOSTIC } from "../runtime/child.mjs";
import {
  childProcessAdapter,
  createWorkDir,
  initTestFiles,
  withTestIds,
} from "./child-process.mjs";
import { buildOverlay, errorMessage, isSupportedVersion, validateInit } from "./common.mjs";

const RUNNER_NAME = "node-test";
const PRELOAD = new URL("../runtime/node-test-preload.mjs", import.meta.url).href;

/**
 * The default test file patterns of `node --test`, with the TypeScript
 * extensions that Node strips.
 */
const TEST_FILE_PATTERNS = [
  "**/*.test.{cjs,mjs,js,cts,mts,ts}",
  "**/*-test.{cjs,mjs,js,cts,mts,ts}",
  "**/*_test.{cjs,mjs,js,cts,mts,ts}",
  "**/test-*.{cjs,mjs,js,cts,mts,ts}",
  "**/test.{cjs,mjs,js,cts,mts,ts}",
  "**/test/**/*.{cjs,mjs,js,cts,mts,ts}",
];

/** The failure type of a suite with a failed test. It is not a failure outside a test. */
const SUBTEST_FAILURE = "subtestsFailed";
const TIMEOUT_FAILURE = "testTimeoutFailure";
const STDERR_TAIL_LINES = 8;
const MAX_STDERR_CHUNKS = STDERR_TAIL_LINES * 4;

/** Reads the Node version, or throws when the companion does not run on Node. */
const nodeVersion = () => {
  if (process.versions.bun !== undefined) {
    throw new CompanionError(
      "runner-not-found",
      "the node-test runner needs the Node.js runtime; the companion runs on Bun",
    );
  }
  const version = process.versions.node;
  if (!isSupportedVersion(RUNNER_NAME, version)) {
    throw new CompanionError("runner-version", `node:test of Node.js ${version} is not supported`);
  }
  return version;
};

const orNull = (value) => value ?? null;

const detailsOf = (data) => data.details ?? {};

const errorOf = (data) => detailsOf(data).error ?? {};

const isAssertion = (cause) => cause?.code === "ERR_ASSERTION" || cause?.name === "AssertionError";

const isTimeout = (error) => error.failureType === TIMEOUT_FAILURE;

const failureKind = (error) => {
  if (isTimeout(error)) return "timeout";
  return isAssertion(error.cause) ? "assertion" : "error";
};

const isFlagSet = (value) => value !== undefined && value !== false;

const resultStatus = (type, data) => {
  if (isFlagSet(data.skip)) return "skip";
  if (isFlagSet(data.todo)) return "todo";
  return type === "test:pass" ? "pass" : "fail";
};

/**
 * True for the result of a test file process: Node reports a file that
 * fails outside a test (for example a module that throws when it loads) as a
 * test with the file path as its name.
 */
const isFileResult = (data) => data.nesting === 0 && data.name === data.file;

const isSuite = (data) => detailsOf(data).type === "suite";

/** True for a suite that fails outside its tests, for example in a `before` hook. */
const isSuiteError = (type, data) =>
  type === "test:fail" && isSuite(data) && errorOf(data).failureType !== SUBTEST_FAILURE;

/**
 * The first line of an uncaught error that Node writes to stderr, for
 * example `SyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript enum
 * is not supported in strip-only mode`.
 */
const ERROR_LINE = /^\w*(?:Error|Exception)(?: \[\w+\])?: /u;

const stderrLines = (stderr) => stderr.join("").trim().split("\n");

/**
 * Gives the error line and the last lines of the stderr of a file process.
 * The error line is the last match, because a test file or a preload can
 * print an unrelated error line before the error that stops the process.
 */
const stderrDetail = (stderr) => {
  const lines = stderrLines(stderr);
  const errorLine = lines.findLast((line) => ERROR_LINE.test(line));
  const tail = lines
    .slice(-STDERR_TAIL_LINES)
    .filter((line) => line !== errorLine)
    .join("\n");
  return { errorLine, tail };
};

const causeText = (error) => errorMessage(error.cause ?? error.message ?? "failed");

const joinDetail = (head, tail) => (tail.length === 0 ? head : `${head}: ${tail}`);

/**
 * Gives the message of a failure outside a test. A file process that fails
 * has only a generic error, so the message adds its stderr. When the stderr
 * has an error line, the message starts with that line, and the last lines
 * of the stderr follow.
 */
const failureMessage = (data, stderr) => {
  const message = causeText(errorOf(data));
  if (!isFileResult(data)) return message;
  const { errorLine, tail } = stderrDetail(stderr);
  if (errorLine === undefined) return joinDetail(message, tail);
  return tail.length === 0 ? errorLine : `${errorLine}\n${tail}`;
};

/** The Node flags whose value is a module specifier or a module path. */
const URL_FLAGS = new Set(["--import", "--experimental-loader", "--loader"]);
const PATH_FLAGS = new Set(["--require", "-r"]);

const isRelativePath = (value) => /^\.\.?(?:[\\/]|$)/u.test(value);

/**
 * Resolves a relative module value of `--import`, `--require` or a loader
 * flag against the project root. `--import` and a loader take a URL, and
 * `--require` takes a path. Another value does not change.
 */
const resolveNodeArg = (root, flag, value) => {
  if (!isRelativePath(value)) return value;
  const path = resolve(root, value);
  if (URL_FLAGS.has(flag)) return pathToFileURL(path).href;
  return PATH_FLAGS.has(flag) ? path : value;
};

/**
 * Gives the Node flags of the test script for each test process. Each flag
 * with a value is a separate entry, followed by its value.
 */
const nodeArgs = (root, args = []) =>
  args.map((arg, index) => (index === 0 ? arg : resolveNodeArg(root, args[index - 1], arg)));

const testFailureKind = (status, data) => (status === "fail" ? failureKind(errorOf(data)) : null);

/** Builds the entry of one test result. `names` holds the names of its suites. */
const testEntry = (names, type, data) => {
  const status = resultStatus(type, data);
  return {
    parts: [...names.slice(0, data.nesting), data.name],
    line: orNull(data.line),
    column: orNull(data.column),
    durationMs: orNull(detailsOf(data).duration_ms),
    status,
    failureKind: testFailureKind(status, data),
    hits: [],
  };
};

const fileResultKind = (data) => (isTimeout(errorOf(data)) ? "fileTimeout" : "outside");

const otherResultKind = (type, data) => {
  if (isSuiteError(type, data)) return "outside";
  return isSuite(data) ? "suite" : "test";
};

/** Sorts a result event: a file timeout, a failure outside a test, a suite, or a test. */
const resultKind = (type, data) =>
  isFileResult(data) ? fileResultKind(data) : otherResultKind(type, data);

const RESULT_ACTIONS = {
  fileTimeout: (state) => {
    state.timedOut = true;
  },
  outside: (state, type, data) => {
    if (type === "test:fail") {
      state.errors.push({ message: failureMessage(data, state.stderr), file: state.file });
    }
  },
  suite: () => {},
  test: (state, type, data) => {
    state.last = testEntry(state.names, type, data);
    state.tests.push(state.last);
  },
};

const recordResult = (state, type, data) => {
  state.last = null;
  RESULT_ACTIONS[resultKind(type, data)](state, type, data);
};

const isHitsDiagnostic = (message) =>
  typeof message === "string" && message.startsWith(HITS_DIAGNOSTIC);

const isSameLocation = (test, data) =>
  test !== null && test.line === data.line && test.column === data.column;

const addAll = (target, values) => {
  for (const value of values) target.add(value);
};

const attachHits = (state, hits, joined) => {
  if (joined) state.last.hits = hits;
  else state.tests.at(-1)?.hits.push(...hits);
};

// The diagnostic of the preload follows the result of its test, with the
// same line and column. Hits that cannot name their test are shared.
const recordDiagnostic = (state, data) => {
  if (!isHitsDiagnostic(data.message)) return;
  const payload = JSON.parse(data.message.slice(HITS_DIAGNOSTIC.length));
  addAll(state.loaded, payload.loaded);
  addAll(state.staticHits, payload.static_hits);
  const joined = isSameLocation(state.last, data);
  attachHits(state, payload.hits, joined);
  state.shared ||= !joined || payload.overlap;
};

const EVENT_HANDLERS = {
  "test:start": (state, data) => {
    state.names.length = data.nesting;
    state.names.push(data.name);
  },
  "test:pass": (state, data) => recordResult(state, "test:pass", data),
  "test:fail": (state, data) => recordResult(state, "test:fail", data),
  "test:diagnostic": recordDiagnostic,
  "test:stderr": (state, data) => {
    if (state.stderr.length < MAX_STDERR_CHUNKS) state.stderr.push(String(data.message));
  },
  "test:summary": (state, data) => {
    if (data.file !== undefined) state.durationMs = orNull(data.duration_ms);
  },
};

const handleEvent = (state, { type, data }) => EVENT_HANDLERS[type]?.(state, data);

const isResultEvent = (type) => type === "test:pass" || type === "test:fail";

const hasFailed = (state) =>
  state.last?.status === "fail" || state.timedOut || state.errors.length > 0;

/** True when a failure ends the read: `stopAtFailure()` stopped the process. */
const shouldStop = (state, type, stopAtFailure) =>
  isResultEvent(type) && hasFailed(state) && stopAtFailure();

const newFileState = (file) => ({
  file,
  names: [],
  tests: [],
  errors: [],
  loaded: new Set(),
  staticHits: new Set(),
  stderr: [],
  last: null,
  durationMs: null,
  shared: false,
  timedOut: false,
});

/**
 * Reads the event stream of one test file process: each test in report
 * order with its name parts, status and hits, the errors outside a test,
 * the source files that the process loaded, and the file duration. After a
 * failure, `stopAtFailure()` can end the read; it is true when it stopped
 * the process.
 */
const readFileEvents = async (stream, file, stopAtFailure) => {
  const state = newFileState(file);
  for await (const event of stream) {
    handleEvent(state, event);
    if (shouldStop(state, event.type, stopAtFailure)) break;
  }
  const { tests, errors, loaded, staticHits, shared, timedOut, durationMs } = state;
  return {
    tests: withTestIds(file, tests),
    errors,
    loaded,
    staticHits,
    shared,
    timedOut,
    durationMs,
  };
};

const setActiveMutant = (mutantId) => {
  if (mutantId === null) delete process.env[CHILD_ENV.active];
  else process.env[CHILD_ENV.active] = mutantId;
};

/** The `run()` options that only a mutant run sets. */
const runOptions = ({ timeoutMs, pattern }) => ({
  // Node 24 and later apply the timeout to each test. Node 22 applies it to
  // the whole test file process.
  ...(timeoutMs === undefined ? {} : { timeout: timeoutMs }),
  ...(pattern === undefined ? {} : { testNamePatterns: [new RegExp(pattern, "u")] }),
});

/**
 * Creates the node:test adapter for an `init` request. The companion runs on
 * Node. Each dry run and each run starts one Node process for each test file
 * through `run()` of `node:test`, with a preload that serves the
 * instrumented files and records the hits of each test.
 *
 * The companion reads this export from the `import()` namespace that
 * `loadAdapter` returns. The `ignoreExports` entry of the Fallow config
 * keeps it out of the unused-export and duplicate-export findings.
 */
export const createAdapter = async (init) => {
  validateInit(init);
  const version = nodeVersion();
  const root = resolve(init.root);
  const overlay = buildOverlay(root, init.run_dir, init.path_map);
  const testFiles = initTestFiles(root, init, TEST_FILE_PATTERNS);
  const workDir = createWorkDir("fallow-mutation-node-", overlay);
  // The preload comes first, so a loader of the project wraps its hook.
  const execArgv = ["--import", PRELOAD, ...nodeArgs(root, init.options?.node_args)];
  // A companion that a node:test process starts inherits its test context.
  // The test processes of the adapter must report to this companion instead.
  delete process.env.NODE_TEST_CONTEXT;
  Object.assign(process.env, init.options?.env ?? {});

  // `run()` gives its test processes the environment of this process, so
  // the preload values are set for the time of one file. With `bail`, the
  // first failure stops the test process. That also ends a test that keeps
  // a timer after its timeout.
  const runFile = async (file, options) => {
    const controller = new AbortController();
    const stopAtFailure = () => {
      if (options.bail === true) controller.abort();
      return options.bail === true;
    };
    process.env[CHILD_ENV.overlay] = workDir.overlayPath;
    setActiveMutant(options.mutantId);
    try {
      const stream = runNodeTests({
        files: [resolve(root, file)],
        cwd: root,
        isolation: "process",
        concurrency: 1,
        signal: controller.signal,
        execArgv,
        ...runOptions(options),
      });
      return await readFileEvents(stream, file, stopAtFailure);
    } finally {
      delete process.env[CHILD_ENV.overlay];
      setActiveMutant(null);
    }
  };

  return childProcessAdapter({
    runnerName: RUNNER_NAME,
    init,
    version,
    testFiles,
    workDir,
    runFile,
  });
};
