import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { CompanionError, expectRequest, isPlainObject } from "../protocol.mjs";
import { overlayErrorsPath } from "../runtime/child.mjs";
import { LOOP_TIMEOUT_NAME, WALL_LOOP_TIMEOUT_NAME } from "../runtime/global.mjs";
import { recordEntries } from "../runtime/records.mjs";
import { SIDES } from "../runtime/replay.mjs";
import { createWorkDir } from "./child-process.mjs";
import { REUSED_POOLS, createReusedPool } from "./vitest-reuse.mjs";
import {
  TEST_ID_SEPARATOR,
  addRecords,
  attributeHits,
  buildOverlay,
  byCodeUnit,
  dryRunResult,
  dryRunTimeout,
  errorDirectories,
  errorMessage,
  hasRun,
  hasTimeoutLimit,
  isSupportedVersion,
  parseVersion,
  realPath,
  replayEntry,
  runResult,
  testErrorText,
  toPosix,
  repeatedNameFlags,
  uniqueNames,
  unsupportedVersion,
  validateDryRun,
  validateInit,
  validateRecord,
  validateReplay,
  validateRun,
} from "./common.mjs";

const RUNNER_NAME = "vitest";
const ACTIVE_MUTANT_KEY = "fallowActiveMutant";
const FRESH_MODULES_KEY = "fallowFreshModules";
const LOOP_BUDGET_KEY = "fallowLoopBudget";
const OVERLAY_KEY = "fallowOverlay";
const SUBPROCESS_PRELOAD_KEY = "fallowSubprocessPreload";
const SUBPROCESS_PRELOAD = new URL("../runtime/node-subprocess-preload.mjs", import.meta.url);
const SETUP_FILE = fileURLToPath(new URL("../runtime/vitest-setup.mjs", import.meta.url));
const REPLAY_FILE = fileURLToPath(new URL("../runtime/vitest-replay.mjs", import.meta.url));
const REPLAY_KEY = "fallowReplay";
const RECORD_ARGS_KEY = "fallowRecordArgs";
// The time of the module loads and of Vitest itself in a replay test, on top
// of the call timeouts.
const REPLAY_TEST_MARGIN_MS = 10_000;
/** The companion directory, with the runtime files that a test worker loads. */
const COMPANION_DIR = fileURLToPath(new URL("..", import.meta.url));
const TIMEOUT_MESSAGE = /\b(?:Test|Hook) timed out in \d+ms/u;
const OVERLAY_PLUGIN = "fallow-mutation-overlay";

/** Keys a test module by its project, because two projects can include one file. */
const moduleKey = (project, moduleId) => `${project.name}\0${moduleId}`;

const resolvePackage = (projectRequire, root) => {
  try {
    return projectRequire.resolve("vitest/package.json");
  } catch {
    throw new CompanionError("runner-not-found", `vitest is not installed in ${root}`);
  }
};

/** Loads `vitest/node` from the project, never from the companion. */
const loadProjectVitest = async (root) => {
  const projectRequire = createRequire(join(root, "package.json"));
  const { version } = JSON.parse(readFileSync(resolvePackage(projectRequire, root), "utf8"));
  if (!isSupportedVersion(RUNNER_NAME, version)) {
    throw unsupportedVersion(RUNNER_NAME, `vitest ${version}`);
  }
  try {
    const node = await import(pathToFileURL(projectRequire.resolve("vitest/node")).href);
    return {
      createVitest: node.createVitest,
      version,
      workers: { forks: node.ForksPoolWorker, threads: node.ThreadsPoolWorker },
    };
  } catch (error) {
    throw new CompanionError("runner-load-failed", `cannot import vitest/node: ${error.message}`);
  }
};

const readSourceMap = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null);

/** Vue, Svelte and Astro component files. */
const COMPONENT_FILE = /\.(?:vue|svelte|astro)$/u;

const isComponent = (source) => COMPONENT_FILE.test(source);

/** A request for a file on disk: no virtual module and no query. */
const isPlainRequest = (id) => !id.startsWith("\0") && !id.includes("?");

/**
 * The overlay entry that the plugin serves for a request, or undefined. A
 * script file gets its copy also for a request with a query. A component file
 * gets its copy only for a plain request.
 */
const servedEntry = (overlay, id) => {
  if (id.startsWith("\0")) return undefined;
  const entry = overlay.get(realPath(id.split("?", 1)[0]));
  if (entry === undefined) return undefined;
  return !isComponent(entry.source) || isPlainRequest(id) ? entry : undefined;
};

/**
 * Serves the instrumented copy of each mapped file in the `load` hook, as
 * if the copy were the file on disk. Each `transform` hook of Vite, Vitest
 * and the project then runs on the copy, as `vitest run` runs it on the
 * original. For example, Vitest rewrites each `import.meta.env` read to the
 * environment object that `vi.stubEnv` changes, and a framework plugin
 * compiles the instrumented component. A component request with a query (a
 * style or a template block) is not served: the framework plugin makes it
 * from the component that it compiled.
 *
 * A project plugin with `enforce: "pre"` can load a script file before this
 * plugin, and Vite keeps the first `load` result. The `transform` hook then
 * replaces the code of that file with the copy. The transforms that ran
 * before this plugin do not apply to the copy in that case.
 *
 * The `config` hook adds this plugin to each inline project definition. Vitest 4 gives the
 * plugins of `createVitest` to the root project only. Vitest 5 does not give
 * them to an inline project with a `browser` block, also when browser mode
 * is off. Vite keeps one copy of a plugin that a project gets twice.
 *
 * The `config` hook also adds the companion directory to `server.fs.allow`
 * of each server that has this plugin. A client test environment, such as
 * `happy-dom` or `jsdom`, loads the setup file of the runtime through Vite.
 * Vite refuses a file outside `server.fs.allow`, and the companion directory
 * is outside the project when `FALLOW_CACHE_DIR` is outside it. Vitest adds
 * its own default entries to the list, so the workspace root stays allowed.
 */
