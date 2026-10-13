import { existsSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { RUNNERS } from "../generated-protocol.mjs";
import { CompanionError, expectRequest, isPlainObject } from "../protocol.mjs";

const RUNNER_NAME = "vitest";
const ACTIVE_MUTANT_KEY = "fallowActiveMutant";
const TEST_ID_SEPARATOR = "::";
const SETUP_FILE = fileURLToPath(new URL("../runtime/vitest-setup.mjs", import.meta.url));
const TIMEOUT_MESSAGE = /\b(?:Test|Hook) timed out in \d+ms/u;
const OVERLAY_PLUGIN = "fallow-mutation-overlay";

const toPosix = (path) => path.split(sep).join("/");

/** Compares two strings by UTF-16 code unit, independent of the host locale. */
const byCodeUnit = (left, right) => (left < right ? -1 : left > right ? 1 : 0);

/** Keys a test module by its project, because two projects can include one file. */
const moduleKey = (project, moduleId) => `${project.name}\0${moduleId}`;

const realPath = (path) => {
  try {
    return realpathSync(path);
  } catch {
    return resolve(path);
  }
};

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

const parseVersion = (version) => {
  const match = /^(\d+)\.(\d+)\.(\d+)/u.exec(version);
  return match ? match.slice(1, 4).map(Number) : null;
};

const compareVersions = (left, right) => {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] - right[index];
  }
  return 0;
};

const isSupportedVersion = (version) => {
  const runner = RUNNERS.find((entry) => entry.name === RUNNER_NAME);
  const parsed = parseVersion(version);
  return (
    parsed !== null &&
    compareVersions(parsed, parseVersion(runner.min_version)) >= 0 &&
    compareVersions(parsed, parseVersion(runner.max_version_exclusive)) < 0
  );
};

const validateInit = (init) => {
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
};

const validateDryRun = (request) => {
  expectRequest(isOptional(request.test_files, isStringList), "test_files must be a list of paths");
};

const validateRun = (request) => {
  expectRequest(
    typeof request.mutant_id === "string" && request.mutant_id.length > 0,
    "mutant_id must be a string",
  );
  expectRequest(isNonEmptyStringList(request.test_ids), "test_ids must be a non-empty list");
  expectRequest(isPositiveInteger(request.timeout_ms), "timeout_ms must be a positive integer");
  expectRequest(typeof request.bail === "boolean", "bail must be a boolean");
};

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
  if (!isSupportedVersion(version)) {
    throw new CompanionError("runner-version", `vitest ${version} is not supported`);
  }
  try {
    const node = await import(pathToFileURL(projectRequire.resolve("vitest/node")).href);
    return { createVitest: node.createVitest, version };
  } catch (error) {
    throw new CompanionError("runner-load-failed", `cannot import vitest/node: ${error.message}`);
  }
};

/**
 * Maps the real path of each project source file to its instrumented copy in
 * the run directory. The comparison uses real paths, so a symlink cannot
 * select a file outside the map.
 */
const buildOverlay = (root, runDir, pathMap) => {
  const realRunDir = realPath(runDir);
  const overlay = new Map();
  for (const [source, target] of Object.entries(pathMap)) {
    expectRequest(typeof target === "string", `path_map value for ${source} must be a string`);
    const sourcePath = resolve(root, source);
    const targetPath = realPath(resolve(runDir, target));
    expectRequest(isInside(root, sourcePath), `path_map key ${source} is outside root`);
    expectRequest(isInside(realRunDir, targetPath), `path_map value ${target} is outside run_dir`);
    overlay.set(realPath(sourcePath), { source, target: targetPath });
  }
  return overlay;
};

const readSourceMap = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null);

/**
 * Serves the instrumented copy of each mapped file. The `config` hook also
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
    transform(_code, id) {
      if (id.startsWith("\0")) return null;
      const entry = overlay.get(realPath(id.split("?", 1)[0]));
      if (entry === undefined) return null;
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

/** True for a test that ran to a result. A skipped or cancelled test did not run. */
const hasRun = (status) => status === "pass" || status === "fail";

