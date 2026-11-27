// Generated from tools/mutation-runner/src/adapters/bun-test.mjs. Do not edit.
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { CompanionError } from "../protocol.mjs";
import { CHILD_ENV } from "../runtime/child.mjs";
import {
  childProcessAdapter,
  createWorkDir,
  initTestFiles,
  spawnWithStderrTail,
  withTestIds,
} from "./child-process.mjs";
import {
  buildOverlay,
  dryRunTimeout,
  hasRun,
  isSupportedVersion,
  unsupportedVersion,
  validateInit,
} from "./common.mjs";

const RUNNER_NAME = "bun-test";
const PRELOAD = fileURLToPath(new URL("../runtime/bun-test-preload.mjs", import.meta.url));
const STDERR_TAIL_BYTES = 4000;
const NO_MATCH = /matched 0 tests/u;
/** The first line of an error in the Bun output, such as `error: load failure`. */
const ERROR_LINE = /^(?:error|[A-Za-z]*Error):/u;

/** The default test file patterns of `bun test`. */
const TEST_FILE_PATTERNS = [
  "**/*.test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
  "**/*_test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
  "**/*.spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
  "**/*_spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
];

/** The default test timeout of `bun test`. */
const BUN_DEFAULT_TIMEOUT_MS = 5000;
/**
 * The timeout of a dry run. The dry run runs the instrumented code, so it
 * gets the Bun default times `DRY_RUN_TIMEOUT_FACTOR`.
 */
const DRY_RUN_TIMEOUT_MS = dryRunTimeout(BUN_DEFAULT_TIMEOUT_MS);

/** The config file that `bun test` reads from its working folder. */
const BUNFIG = "bunfig.toml";

/** A `bunfig.toml` line that runs the tests in a random order. */
const RANDOMIZE = /^\s*randomize\s*=\s*true\b/mu;

/** Reads the Bun version, or throws when the companion does not run on Bun. */
const bunVersion = () => {
  const version = process.versions.bun;
  if (version === undefined) {
    throw new CompanionError(
      "runner-not-found",
      "the bun-test runner needs the Bun runtime; the companion runs on Node.js",
    );
  }
  if (!isSupportedVersion(RUNNER_NAME, version)) {
    throw unsupportedVersion(RUNNER_NAME, `bun test of Bun ${version}`);
  }
  return version;
};

const ENTITIES = { lt: "<", gt: ">", amp: "&", quot: '"', apos: "'" };

const numericEntity = (text) =>
  /^x/iu.test(text) ? Number.parseInt(text.slice(1), 16) : Number.parseInt(text, 10);

const decodeEntity = (match, code) => {
  if (code.startsWith("#")) return String.fromCodePoint(numericEntity(code.slice(1)));
  return ENTITIES[code] ?? match;
};