const overlayPlugin = (overlay, loaded) => {
  const servedIds = new Set();
  const serve = (entry) => {
    loaded.add(entry.source);
    return {
      code: readFileSync(entry.target, "utf8"),
      map: readSourceMap(`${entry.target}.map`),
    };
  };
  const plugin = {
    name: OVERLAY_PLUGIN,
    enforce: "pre",
    config(config) {
      const projects = config.test?.projects;
      if (Array.isArray(projects)) {
        config.test.projects = projects.map((project) =>
          isPlainObject(project)
            ? { ...project, plugins: [...(project.plugins ?? []), plugin] }
            : project,
        );
      }
      return { server: { fs: { allow: [COMPANION_DIR] } } };
    },
    load(id) {
      const entry = servedEntry(overlay, id);
      if (entry === undefined) return null;
      servedIds.add(id);
      return serve(entry);
    },
    transform(_code, id) {
      if (servedIds.delete(id)) return null;
      const entry = servedEntry(overlay, id);
      if (entry === undefined || isComponent(entry.source)) return null;
      return serve(entry);
    },
  };
  return plugin;
};

/**
 * True for a test that needed a retry. The project `retry` is 0, but a
 * `retry` option of a test or a suite in the test code still applies.
 */
const wasRetried = (testCase) => (testCase.diagnostic()?.retryCount ?? 0) > 0;

/**
 * Reads the status of a test for each Vitest state. A pass after a retry is a
 * failure: the failed first attempt is the result that counts, so a retry
 * never hides a kill.
 */
const STATUS_BY_STATE = {
  passed: (testCase) => (wasRetried(testCase) ? "fail" : "pass"),
  failed: () => "fail",
  skipped: (testCase) => (testCase.options.mode === "todo" ? "todo" : "skip"),
};

/** Gives the status of a test, or null for a test that did not finish. */
const testStatus = (testCase) => STATUS_BY_STATE[testCase.result().state]?.(testCase) ?? null;

const isConcurrent = (testCase) =>
  testCase.options.concurrent === true || testCase.project.config.sequence?.concurrent === true;

const hasErrorNamed = (testCase, names) =>
  (testCase.result().errors ?? []).some((error) => names.includes(error?.name));

/** True for a test that the loop budget ended, on either clock. */
const isLoopTimeout = (testCase) =>
  hasErrorNamed(testCase, [LOOP_TIMEOUT_NAME, WALL_LOOP_TIMEOUT_NAME]);

const failureKind = (testCase) => {
  const errors = testCase.result().errors ?? [];
  if (isLoopTimeout(testCase)) return "timeout";
  if (errors.some((error) => TIMEOUT_MESSAGE.test(errorMessage(error)))) return "timeout";
  return errors.some((error) => error?.name === "AssertionError") ? "assertion" : "error";
};

/**
 * Turns off the persistent transform cache. Its key covers the original file
 * text, not the run-dir file, so a cached entry from an earlier run directory
 * would replace the overlay. Vitest 5 has the option at the top level and
 * turns it on by default. Vitest 4 has it under `experimental`.
 */
const moduleCacheOptions = (version) =>
  parseVersion(version)[0] >= 5
    ? { fsModuleCache: false }
    : { experimental: { fsModuleCache: false } };

/** Gives the test-file path of `path` relative to the root, or null. */
const relativeTestFile = (root, path) =>
  typeof path === "string" ? toPosix(relative(realPath(root), realPath(path))) : null;

/** Lists the errors of a test module and of each suite in it. */
const moduleErrors = (testModule, root) => {
  const file = relativeTestFile(root, testModule.moduleId);
  return [testModule, ...testModule.children.allSuites()].flatMap((owner) =>
    owner.errors().map((error) => ({ message: errorMessage(error), file })),
  );
};

/**
 * Collects the errors that fail a Vitest run outside a test: unhandled
 * errors and rejections, and errors of a test module or a suite (for example
 * a module that throws when it loads). Vitest fails the run for each of them,
 * also when every test passes.
 */
const runErrors = (result, root) => [
  ...result.testModules.flatMap((testModule) => moduleErrors(testModule, root)),
  ...result.unhandledErrors.map((error) => ({
    message: errorMessage(error),
    file: relativeTestFile(root, error?.VITEST_TEST_PATH),
  })),
];

const resolveConfigPath = (init) => {
  if (typeof init.config_path !== "string") return undefined;
  const configPath = resolve(init.root, init.config_path);
  if (!existsSync(configPath)) {
    throw new CompanionError("config-load-failed", `config file not found: ${init.config_path}`);
  }
  return configPath;
};

/**
 * Starts Vitest as the Vitest CLI does from the project root. The process
 * goes to the project root first, and the options do not set `root`. A
 * `root` option is a CLI override: it replaces the `root` and `test.root` of
 * the config, so a config with its own root lists no test files. Without the
 * option, Vitest finds the config from the working directory, resolves a
 * relative config root against it, and uses it as the root when the config
 * sets none. The protocol paths stay relative to `init.root`, because the
 * adapter maps each path from its absolute module id.
 */