const isConcurrent = (testCase) =>
  testCase.options.concurrent === true || testCase.project.config.sequence?.concurrent === true;

const errorMessage = (error) => String(error?.message ?? error);

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

/** Names each test of a module. A repeated full name gets a ` (n)` suffix. */
const uniqueTestNames = (testCases) => {
  const seen = new Map();
  return testCases.map((testCase) => {
    const count = (seen.get(testCase.fullName) ?? 0) + 1;
    seen.set(testCase.fullName, count);
    return count === 1 ? testCase.fullName : `${testCase.fullName} (${count})`;
  });
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

/** Rounds each value of a map up to a whole millisecond, with the keys sorted. */
const roundedByKey = (values) =>
  Object.fromEntries(
    Object.keys(values)
      .toSorted(byCodeUnit)
      .map((key) => [key, Math.ceil(values[key])]),
  );

/** Reads each test of one dry-run module with its hits and its report entry. */
const readDryRunTests = (testModule, file, prefix) => {
  const testCases = [...testModule.children.allTests()];
  const names = uniqueTestNames(testCases);
  return testCases.map((testCase, index) => {
    const meta = testCase.meta();
    return {
      taskId: testCase.id,
      concurrent: isConcurrent(testCase),
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

/**
 * Gives the hits of each test that ran, as `[test_id, hits]` pairs.
 * Concurrent tests overlap, so a hit cannot name its test. Each test that ran
 * in such a file gets the union of the file hits. The scheduler then runs a
 * mutant against every test of the file that can kill it.
 */
const attributeHits = (entries) => {
  const ran = entries.filter((entry) => hasRun(entry.report.status));
  const shared = entries.some((entry) => entry.concurrent)
    ? [...new Set(ran.flatMap((entry) => entry.hits))].toSorted()
    : null;
  return ran
    .map((entry) => [entry.report.test_id, shared ?? [...entry.hits].toSorted()])
    .filter(([, hits]) => hits.length > 0);
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

const failureStatus = (failure) => (failure.failure_kind === "timeout" ? "timeout" : "killed");

/**
 * Gives the status of a run. An error outside a test fails the Vitest run, so
 * the mutant is detected. Zero executed tests, or fewer than requested with
 * no failure, is a fault of the run and never evidence that the mutant
 * survived.
 */
const runStatus = (tests, errors, requestedCount) => {
  const firstFailure = tests.find((test) => test.status === "fail");
  if (firstFailure !== undefined) return failureStatus(firstFailure);
  if (errors.length > 0) return "killed";
  return tests.length === requestedCount ? "survived" : "runtime_error";
};

/**
 * Creates the Vitest adapter for an `init` request. It starts one Vitest
 * instance with no file parallelism and one worker, and keeps the project
 * `isolate` setting.
 *
 * The companion reads this export from the `import()` namespace that
 * `loadAdapter` returns. Fallow 3.32 does not follow that return, so the
 * next line keeps the export out of the unused-export findings.
 */
// fallow-ignore-next-line unused-export
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
    return {
      duration_ms: Math.round(performance.now() - started),
      tests: summary.tests,
      hits: summary.hits,
      static_hits: [...summary.staticHits].toSorted(),
      file_setup_ms: roundedByKey(summary.setup),
      unhandled_errors: runErrors(result, root),
      unloaded_paths: Object.keys(init.path_map)
        .filter((path) => !loaded.has(path))
        .toSorted(),
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
    const tests = collectRunTests(result.testModules, testIdByTask);
    const errors = runErrors(result, root);
    const status = runStatus(tests, errors, requested.length);
    return {
      mutant_id: request.mutant_id,
      status,
      // With no failing test, a kill comes from an error outside a test, so
      // no test is the killer.
      killed_by:
        status === "killed"
          ? (tests.find((test) => test.status === "fail")?.test_id ?? null)
          : null,
      duration_ms: Math.round(performance.now() - started),
      tests_executed: tests.length,
      tests,
      unhandled_errors: errors,
    };
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
