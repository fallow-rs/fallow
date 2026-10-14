import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./companion-process.mjs";
import { createFixtureProject } from "./vitest-fixture.mjs";

const FILE = "test/math.test.ts";
const FAULTS = "test/faults.test.ts";
const CONCURRENT = "test/concurrent.test.ts";
const id = (name, file = FILE) => `${file}::${name}`;

/**
 * The protocol tests of an adapter that starts one test process for each
 * test file. `runner` is the runner name, `fixture` the fixture directory,
 * `runtime` the executable of the companion, `runnerVersion` the version
 * that `ready` must report, and `testFiles` the test files that the default
 * patterns of the runner match.
 */
export const childRunnerSuite = ({
  runner,
  fixture,
  runtime,
  runnerVersion,
  testFiles,
  skip = false,
}) => {
  describe(`${runner} adapter`, { skip }, () => {
    let project;
    let companion;
    let ready;
    let dryRun;
    let nextId = 1;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    const run = (mutantId, testIds, timeoutMs = 10_000) =>
      request({
        type: "run",
        mutant_id: mutantId,
        test_ids: testIds,
        timeout_ms: timeoutMs,
        bail: true,
      });

    before(async () => {
      project = createFixtureProject({ fixture });
      companion = startCompanion({ runtime });
      ready = await request({
        type: "init",
        root: project.root,
        runner,
        config_path: null,
        run_dir: project.runDir,
        path_map: project.pathMap,
        options: { env: {} },
      });
      dryRun = await request({ type: "dry_run", test_files: [FILE] });
    });

    after(async () => {
      await request({ type: "shutdown" }).catch(() => {});
      companion?.stop();
      project?.cleanup();
    });

    test("ready reports the runner version, the test files and the capabilities", () => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.equal(ready.runner_version, runnerVersion);
      assert.deepEqual(ready.test_files, testFiles);
      assert.equal(ready.capabilities.per_test_attribution, true);
      assert.equal(ready.capabilities.test_filter, true);
      assert.equal(ready.capabilities.hot_switch, false);
      assert.equal(ready.capabilities.fresh_modules_per_run, true);
      assert.equal(ready.capabilities.batch_mutants, false);
      assert.deepEqual(ready.notes, []);
    });

    test("the dry run reports every test with its suite path, status and line", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(
        dryRun.tests.map(({ test_id, file, name, line, status }) => ({
          test_id,
          file,
          name,
          line,
          status,
        })),
        [
          {
            test_id: id("add > adds numbers"),
            file: FILE,
            name: "add > adds numbers",
            line: 7,
            status: "pass",
          },
          {
            test_id: id("add > adds again"),
            file: FILE,
            name: "add > adds again",
            line: 11,
            status: "pass",
          },
          { test_id: id("limit"), file: FILE, name: "limit", line: 16, status: "pass" },
          {
            test_id: id("positive without an assertion"),
            file: FILE,
            name: "positive without an assertion",
            line: 20,
            status: "pass",
          },
          { test_id: id("noise"), file: FILE, name: "noise", line: 24, status: "pass" },
          { test_id: id("skipped"), file: FILE, name: "skipped", line: 29, status: "skip" },
          { test_id: id("todo later"), file: FILE, name: "todo later", line: 33, status: "todo" },
        ],
      );
    });

    test("the dry run attributes hits to each test and separates static hits", () => {
      assert.deepEqual(dryRun.hits, {
        [id("add > adds numbers")]: ["m1"],
        [id("add > adds again")]: ["m1"],
        [id("positive without an assertion")]: ["m2"],
      });
      assert.deepEqual(dryRun.static_hits, ["m3"]);
      assert.deepEqual(dryRun.unloaded_paths, ["src/faults.ts", "src/unused.ts"]);
      assert.deepEqual(dryRun.unhandled_errors, []);
      assert.ok(dryRun.file_setup_ms[FILE] >= 0, JSON.stringify(dryRun.file_setup_ms));
    });

    test("a mutant that a test detects is killed", async () => {
      const result = await run("m1", [id("add > adds numbers"), id("add > adds again")]);
      assert.equal(result.type, "run_result", JSON.stringify(result));
      assert.equal(result.mutant_id, "m1");
      assert.equal(result.status, "killed");
      assert.equal(result.killed_by, id("add > adds numbers"));
      assert.deepEqual(result.tests[0], {
        test_id: id("add > adds numbers"),
        status: "fail",
        failure_kind: "assertion",
      });
    });

    test("a mutant that no test detects survives, and the run selects only the requested test", async () => {
      const result = await run("m2", [id("positive without an assertion")]);
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.killed_by, null);
      assert.equal(result.tests_executed, 1);
      assert.deepEqual(result.tests, [
        { test_id: id("positive without an assertion"), status: "pass" },
      ]);
    });

    test("a static mutant is active when the module loads", async () => {
      const result = await run("m3", [id("limit")]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.killed_by, id("limit"));
    });

    test("an unknown mutant runs the original code", async () => {
      const result = await run("m9", [id("add > adds numbers")]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("a run where zero tests execute is a runtime error, never survived", async () => {
      const result = await run("m1", [id("skipped")]);
      assert.equal(result.status, "runtime_error", JSON.stringify(result));
      assert.equal(result.tests_executed, 0);
    });

    test("a run with a test id that the dry run did not report is a bad request", async () => {
      const result = await run("m1", [id("no such test")]);
      assert.equal(result.type, "error");
      assert.equal(result.error.code, "bad-request");
    });

    test("a dry run of a file that the runner does not list is an error", async () => {
      const result = await request({ type: "dry_run", test_files: ["src/math.ts"] });
      assert.equal(result.type, "error");
      assert.equal(result.error.code, "test-file-missing");
    });

    test("reload needs no action, because each run starts new test processes", async () => {
      const result = await request({ type: "reload" });
      assert.deepEqual(
        { type: result.type, method: result.method },
        { type: "reloaded", method: "noop" },
      );
    });

    test("test output never reaches the protocol channel", () => {
      assert.deepEqual(companion.strayLines, []);
      for (const line of companion.lines) {
        assert.doesNotThrow(() => JSON.parse(line), line);
      }
    });

    test("a mutant that throws a runtime error is killed with the error failure kind", async () => {
      const faults = await request({ type: "dry_run", test_files: [FAULTS] });
      assert.equal(faults.type, "dry_run_result", JSON.stringify(faults));
      const result = await run("m6", [id("shouts", FAULTS)]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.deepEqual(result.tests, [
        { test_id: id("shouts", FAULTS), status: "fail", failure_kind: "error" },
      ]);
    });

    test("a slow mutant times out at timeout_ms", async () => {
      const started = performance.now();
      const result = await run("m5", [id("pauses", FAULTS)], 500);
      assert.equal(result.status, "timeout", JSON.stringify(result));
      assert.equal(result.killed_by, null);
      assert.deepEqual(result.tests, [
        { test_id: id("pauses", FAULTS), status: "fail", failure_kind: "timeout" },
      ]);
      assert.ok(performance.now() - started < 10_000, "the run waited for the slow test");
    });

    test("a mutant that breaks the module load is killed, not a runtime error", async () => {
      const result = await run("m7", [id("settles", FAULTS)]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.tests_executed, 0);
      assert.equal(result.unhandled_errors.length, 1, JSON.stringify(result));
      assert.equal(result.unhandled_errors[0].file, FAULTS);
      assert.match(result.unhandled_errors[0].message, /load failure/u);
    });

    test("overlapping tests in a file share the hits of the file, so a covering test kills the mutant", async () => {
      const result = await request({ type: "dry_run", test_files: [CONCURRENT] });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      const overlap = (name) => Object.keys(result.hits).find((key) => key.endsWith(name));
      assert.deepEqual(result.hits[overlap("adds later")], ["m1", "m2"]);
      assert.deepEqual(result.hits[overlap("checks the sign of zero")], ["m1", "m2"]);
      const killed = await run("m2", [overlap("adds later"), overlap("checks the sign of zero")]);
      assert.equal(killed.status, "killed", JSON.stringify(killed));
    });
  });
};