const createVitestInstance = async ({ createVitest, version, init, plugin }) => {
  const configPath = resolveConfigPath(init);
  process.chdir(init.root);
  try {
    // Vitest 4 crashes on an empty reporter list, so one empty reporter
    // object keeps the console quiet on both majors.
    return await createVitest(
      "test",
      {
        config: configPath,
        watch: false,
        reporters: [{}],
        fileParallelism: false,
        maxWorkers: 1,
        bail: 0,
        includeTaskLocation: true,
        coverage: { enabled: false },
        ...moduleCacheOptions(version),
      },
      { plugins: [plugin] },
      { stdout: process.stderr, stderr: process.stderr },
    );
  } catch (error) {
    throw new CompanionError(
      "config-load-failed",
      `cannot load the Vitest config: ${error.message}`,
    );
  }
};

/**
 * Gives each project the active mutant and its loop budget. A run without a
 * budget (a dry run or a control run) provides null, so a worker never keeps
 * the budget of an earlier run.
 */
const provideActiveMutant = (vitest, mutantId, loopBudgetMs) => {
  for (const project of vitest.projects) {
    project.provide(ACTIVE_MUTANT_KEY, mutantId);
    project.provide(LOOP_BUDGET_KEY, loopBudgetMs);
  }
};

/**
 * Gives each project the overlay as a plain object, keyed by the real path
 * of each source file. The setup file serves it to a native `require`, which
 * the Vite overlay plugin never sees.
 */
const provideOverlay = (vitest, overlay) => {
  const entries = Object.fromEntries(
    [...overlay].map(([path, { source, target }]) => [path, { source, target }]),
  );
  for (const project of vitest.projects) {
    project.provide(OVERLAY_KEY, entries);
  }
};

/**
 * Gives each project the URL of the subprocess preload, with the path of the
 * overlay file. The setup file adds it to NODE_OPTIONS with the active mutant,
 * so a Node process that a test starts also loads the instrumented files.
 */
const provideSubprocessPreload = (vitest, overlayPath) => {
  const url = new URL(SUBPROCESS_PRELOAD);
  url.searchParams.set("overlay", overlayPath);
  for (const project of vitest.projects) {
    project.provide(SUBPROCESS_PRELOAD_KEY, url.href);
  }
};

/**
 * Runs `task` with an empty integrity log of the overlay file. A Node child
 * of a test writes to the log when it cannot serve an instrumented file for
 * certain, for example a copied source that matches two files. Such a run
 * cannot give a trusted verdict, so an entry in the log is an error.
 */
const withOverlayCheck = async (overlayPath, task) => {
  const errorsPath = overlayErrorsPath(overlayPath);
  writeFileSync(errorsPath, "");
  const result = await task();
  const incomplete = readFileSync(errorsPath, "utf8").trim();
  if (incomplete.length > 0) throw new CompanionError("overlay-incomplete", incomplete);
  return result;
};

const hasOverlay = (project) =>
  project.vite.config.plugins.some((plugin) => plugin.name === OVERLAY_PLUGIN);

/**
 * True for a project from a config file other than the root config. Vitest 5
 * gives an inline project the root config file, and Vitest 4 gives it none.
 */
const isFileBased = (project, rootConfigFile) => {
  const configFile = project.vite.config.configFile;
  return typeof configFile === "string" && configFile !== rootConfigFile;
};

const projectNames = (projects) =>
  projects.map((project) => JSON.stringify(project.name)).join(", ");

/** Gives the reason of each group of projects without the overlay. */
const uncoveredReasons = (uncovered, rootConfigFile) => {
  const fileBased = uncovered.filter((project) => isFileBased(project, rootConfigFile));
  const inline = uncovered.filter((project) => !isFileBased(project, rootConfigFile));
  const reasons = [];
  if (fileBased.length > 0) {
    reasons.push(
      `Vitest file-based project ${projectNames(fileBased)} comes from a separate config file and cannot load the instrumented files; define the project inline in the root config`,
    );
  }
  if (inline.length > 0) {
    reasons.push(
      `Vitest inline project ${projectNames(inline)} did not get the companion overlay plugin and cannot load the instrumented files`,
    );
  }
  return reasons;
};

/**
 * Throws when a project does not serve the instrumented files. A project
 * without the overlay runs the original code for each mutant, so its tests
 * can never detect one. For example, Vitest does not give the companion
 * plugins to a project from a separate config file.
 */
const assertOverlayInEachProject = async (vitest) => {
  const uncovered = vitest.projects.filter((project) => !hasOverlay(project));
  if (uncovered.length === 0) return;
  const reasons = uncoveredReasons(uncovered, vitest.vite.config.configFile);
  await vitest.close();
  throw new CompanionError("config-load-failed", reasons.join("; "));
};

/**
 * Lists the facts of the runner setup that limit the result, as kebab-case
 * notes. `typecheck-not-run`: a project enables the Vitest type-check pass,
 * but the companion does not run it, so a type error never detects a mutant.
 */
const runnerNotes = (projects) =>
  projects.some((project) => project.config.typecheck?.enabled === true)
    ? ["typecheck-not-run"]
    : [];

/**
 * Sets the project options that each run of the companion needs: the setup
 * file of the runtime first, and no retry. A retry can turn a failed first
 * attempt into a pass. A mutant run then gives `survived` although a test
 * detected the mutant, and a dry run hides a flaky baseline test.
 */
const forceRunnerOptions = (vitest) => {
  for (const project of vitest.projects) {
    project.config.setupFiles = [SETUP_FILE, ...project.config.setupFiles];
    project.config.retry = 0;
  }
};

/** Gives the hook timeout of a run: never shorter than `timeout_ms`, and no limit stays no limit. */
const runHookTimeout = (projectHook, timeoutMs) =>
  hasTimeoutLimit(projectHook) ? Math.max(projectHook, timeoutMs ?? 0) : projectHook;

