import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { CompanionError, expectRequest, isPlainObject } from "../protocol.mjs";
import {
  TEST_ID_SEPARATOR,
  attributeHits,
  buildOverlay,
  byCodeUnit,
  dryRunResult,
  errorMessage,
  hasRun,
  isSupportedVersion,
  parseVersion,
  realPath,
  runResult,
  toPosix,
  uniqueNames,
  unsupportedVersion,
  validateDryRun,
  validateInit,
  validateRun,
} from "./common.mjs";

const RUNNER_NAME = "vitest";
const ACTIVE_MUTANT_KEY = "fallowActiveMutant";
const SETUP_FILE = fileURLToPath(new URL("../runtime/vitest-setup.mjs", import.meta.url));
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
    return { createVitest: node.createVitest, version };
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

/** The overlay entry of a plain request for a component file, or undefined. */
const componentEntry = (overlay, id) => {
  if (!isPlainRequest(id)) return undefined;
  const entry = overlay.get(realPath(id));
  return entry !== undefined && isComponent(entry.source) ? entry : undefined;
};

/**
 * Serves the instrumented copy of each mapped file. A script file gets its
 * copy in the `transform` hook. A component file gets its copy in the `load`
 * hook, so the framework plugin compiles the instrumented component also when
 * its own `transform` hook runs first. A component request with a query (a
 * style or a template block) is not served: the framework plugin makes it
 * from the component that it compiled. The `config` hook also
 * adds this plugin to each inline project definition. Vitest 4 gives the
 * plugins of `createVitest` to the root project only. Vitest 5 does not give
 * them to an inline project with a `browser` block, also when browser mode
 * is off. Vite keeps one copy of a plugin that a project gets twice.
 */