const decodeXml = (text) => text.replace(/&(#x[0-9a-f]+|#\d+|\w+);/giu, decodeEntity);

const xmlAttributes = (text) =>
  Object.fromEntries(
    [...text.matchAll(/([\w:-]+)="([^"]*)"/gu)].map(([, name, value]) => [name, decodeXml(value)]),
  );

const JUNIT_TAG = /<(\/?)(testsuite|testcase|failure|error|skipped)\b([^>]*?)(\/?)>/gu;

const failureKind = (type) => {
  if (type === "TimeoutError") return "timeout";
  return type === "AssertionError" ? "assertion" : "error";
};

const numberOrNull = (text) => (text === undefined ? null : Number(text));

const secondsToMs = (text) => (text === undefined ? null : Number(text) * 1000);

/** Builds a test case. `suites` starts with the testsuite of the test file. */
const junitCase = (suites, attributes) => ({
  parts: [...suites.slice(1), attributes.name ?? ""],
  line: numberOrNull(attributes.line),
  durationMs: secondsToMs(attributes.time),
  status: "pass",
  failureKind: null,
  error: null,
});

const markFailed = (testCase, attributes) => {
  testCase.status = "fail";
  testCase.failureKind = failureKind(attributes.type);
  if (attributes.message !== undefined) {
    testCase.error = { name: attributes.type, message: attributes.message };
  }
};

const markSkipped = (testCase, attributes) => {
  testCase.status = attributes.message === "TODO" ? "todo" : "skip";
};

/** Applies `mark` to the open test case for an opening tag. */
const markCase = (mark) => (state, tag) => {
  if (state.current === null || tag.close) return;
  mark(state.current, tag.attributes);
};

const JUNIT_HANDLERS = {
  testsuite: (state, tag) => {
    if (tag.close) state.suites.pop();
    else if (!tag.selfClose) state.suites.push(tag.attributes.name ?? "");
  },
  testcase: (state, tag) => {
    if (tag.close) {
      state.current = null;
      return;
    }
    const testCase = junitCase(state.suites, tag.attributes);
    state.cases.push(testCase);
    state.current = tag.selfClose ? null : testCase;
  },
  failure: markCase(markFailed),
  error: markCase(markFailed),
  skipped: markCase(markSkipped),
};

/**
 * Reads the test cases of a `bun test` JUnit report in document order. Each
 * case has the names of its suites and its own name, its line, its time,
 * and its status. The first `testsuite` is the test file.
 */
const parseJunit = (xml) => {
  const state = { suites: [], cases: [], current: null };
  for (const [, close, tag, attributeText, selfClose] of xml.matchAll(JUNIT_TAG)) {
    JUNIT_HANDLERS[tag](state, {
      close: close === "/",
      selfClose: selfClose === "/",
      attributes: xmlAttributes(attributeText),
    });
  }
  return state.cases;
};

const readJunit = (path) => (existsSync(path) ? parseJunit(readFileSync(path, "utf8")) : []);

const readLines = (path) =>
  existsSync(path)
    ? readFileSync(path, "utf8")
        .split("\n")
        .filter((line) => line.length > 0)
        .map((line) => JSON.parse(line))
    : [];

/** Starts one `bun test` process and waits for it. */
const spawnBun = (args, { cwd, env }) =>
  new Promise((resolveExit, rejectExit) => {
    const { child, stderr } = spawnWithStderrTail(process.execPath, args, {
      cwd,
      env,
      tailBytes: STDERR_TAIL_BYTES,
    });
    child.on("error", rejectExit);
    child.on("close", (code, signal) => resolveExit({ code, signal, stderr: stderr() }));
  });

const hasFailedCase = (cases) => cases.some((testCase) => testCase.status === "fail");

/**
 * True for a non-zero exit with no failed test: a failure outside a test,
 * for example a test file that throws when it loads, or an unhandled error.
 * A filter that matches no test also exits non-zero; the run then has no
 * executed test.
 */
const failedOutsideTests = (exit, cases) =>
  exit.code !== 0 && !hasFailedCase(cases) && !NO_MATCH.test(exit.stderr);

const exitReason = (exit) =>
  exit.signal === null ? `exit code ${exit.code}` : `signal ${exit.signal}`;

/** The first error line of the Bun output, or the end of the output. */
const errorDetail = (stderr) => {
  const lines = stderr.split("\n").map((line) => line.trim());
  const nonEmpty = lines.filter((line) => line.length > 0);
  return lines.find((line) => ERROR_LINE.test(line)) ?? nonEmpty.slice(-5).join("\n");
};

/** Gives the errors outside a test of one process. */
const processErrors = (exit, cases, file) =>
  failedOutsideTests(exit, cases)
    ? [
        {
          message: `bun test failed outside a test (${exitReason(exit)}): ${errorDetail(exit.stderr)}`,
          file,
        },
      ]
    : [];

const isOrdered = (ran, rows, randomized) =>
  !randomized && rows.length === ran.length && !rows.some((row) => row.overlap);

/**
 * Joins the hit lines of the preload to the tests that ran, by order. The
 * join is not safe when the counts differ, when tests overlap, or when Bun
 * runs the tests in a random order. Each test that ran then gets the hits
 * of the file, and the result is shared.
 */
const joinHits = (tests, rows, randomized) => {
  const ran = tests.filter((test) => hasRun(test.status));
  const ordered = isOrdered(ran, rows, randomized);
  const allHits = [...new Set(rows.flatMap((row) => row.hits))];
  for (const [index, test] of ran.entries()) {
    test.hits = ordered ? rows[index].hits : allHits;
  }
  return !ordered;
};

const optionalArgs = (condition, args) => (condition ? args : []);

/** The arguments of one `bun test` process. `file` is relative to its folder. */
const bunArgs = (file, junit, { timeoutMs = DRY_RUN_TIMEOUT_MS, pattern, bail }) => [
  "test",
  "--preload",
  PRELOAD,
  "--reporter=junit",
  `--reporter-outfile=${junit}`,
  "--timeout",
  String(timeoutMs),
  ...optionalArgs(bail === true, ["--bail"]),
  ...optionalArgs(pattern !== undefined, ["-t", pattern]),
  // A path that starts with `./` names one file. A bare name is a filter.
  `./${file}`,
];

const childEnv = (workDir, hits, mutantId) => {
  const env = { ...process.env, [CHILD_ENV.overlay]: workDir.overlayPath, [CHILD_ENV.hits]: hits };
  if (mutantId !== null) env[CHILD_ENV.active] = mutantId;
  return env;
};

const isRandomized = (dir) => {
  const bunfig = join(dir, BUNFIG);
  return existsSync(bunfig) && RANDOMIZE.test(readFileSync(bunfig, "utf8"));
};

/**
 * The folder of the nearest `bunfig.toml` from the test file up to the root,
 * relative to the root, or `.` for the root. Bun reads the project
 * `bunfig.toml` only from its working folder and does not search the parent
 * folders. A workspace package with its own file therefore runs its tests
 * from that folder, as `cd <package> && bun test` does (issue #5286). A file
 * above the root never applies.
 */
const bunfigFolder = (root, file) => {
  for (let dir = posix.dirname(file); dir !== "."; dir = posix.dirname(dir)) {
    if (existsSync(join(root, dir, BUNFIG))) return dir;
  }
  return ".";
};

/**
 * Gives the working folder of each test file, with the file path relative
 * to that folder and the random-order setting of its `bunfig.toml`.
 */
const runFolders = (root) => {
  const randomized = new Map();
  return (file) => {
    const dir = bunfigFolder(root, file);
    if (!randomized.has(dir)) randomized.set(dir, isRandomized(join(root, dir)));
    return {
      cwd: join(root, dir),
      file: posix.relative(dir, file),
      randomized: randomized.get(dir),
    };
  };
};

/** Reads the result of one finished `bun test` process. */
const fileResult = ({ file, exit, junit, hits, randomized, durationMs }) => {
  const cases = readJunit(junit);
  const tests = withTestIds(file, cases).map((test) => ({ ...test, hits: [] }));
  const rows = readLines(hits);
  return {
    tests,
    shared: joinHits(tests, rows, randomized),
    staticHits: new Set(rows.flatMap((row) => row.static_hits)),
    loaded: new Set(rows.flatMap((row) => row.loaded)),
    timedOut: false,
    errors: processErrors(exit, cases, file),
    durationMs,
  };
};

/**
 * Creates the bun test adapter for an `init` request. The companion runs on
 * Bun. Each dry run and each run starts one `bun test` process for each test
 * file, with a preload that serves the instrumented files and records the
 * hits of each test.
 *
 * The companion reads this export from the `import()` namespace that
 * `loadAdapter` returns. The `ignoreExports` entry of the Fallow config
 * keeps it out of the unused-export and duplicate-export findings.
 */
export const createAdapter = async (init) => {
  validateInit(init);
  const version = bunVersion();
  const root = resolve(init.root);
  const overlay = buildOverlay(root, init.run_dir, init.path_map);
  const testFiles = initTestFiles(root, init, TEST_FILE_PATTERNS);
  const folderOf = runFolders(root);
  const workDir = createWorkDir("fallow-mutation-bun-", overlay);
  Object.assign(process.env, init.options?.env ?? {});
  let processCount = 0;

  const runFile = async (file, options) => {
    processCount += 1;
    const junit = join(workDir.dir, `junit-${processCount}.xml`);
    const hits = join(workDir.dir, `hits-${processCount}.jsonl`);
    const folder = folderOf(file);
    const started = performance.now();
    try {
      const exit = await spawnBun(bunArgs(folder.file, junit, options), {
        cwd: folder.cwd,
        env: childEnv(workDir, hits, options.mutantId),
      });
      const durationMs = performance.now() - started;
      return fileResult({
        file,
        exit,
        junit,
        hits,
        randomized: folder.randomized,
        durationMs,
      });
    } finally {
      rmSync(junit, { force: true });
      rmSync(hits, { force: true });
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