/**
 * Sets the test and hook timeouts of each project for one run. Vitest
 * serializes the project config for the workers at each run, so a changed
 * timeout applies to the next run.
 *
 * A run sets the test timeout to `timeout_ms`. A hook keeps the project hook
 * timeout when it is longer: `timeout_ms` comes from the test durations of
 * the dry run, which do not include a suite hook such as `beforeAll`. A
 * shorter hook timeout would report a slow hook as a kill. The API watchdog
 * stays the hard bound of a run. A dry run (`timeoutMs` null) runs the
 * instrumented code, so it uses the project values times
 * `DRY_RUN_TIMEOUT_FACTOR`. Vitest reads a timeout of 0 as no limit, so a
 * project timeout that is not a positive finite number stays as it is.
 */
const createTimeoutControl = (projects) => {
  const original = new Map(
    projects.map((project) => [
      project,
      { test: project.config.testTimeout, hook: project.config.hookTimeout },
    ]),
  );
  return (timeoutMs) => {
    for (const [project, values] of original) {
      if (timeoutMs === null) {
        project.config.testTimeout = dryRunTimeout(values.test);
        project.config.hookTimeout = dryRunTimeout(values.hook);
        continue;
      }
      project.config.testTimeout = timeoutMs;
      project.config.hookTimeout = runHookTimeout(values.hook, timeoutMs);
    }
  };
};

/**
 * Turns module isolation off for a run, so that Vitest runs the test files of
 * the run in one worker. Each run still starts a new worker, so the modules
 * stay fresh for each mutant. A project in browser mode keeps its value.
 * `shared: false` gives each project its own value back.
 */
const createIsolationControl = (projects) => {
  const original = new Map(projects.map((project) => [project, project.config.isolate]));
  return (shared) => {
    for (const [project, isolate] of original) {
      const browser = project.config.browser?.enabled === true;
      project.config.isolate = shared && !browser ? false : isolate;
    }
  };
};

// A reused worker reads `testTimeout` only when it starts. A run rounds its
// timeout up to a multiple of this step, so that most runs keep the process.
// Rounding up never ends a test earlier than `timeout_ms`, and the step stays
// below the 2 second grace of the watchdog of the API.
const REUSED_TIMEOUT_STEP_MS = 1000;

const reusedTimeout = (timeoutMs) =>
  timeoutMs === null
    ? null
    : Math.ceil(timeoutMs / REUSED_TIMEOUT_STEP_MS) * REUSED_TIMEOUT_STEP_MS;

/**
 * Switches each `forks` or `threads` project to a reused pool for a run, and
 * back to its own pool for each other run. Each project gets its own process,
 * because the worker reads the project config at start.
 */
const createReuseControl = (projects, workers) => {
  const entries = projects.flatMap((project) => {
    const pool = project.config.pool;
    const WorkerClass = workers?.[pool];
    if (typeof WorkerClass !== "function" || project.config.browser?.enabled === true) return [];
    return [
      {
        project,
        pool,
        poolRunner: project.config.poolRunner,
        reused: createReusedPool(REUSED_POOLS[pool], WorkerClass),
      },
    ];
  });
  return {
    available: entries.length > 0,
    select: async (reused) => {
      for (const entry of entries) {
        const { project, reused: pool } = entry;
        if (reused) {
          const { testTimeout, hookTimeout, bail } = project.config;
          await pool.prepare(JSON.stringify([testTimeout, hookTimeout, bail]));
        }
        project.config.pool = reused ? pool.name : entry.pool;
        project.config.poolRunner = reused ? pool : entry.poolRunner;
        project.provide(FRESH_MODULES_KEY, reused);
      }
    },
    recycle: async () => {
      for (const entry of entries) await entry.reused.close();
    },
    close: async () => {
      for (const entry of entries) await entry.reused.close();
    },
  };
};

const DETECTED = new Set(["killed", "timeout"]);

// The longest isolated dry run that still gets the check for shared runs.
// The check repeats the dry run, and the API waits at most `--setup-timeout`
// (2 minutes by default) for the first dry run of a companion.
const SHARED_CHECK_MAX_DRY_RUN_MS = 45_000;

/** The start time of a test module in its worker, or infinity when unknown. */
const startTime = (testModule) => testModule.task?.result?.startTime ?? Number.POSITIVE_INFINITY;

/**
 * Gives the status of each test of a run, keyed by module, test name and
 * the position among the tests of the module with the same name.
 */
const statusByTest = (result) => {
  const statuses = new Map();
  for (const testModule of result.testModules) {
    const seen = new Map();
    for (const testCase of testModule.children.allTests()) {
      const name = `${moduleKey(testModule.project, testModule.moduleId)}\u0000${testCase.fullName}`;
      const position = seen.get(name) ?? 0;
      seen.set(name, position + 1);
      statuses.set(`${name}\u0000${position}`, testStatus(testCase));
    }
  }
  return statuses;
};

/** Gives the errors outside a test of a run as a set of file and message. */
const errorSet = (result, root) =>
  new Set(runErrors(result, root).map(({ file, message }) => `${file}\u0000${message}`));

/**
 * True when a run in one shared worker gives each test the status of the
 * isolated run, with no extra error outside a test. A test that depends on a
 * fresh worker for each file (for example a global that another file sets)
 * makes the two runs differ.
 */
const sameOutcome = (isolated, shared, root) => {
  const left = statusByTest(isolated);
  const right = statusByTest(shared);
  if (left.size !== right.size) return false;
  for (const [key, status] of left) if (right.get(key) !== status) return false;
  const isolatedErrors = errorSet(isolated, root);
  return [...errorSet(shared, root)].every((error) => isolatedErrors.has(error));
};