const overlayPlugin = (overlay, loaded) => {
  const plugin = {
    name: OVERLAY_PLUGIN,
    enforce: "pre",
    config(config) {
      const projects = config.test?.projects;
      if (!Array.isArray(projects)) return;
      config.test.projects = projects.map((project) =>
        isPlainObject(project)
          ? { ...project, plugins: [...(project.plugins ?? []), plugin] }
          : project,
      );
    },
    load(id) {
      const entry = componentEntry(overlay, id);
      if (entry === undefined) return null;
      loaded.add(entry.source);
      return readFileSync(entry.target, "utf8");
    },
    transform(_code, id) {
      if (id.startsWith("\0")) return null;
      const entry = overlay.get(realPath(id.split("?", 1)[0]));
      if (entry === undefined || isComponent(entry.source)) return null;
      loaded.add(entry.source);
      return {
        code: readFileSync(entry.target, "utf8"),
        map: readSourceMap(`${entry.target}.map`),
      };
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

const failureKind = (testCase) => {
  const errors = testCase.result().errors ?? [];
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

const createVitestInstance = async ({ createVitest, version, init, plugin }) => {
  const configPath = resolveConfigPath(init);
  try {
    // Vitest 4 crashes on an empty reporter list, so one empty reporter
    // object keeps the console quiet on both majors.
    return await createVitest(
      "test",
      {
        root: init.root,
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

const provideActiveMutant = (vitest, mutantId) => {
  for (const project of vitest.projects) {
    project.provide(ACTIVE_MUTANT_KEY, mutantId);
  }
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

/** True for a hook timeout that sets a limit. Vitest reads 0 as no limit. */
const hasHookLimit = (value) => Number.isFinite(value) && value > 0;

/** Gives the hook timeout of a run: never shorter than `timeout_ms`, and no limit stays no limit. */
const runHookTimeout = (projectHook, timeoutMs) =>
  hasHookLimit(projectHook) ? Math.max(projectHook, timeoutMs ?? 0) : projectHook;

/**
 * Sets the test and hook timeouts of each project for one run. Vitest
 * serializes the project config for the workers at each run, so a changed
 * timeout applies to the next run.
 *
 * A run sets the test timeout to `timeout_ms`. A hook keeps the project hook
 * timeout when it is longer: `timeout_ms` comes from the test durations of
 * the dry run, which do not include a suite hook such as `beforeAll`. A
 * shorter hook timeout would report a slow hook as a kill. The API watchdog
 * stays the hard bound of a run. A dry run (`timeoutMs` null) uses the
 * project values. Vitest reads a hook timeout of 0 as no limit, so a project
 * hook timeout that is not a positive finite number stays as it is.
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
      project.config.testTimeout = timeoutMs ?? values.test;
      project.config.hookTimeout = runHookTimeout(values.hook, timeoutMs);
    }
  };
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

/** Reads each test of one dry-run module with its hits and its report entry. */
const readDryRunTests = (testModule, file, prefix) => {
  const testCases = [...testModule.children.allTests()];
  const names = uniqueNames(testCases.map((testCase) => testCase.fullName));
  return testCases.map((testCase, index) => {
    const meta = testCase.meta();
    return {
      taskId: testCase.id,
      shared: isConcurrent(testCase),
      hits: metaList(meta, "fallowHits"),
      staticHits: metaList(meta, "fallowStaticHits"),
      report: {
        test_id: `${prefix}${file}${TEST_ID_SEPARATOR}${names[index]}`,
        file,
        name: names[index],
        line: testLine(testCase),
        duration_ms: testDuration(testCase),
        status: testStatus(testCase) ?? "skip",
      },
    };
  });
};

/** Gives the report entry of one test of a run, or no entry when it did not run. */
const runTestEntry = (testCase, testId) => {
  const status = testStatus(testCase);
  if (testId === undefined || !hasRun(status)) return [];
  return status === "fail"
    ? [{ test_id: testId, status, failure_kind: failureKind(testCase) }]
    : [{ test_id: testId, status }];
};

const collectRunTests = (testModules, testIdByTask) =>
  testModules.flatMap((testModule) =>
    [...testModule.children.allTests()].flatMap((testCase) =>
      runTestEntry(testCase, testIdByTask.get(testCase.id)),
    ),
  );

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
  const { createVitest, version } = await loadProjectVitest(root);
  const overlay = buildOverlay(root, init.run_dir, init.path_map);
  const loaded = new Set();
  Object.assign(process.env, init.options?.env ?? {});

  const vitest = await createVitestInstance({
    createVitest,
    version,
    init: { ...init, root },
    plugin: overlayPlugin(overlay, loaded),
  });
  await assertOverlayInEachProject(vitest);
  forceRunnerOptions(vitest);
  const freshModulesPerRun = vitest.projects.every((project) => project.config.isolate !== false);
  const setTimeouts = createTimeoutControl(vitest.projects);

  const allSpecifications = await vitest.globTestSpecifications();
  const initFiles = selectSpecifications(allSpecifications, root, init.options?.test_files);
  const testIndex = new Map();
  // With more than one project, a test id names its project, so that a file
  // that two projects include gives a distinct id for each project.
  const testIdPrefix = (project) =>
    vitest.projects.length > 1 ? `${project.name}${TEST_ID_SEPARATOR}` : "";

  const runSpecifications = async (specifications, { mutantId, bail, timeoutMs = null }) => {
    provideActiveMutant(vitest, mutantId);
    setTimeouts(timeoutMs);
    vitest.config.bail = bail ? 1 : 0;
    const result = await vitest.runTestSpecifications(specifications);
    return {
      testModules: modulesOfRun(result.testModules, specifications),
      unhandledErrors: result.unhandledErrors ?? [],
    };
  };

  const dryRunSelection = (files) =>
    files === undefined || files.length === 0
      ? initFiles
      : selectSpecifications(allSpecifications, root, files);

  /** Adds the tests of one dry-run module to the test index and the result. */
  const recordDryRunModule = (testModule, file, summary) => {
    const entries = readDryRunTests(testModule, file, testIdPrefix(testModule.project));
    for (const entry of entries) {
      testIndex.set(entry.report.test_id, {
        project: testModule.project,
        moduleId: testModule.moduleId,
        taskId: entry.taskId,
      });
      for (const id of entry.staticHits) summary.staticHits.add(id);
      summary.tests.push(entry.report);
    }
    Object.assign(summary.hits, Object.fromEntries(attributeHits(entries)));
    summary.setup[file] = (summary.setup[file] ?? 0) + moduleSetupMs(testModule, entries);
  };

  const dryRun = async (request) => {
    validateDryRun(request);
    const selected = dryRunSelection(request.test_files);
    const started = performance.now();
    const result = await runSpecifications(
      selected.map(([, specification]) => specification),
      { mutantId: null, bail: false },
    );
    const fileByModule = new Map(
      selected.map(([file, spec]) => [moduleKey(spec.project, spec.moduleId), file]),
    );
    const summary = { tests: [], hits: {}, staticHits: new Set(), setup: {} };
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

  const run = async (request) => {
    validateRun(request);
    const requested = [...new Set(request.test_ids)];
    const { specifications, testIdByTask } = specificationsFor(requested);
    const started = performance.now();
    const result = await runSpecifications(specifications, {
      mutantId: request.mutant_id,
      bail: request.bail,
      timeoutMs: request.timeout_ms,
    });
    return runResult({
      mutantId: request.mutant_id,
      tests: collectRunTests(result.testModules, testIdByTask),
      errors: runErrors(result, root),
      requestedCount: requested.length,
      durationMs: performance.now() - started,
    });
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
    },
    notes: runnerNotes(vitest.projects),
    dryRun,
    run,
    reload: async () => ({ method: freshModulesPerRun ? "noop" : "restart_required" }),
    close: () => vitest.close(),
  };
};