/**
 * Sets the fail-fast value of a run on the root config and on each project.
 * The root value controls the cancellation of the pool. A worker reads the
 * serialized project config, and Vitest copies the root `bail` value into an
 * inline project only when it creates the project. Each run sets the value
 * again, so a run without bail never keeps the value of a mutant run.
 */
const setBail = (vitest, bail) => {
  const value = bail ? 1 : 0;
  vitest.config.bail = value;
  for (const project of vitest.projects) {
    project.config.bail = value;
  }
};

const groupByFile = (specifications, root) => {
  const byFile = new Map();
  for (const specification of specifications) {
    const file = toPosix(relative(realPath(root), realPath(specification.moduleId)));
    byFile.set(file, [...(byFile.get(file) ?? []), specification]);
  }
  return byFile;
};

const requestedFiles = (files, byFile) =>
  files === undefined || files.length === 0
    ? [...byFile.keys()]
    : [...new Set(files.map((file) => toPosix(file)))];

/**
 * Selects the specifications of the test files as `[file, specification]`
 * pairs, ordered by file and then by project name. A file that more than one
 * project includes gives one pair for each project.
 */
const selectSpecifications = (specifications, root, files) => {
  const byFile = groupByFile(specifications, root);
  return requestedFiles(files, byFile)
    .toSorted(byCodeUnit)
    .flatMap((file) => {
      const matches = byFile.get(file);
      if (matches === undefined) {
        throw new CompanionError("test-file-missing", `not a Vitest test file: ${file}`);
      }
      return matches
        .toSorted((left, right) => byCodeUnit(left.project.name, right.project.name))
        .map((specification) => [file, specification]);
    });
};

/**
 * Keeps the test modules of one run in the order of its specifications. The
 * Vitest state also keeps the test modules of earlier runs.
 */
const modulesOfRun = (testModules, specifications) => {
  const order = new Map(
    specifications.map((specification, index) => [
      moduleKey(specification.project, specification.moduleId),
      index,
    ]),
  );
  const position = (testModule) => order.get(moduleKey(testModule.project, testModule.moduleId));
  return testModules
    .filter((testModule) => position(testModule) !== undefined)
    .toSorted((left, right) => position(left) - position(right));
};

const metaList = (meta, key) => (Array.isArray(meta[key]) ? meta[key] : []);

const testLine = (testCase) => testCase.location?.line ?? null;

const testDuration = (testCase) => testCase.diagnostic()?.duration ?? null;

const SETUP_PHASES = [
  "environmentSetupDuration",
  "prepareDuration",
  "collectDuration",
  "setupDuration",
  "duration",
];

/**
 * Gives the time of a test module outside its tests: environment setup,
 * module load and suite hooks such as `beforeAll`. The API adds it to the
 * watchdog wait of a run of the file, because `timeout_ms` covers only the
 * test durations.
 */
const moduleSetupMs = (testModule, entries) => {
  const diagnostic = testModule.diagnostic() ?? {};
  const total = SETUP_PHASES.reduce((sum, key) => sum + (diagnostic[key] ?? 0), 0);
  const tests = entries.reduce((sum, entry) => sum + (entry.report.duration_ms ?? 0), 0);
  return Math.max(0, total - tests);
};

/**
 * The failure fields of a failed test: its failure kind, and its first error
 * when it has one. A dry-run entry and a run entry carry the same fields.
 */
const failureFields = (testCase, directories) => {
  const fields = { failure_kind: failureKind(testCase) };
  const message = testErrorText(testCase.result().errors?.[0], directories);
  return message === null ? fields : { ...fields, message };
};

/** The dry-run report entry of one test, with the failure fields of a failed test. */
const dryRunReport = (testCase, base, directories) => {
  const report = { ...base, status: testStatus(testCase) ?? "skip" };
  return report.status === "fail" ? { ...report, ...failureFields(testCase, directories) } : report;
};

/** Reads each test of one dry-run module with its hits, its loaded paths and its report entry. */
const readDryRunTests = (testModule, file, prefix, directories) => {
  const testCases = [...testModule.children.allTests()];
  const fullNames = testCases.map((testCase) => testCase.fullName);
  const names = uniqueNames(fullNames);
  const repeated = repeatedNameFlags(
    fullNames,
    testCases.map((testCase) => hasRun(testStatus(testCase))),
  );
  return testCases.map((testCase, index) => {
    const meta = testCase.meta();
    return {
      taskId: testCase.id,
      repeated: repeated[index],
      shared: isConcurrent(testCase),
      hits: metaList(meta, "fallowHits"),
      staticHits: metaList(meta, "fallowStaticHits"),
      loaded: metaList(meta, "fallowLoaded"),
      report: dryRunReport(
        testCase,
        {
          test_id: `${prefix}${file}${TEST_ID_SEPARATOR}${names[index]}`,
          file,
          name: names[index],
          line: testLine(testCase),
          duration_ms: testDuration(testCase),
        },
        directories,
      ),
    };
  });
};

/** The entry of a failed test, with its first error when it has one. */
const failedTestEntry = (testCase, testId, directories) => ({
  test_id: testId,
  status: "fail",
  ...failureFields(testCase, directories),
});

/** Gives the report entry of one test of a run, or no entry when it did not run. */
const runTestEntry = (testCase, testId, directories) => {
  const status = testStatus(testCase);
  if (testId === undefined || !hasRun(status)) return [];
  return status === "fail"
    ? [failedTestEntry(testCase, testId, directories)]
    : [{ test_id: testId, status }];
};

const collectRunTests = (testModules, testIdByTask, directories) =>
  testModules.flatMap((testModule) =>
    [...testModule.children.allTests()].flatMap((testCase) =>
      runTestEntry(testCase, testIdByTask.get(testCase.id), directories),
    ),
  );

/**
 * Sets the variables that the Vitest CLI sets before it calls `createVitest`.
 * The adapter calls `createVitest` directly, so without this step Vite sets
 * `NODE_ENV=development` and the test workers inherit it. A `NODE_ENV` value
 * of the environment stays, as with the CLI. The `options.env` values of the
 * init request come after this step, so they win.
 */
const setVitestEnv = () => {
  process.env.TEST = "true";
  process.env.VITEST = "true";
  process.env.NODE_ENV ??= "test";
};

/**
 * The project that loads a replay file: a project in Node (not browser mode)
 * whose root holds the file, the deepest root first. Without such a project,
 * the first project in Node, else the first project.
 */
const holdsPath = (root, path) => {
  const rel = relative(root, path);
  return rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
};

const replayProject = (projects, path) => {
  const inNode = projects.filter((project) => project.config.browser?.enabled !== true);
  const holding = inNode
    .filter((project) => holdsPath(project.config.root, path))
    .toSorted((left, right) => right.config.root.length - left.config.root.length);
  return holding[0] ?? inNode[0] ?? projects[0];
};

/**
 * Creates the Vitest adapter for an `init` request. It starts one Vitest
 * instance with no file parallelism and one worker, and keeps the project
 * `isolate` setting.
 *
 * The companion reads this export from the `import()` namespace that
 * `loadAdapter` returns. The `ignoreExports` entry of the Fallow config
 * keeps it out of the unused-export and duplicate-export findings.
 */
export const createAdapter = async (init) => {
  validateInit(init);
  const root = resolve(init.root);
  const { createVitest, version, workers } = await loadProjectVitest(root);
  const overlay = buildOverlay(root, init.run_dir, init.path_map);
  const directories = errorDirectories(root, init.run_dir);
  const loaded = new Set();
  setVitestEnv();
  Object.assign(process.env, init.options?.env ?? {});

  const vitest = await createVitestInstance({
    createVitest,
    version,
    init: { ...init, root },
    plugin: overlayPlugin(overlay, loaded),
  });
  await assertOverlayInEachProject(vitest);
  forceRunnerOptions(vitest);
  provideOverlay(vitest, overlay);
  const workDir = createWorkDir("fallow-mutation-vitest-", overlay);
  provideSubprocessPreload(vitest, workDir.overlayPath);
  const freshModulesPerRun = vitest.projects.every((project) => project.config.isolate !== false);
  const setTimeouts = createTimeoutControl(vitest.projects);

  const allSpecifications = await vitest.globTestSpecifications();
  const initFiles = selectSpecifications(allSpecifications, root, init.options?.test_files);
  const testIndex = new Map();
  // With more than one project, a test id names its project, so that a file
  // that two projects include gives a distinct id for each project.
  const testIdPrefix = (project) =>
    vitest.projects.length > 1 ? `${project.name}${TEST_ID_SEPARATOR}` : "";

  const setShared = createIsolationControl(vitest.projects);
  // Mutant and control runs share one worker for their test files only after
  // a dry run proves that the tests give the same results that way.
  let sharedRuns = false;
  // Mutant runs keep one worker process only after a dry run proves that the
  // tests give the same results in a reused process, also on a second load.
  let reusedRuns = false;
  const reuse = createReuseControl(vitest.projects, workers);
  const runSpecifications = async (
    specifications,
    {
      mutantId,
      bail,
      timeoutMs = null,
      loopBudgetMs = null,
      shared = false,
      reuseProcess = null,
      recordArgs = false,
    },
  ) => {
    provideActiveMutant(vitest, mutantId, loopBudgetMs);
    for (const project of vitest.projects) project.provide(RECORD_ARGS_KEY, recordArgs);
    // A mutant run in a shared worker keeps the worker process for the next
    // mutant run. Control and dry runs, and runs without bail, start a new
    // one. The dry-run check passes `reuseProcess` to test the reused process.
    const reused = reuseProcess ?? (reusedRuns && shared && bail && mutantId !== null);
    setTimeouts(reused ? reusedTimeout(timeoutMs) : timeoutMs);
    setBail(vitest, bail);
    setShared(shared);
    await reuse.select(reused);
    // Vitest copies the pool of the project into a specification when it
    // creates it, so a caller can create them only after the pool is set.
    const specs = typeof specifications === "function" ? specifications() : specifications;
    const result = await withOverlayCheck(workDir.overlayPath, () =>
      vitest.runTestSpecifications(specs),
    );
    return {
      testModules: modulesOfRun(result.testModules, specs),
      unhandledErrors: result.unhandledErrors ?? [],
    };
  };

  const dryRunSelection = (files) =>
    files === undefined || files.length === 0
      ? initFiles
      : selectSpecifications(allSpecifications, root, files);

  /** Adds the tests of one dry-run module to the test index and the result. */
  const recordDryRunModule = (testModule, file, summary) => {
    const entries = readDryRunTests(
      testModule,
      file,
      testIdPrefix(testModule.project),
      directories,
    );
    for (const entry of entries) {
      for (const source of entry.loaded) loaded.add(source);
      testIndex.set(entry.report.test_id, {
        project: testModule.project,
        moduleId: testModule.moduleId,
        taskId: entry.taskId,
      });
      for (const id of entry.staticHits) summary.staticHits.add(id);
      if (entry.repeated) summary.duplicates.add(entry.report.test_id);
      summary.tests.push(entry.report);
    }
    Object.assign(summary.hits, Object.fromEntries(attributeHits(entries)));
    summary.setup[file] = (summary.setup[file] ?? 0) + moduleSetupMs(testModule, entries);
  };

  /**
   * Runs the dry-run files once more in one shared worker and compares the
   * outcome with the isolated dry run. A failure of the shared run turns
   * sharing off and keeps the dry run.
   */
  const sharesLikeIsolated = async (specifications, isolated) => {
    try {
      const shared = await runSpecifications(specifications, {
        mutantId: null,
        bail: false,
        shared: true,
      });
      return sameOutcome(isolated, shared, root);
    } catch {
      return false;
    }
  };

  /**
   * Decides how later runs use worker processes, from the outcome of the
   * isolated dry run. The dry-run files run again in one reused process: the
   * first round shares one worker for all files in a fresh process, and the
   * second round loads each module a second time in the same process, which
   * shows a module that fails when it loads again. A first round that
   * differs falls back to the plain shared check. The rounds run without
   * bail, so they get the status of each test. Mutant runs use bail, so they
   * start a new process.
   */
  const workerModes = async (specifications, isolated) => {
    if (!reuse.available) {
      return { shared: await sharesLikeIsolated(specifications, isolated), reused: false };
    }
    const again = () =>
      specifications.map((specification) =>
        specification.project.createSpecification(specification.moduleId),
      );
    const round = async () => {
      try {
        const reused = await runSpecifications(again, {
          mutantId: null,
          bail: false,
          shared: true,
          reuseProcess: true,
        });
        return sameOutcome(isolated, reused, root);
      } catch {
        return false;
      }
    };
    try {
      if (!(await round())) {
        await reuse.recycle();
        return { shared: await sharesLikeIsolated(specifications, isolated), reused: false };
      }
      return { shared: true, reused: await round() };
    } finally {
      await reuse.recycle();
    }
  };

  const dryRun = async (request) => {
    validateDryRun(request);
    const selected = dryRunSelection(request.test_files);
    const started = performance.now();
    const specifications = selected.map(([, specification]) => specification);
    const result = await runSpecifications(specifications, { mutantId: null, bail: false });
    const checked =
      freshModulesPerRun &&
      specifications.length > 1 &&
      performance.now() - started <= SHARED_CHECK_MAX_DRY_RUN_MS;
    const modes = checked
      ? await workerModes(specifications, result)
      : { shared: false, reused: false };
    sharedRuns = modes.shared;
    reusedRuns = modes.reused;
    const fileByModule = new Map(
      selected.map(([file, spec]) => [moduleKey(spec.project, spec.moduleId), file]),
    );
    const summary = {
      tests: [],
      hits: {},
      staticHits: new Set(),
      duplicates: new Set(),
      setup: {},
    };
    for (const testModule of result.testModules) {
      const file = fileByModule.get(moduleKey(testModule.project, testModule.moduleId));
      recordDryRunModule(testModule, file, summary);
    }
    return dryRunResult({
      started,
      summary,
      errors: runErrors(result, root),
      pathMap: init.path_map,
      loaded,
    });
  };

  /**
   * Runs the dry-run test files once more with the recorder on, and gives
   * the recorded arguments. The test results of this pass count for
   * nothing, so a test that the wraps change never changes a run.
   */
  const record = async (request) => {
    validateRecord(request);
    const started = performance.now();
    const specifications = dryRunSelection(request.test_files).map(([, spec]) => spec);
    const result = await runSpecifications(specifications, {
      mutantId: null,
      bail: false,
      recordArgs: true,
    });
    const records = new Map();
    for (const testModule of result.testModules) {
      for (const testCase of testModule.children.allTests()) {
        addRecords(records, testCase.meta().fallowArgs);
      }
    }
    return {
      recorded_args: recordEntries(records),
      duration_ms: Math.round(performance.now() - started),
    };
  };

  /** Groups the requested tests into one specification for each test module. */
  const specificationsFor = (requested) => {
    const groups = new Map();
    const testIdByTask = new Map();
    for (const testId of requested) {
      const entry = testIndex.get(testId);
      expectRequest(entry !== undefined, `unknown test id (send dry_run first): ${testId}`);
      testIdByTask.set(entry.taskId, testId);
      const key = moduleKey(entry.project, entry.moduleId);
      const group = groups.get(key) ?? { ...entry, taskIds: [] };
      group.taskIds.push(entry.taskId);
      groups.set(key, group);
    }
    const specifications = [...groups.values()].map((group) =>
      group.project.createSpecification(group.moduleId, { testIds: group.taskIds }),
    );
    return { specifications, testIdByTask };
  };

  /** Builds the `run_result` body of one run of `requested`. */
  const describeRun = (request, requested, testIdByTask, result, started) => {
    const firstFailure = result.testModules
      .flatMap((testModule) => [...testModule.children.allTests()])
      .find((testCase) => testIdByTask.has(testCase.id) && testStatus(testCase) === "fail");
    return {
      firstFailure,
      body: runResult({
        mutantId: request.mutant_id,
        tests: collectRunTests(result.testModules, testIdByTask, directories),
        errors: runErrors(result, root),
        requestedCount: requested.length,
        durationMs: performance.now() - started,
        // Only the thread CPU clock makes a loop timeout final. Host load can
        // use wall time, so a wall-clock loop timeout gets the normal retry.
        loopTimeout: firstFailure !== undefined && hasErrorNamed(firstFailure, [LOOP_TIMEOUT_NAME]),
      }),
    };
  };

  const runRequested = async (request, requested, shared) => {
    let testIdByTask;
    const specifications = () => {
      const selected = specificationsFor(requested);
      testIdByTask = selected.testIdByTask;
      return selected.specifications;
    };
    const started = performance.now();
    const result = await runSpecifications(specifications, {
      mutantId: request.mutant_id,
      bail: request.bail,
      timeoutMs: request.timeout_ms,
      loopBudgetMs: request.loop_budget_ms ?? null,
      shared,
    });
    return { result, ...describeRun(request, requested, testIdByTask, result, started) };
  };

  /**
   * The requested tests of the module that decided a kill in a shared run,
   * or null when that module ran first in the worker. A module that ran
   * first had a fresh worker, as in an isolated run.
   */
  const laterDecidingTests = (requested, result, firstFailure, polluted) => {
    const failing = result.testModules.filter(
      (testModule) =>
        moduleErrors(testModule, root).length > 0 ||
        [...testModule.children.allTests()].some((testCase) => testStatus(testCase) === "fail"),
    );
    if (firstFailure === undefined && failing.length === 0) return null;
    // The module that failed first in the worker decided the status.
    const deciding = failing.reduce(
      (earliest, testModule) =>
        earliest === undefined || startTime(testModule) < startTime(earliest)
          ? testModule
          : earliest,
      undefined,
    );
    if (deciding === undefined) return null;
    if (!polluted) {
      if (result.testModules.length < 2) return null;
      const first = result.testModules.reduce((earliest, testModule) =>
        startTime(testModule) < startTime(earliest) ? testModule : earliest,
      );
      if (first === deciding) return null;
    }
    const key = moduleKey(deciding.project, deciding.moduleId);
    const tests = requested.filter((testId) => {
      const entry = testIndex.get(testId);
      return moduleKey(entry.project, entry.moduleId) === key;
    });
    return tests.length > 0 ? tests : null;
  };

  const run = async (request) => {
    validateRun(request);
    const requested = [...new Set(request.test_ids)];
    const outcome = await runRequested(request, requested, sharedRuns);
    // A reused worker whose tests added a global or changed the environment
    // gets a new process, so that the next mutant starts clean.
    const polluted = outcome.result.testModules.some((testModule) =>
      [...testModule.children.allTests()].some((testCase) => testCase.meta().fallowPolluted),
    );
    if (polluted) await reuse.recycle();
    if (!sharedRuns || !DETECTED.has(outcome.body.status)) return outcome.body;
    // A kill with no failing test (an error outside each test) can come from
    // a timer or a promise of an earlier run of a reused process.
    if (reusedRuns && outcome.firstFailure === undefined) {
      return (await runRequested(request, requested, false)).body;
    }
    // A mutant can leave state in a module that a later test file of the
    // shared worker reads, which an isolated run never shows. A kill in a
    // later file, or in a worker with changed globals, must also kill in a
    // fresh worker, or the run is repeated with each file in its own worker.
    const later = laterDecidingTests(requested, outcome.result, outcome.firstFailure, polluted);
    if (later === null) return outcome.body;
    const alone = await runRequested(request, later, false);
    if (DETECTED.has(alone.body.status)) return outcome.body;
    return (await runRequested(request, requested, false)).body;
  };

  /** Runs one replay call as the only test of a run, in a new worker. */
  const replayCall = async (project, request, call) => {
    const testTimeoutMs = request.call_timeout_ms + REPLAY_TEST_MARGIN_MS;
    for (const each of vitest.projects) {
      each.provide(REPLAY_KEY, { ...request, ...call, test_timeout_ms: testTimeoutMs });
    }
    const result = await runSpecifications(() => [project.createSpecification(REPLAY_FILE)], {
      mutantId: null,
      bail: false,
      timeoutMs: testTimeoutMs,
    });
    const testCase = result.testModules.flatMap((testModule) => [
      ...testModule.children.allTests(),
    ])[0];
    const replayed = testCase?.meta().fallowReplay;
    if (isPlainObject(replayed)) return replayed;
    const errors = [
      ...runErrors(result, root).map((error) => error.message),
      ...(testCase?.result().errors ?? []).map((error) => errorMessage(error)),
    ];
    throw new CompanionError(
      "internal",
      `the replay test gave no result${errors.length > 0 ? `: ${errors.join("; ")}` : ""}`,
    );
  };

  /**
   * Runs each call of the replay in its own run, so each call has a new
   * worker: globals, the environment and leftover timers of one call never
   * reach the next call or the other side.
   */
  const replay = async (request) => {
    validateReplay(request);
    const entry = replayEntry(overlay, request.file);
    const project = replayProject(vitest.projects, entry.path);
    const started = performance.now();
    const calls = [];
    for (let index = 1; index <= request.calls; index += 1) {
      for (const side of SIDES) {
        const call = await replayCall(project, request, { side, module_path: entry.path });
        calls.push({ side, index, ...call });
      }
    }
    return { calls, duration_ms: Math.round(performance.now() - started) };
  };

  return {
    runnerVersion: version,
    testFiles: [...new Set(initFiles.map(([file]) => file))],
    capabilities: {
      per_test_attribution: true,
      test_filter: true,
      hot_switch: true,
      bail: true,
      fresh_modules_per_run: freshModulesPerRun,
      batch_mutants: false,
      replay: true,
      record: true,
    },
    notes: runnerNotes(vitest.projects),
    dryRun,
    run,
    replay,
    record,
    reload: async () => ({ method: freshModulesPerRun ? "noop" : "restart_required" }),
    close: async () => {
      try {
        await reuse.close();
        await vitest.close();
      } finally {
        workDir.remove();
      }
    },
  };
};
