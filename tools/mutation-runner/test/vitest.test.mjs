import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FILE = "test/math.test.ts";
const FAULTS = "test/faults.test.ts";
const CONCURRENT = "test/concurrent.test.ts";
const UNHANDLED = "test/unhandled.test.ts";
const id = (name, file = FILE) => `${file}::${name}`;
const withoutMessage = ({ message: _message, ...entry }) => entry;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version}`, () => {
    let fixture;
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
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: { isolation: "process", test_files: [FILE], env: {} },
      });
      dryRun = await request({ type: "dry_run", test_files: [FILE] });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("ready reports the project runner and the measured capabilities", () => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.equal(ready.runner_version, version);
      assert.equal(ready.runtime_version, process.version);
      assert.deepEqual(ready.test_files, [FILE]);
      assert.deepEqual(ready.capabilities, {
        per_test_attribution: true,
        test_filter: true,
        hot_switch: true,
        bail: true,
        fresh_modules_per_run: true,
        batch_mutants: false,
      });
      assert.deepEqual(ready.notes, []);
    });

    test("the dry run reports every test with its status and line", () => {
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
            line: 5,
            status: "pass",
          },
          {
            test_id: id("add > adds again"),
            file: FILE,
            name: "add > adds again",
            line: 9,
            status: "pass",
          },
          { test_id: id("limit"), file: FILE, name: "limit", line: 14, status: "pass" },
          {
            test_id: id("positive without an assertion"),
            file: FILE,
            name: "positive without an assertion",
            line: 18,
            status: "pass",
          },
          { test_id: id("noise"), file: FILE, name: "noise", line: 23, status: "pass" },
          { test_id: id("skipped"), file: FILE, name: "skipped", line: 29, status: "skip" },
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
    });

    test("a mutant that a test detects is killed, and bail stops after the first failure", async () => {
      const result = await run("m1", [id("add > adds numbers"), id("add > adds again")]);
      assert.equal(result.type, "run_result", JSON.stringify(result));
      assert.equal(result.mutant_id, "m1");
      assert.equal(result.status, "killed");
      assert.equal(result.killed_by, id("add > adds numbers"));
      assert.equal(result.tests_executed, 1);
      assert.deepEqual(result.tests.map(withoutMessage), [
        { test_id: id("add > adds numbers"), status: "fail", failure_kind: "assertion" },
      ]);
    });

    test("a failed test carries its assertion and its stack, relative to the project", async () => {
      const result = await run("m1", [id("add > adds numbers")]);
      const { message } = result.tests[0];
      assert.match(message, /^AssertionError: expected -1 to be 5/u, message);
      assert.match(message, /\n {4}at test\/math\.test\.ts:\d+:\d+/u, message);
      assert.ok(!message.includes(fixture.root), message);
      assert.ok(!message.includes(fixture.runDir), message);
      assert.ok(!message.includes("node_modules"), message);
    });

    test("a mutant that no test detects survives", async () => {
      const result = await run("m2", [id("positive without an assertion")]);
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.killed_by, null);
      assert.equal(result.tests_executed, 1);
      assert.deepEqual(result.tests, [
        { test_id: id("positive without an assertion"), status: "pass" },
      ]);
    });

    test("the original code runs again after a killed mutant", async () => {
      const result = await run("m9", [id("add > adds numbers")]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("a static mutant is active when the module loads", async () => {
      const result = await run("m3", [id("limit")]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.killed_by, id("limit"));
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

    test("reload with module isolation needs no action", async () => {
      const result = await request({ type: "reload" });
      assert.deepEqual(
        { type: result.type, method: result.method },
        { type: "reloaded", method: "noop" },
      );
    });

    test("runner output never reaches the protocol channel", () => {
      for (const line of companion.lines) {
        assert.doesNotThrow(() => JSON.parse(line), line);
      }
      assert.match(companion.stderr(), /raw stdout noise/u);
    });

    test("a mutant that throws a runtime error is killed with the error failure kind", async () => {
      const faults = await request({ type: "dry_run", test_files: [FAULTS] });
      assert.equal(faults.type, "dry_run_result", JSON.stringify(faults));
      const result = await run("m6", [id("shouts", FAULTS)]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.deepEqual(result.tests.map(withoutMessage), [
        { test_id: id("shouts", FAULTS), status: "fail", failure_kind: "error" },
      ]);
      assert.match(result.tests[0].message, /^TypeError: /u);
    });

    test("a slow mutant times out at timeout_ms, not at the project test timeout", async () => {
      const started = performance.now();
      const result = await run("m5", [id("pauses", FAULTS)], 500);
      assert.equal(result.status, "timeout", JSON.stringify(result));
      assert.equal(result.killed_by, null);
      assert.deepEqual(result.tests.map(withoutMessage), [
        { test_id: id("pauses", FAULTS), status: "fail", failure_kind: "timeout" },
      ]);
      assert.ok(performance.now() - started < 10_000, "the run waited for the project timeout");
    });

    test("a run after a short timeout_ms uses the next timeout_ms", async () => {
      const result = await run("m9", [id("pauses", FAULTS)]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("an unhandled rejection from a mutant kills it although each test passes", async () => {
      const result = await run("m4", [id("settles", FAULTS)]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.tests_executed, 1);
      assert.deepEqual(result.tests, [{ test_id: id("settles", FAULTS), status: "pass" }]);
      assert.equal(result.killed_by, null);
      assert.deepEqual(
        result.unhandled_errors?.map(({ message, file }) => ({ message, file })),
        [{ message: "late unhandled", file: FAULTS }],
      );
    });

    test("a mutant that breaks the module load is killed, not a runtime error", async () => {
      const result = await run("m7", [id("settles", FAULTS)]);
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.tests_executed, 0);
      assert.deepEqual(
        result.unhandled_errors.map(({ message, file }) => ({ message, file })),
        [{ message: "load failure", file: FAULTS }],
      );
    });

    test("a control run with a null mutant_id runs the original code through the run path", async () => {
      const result = await run(null, [id("settles", FAULTS)]);
      assert.equal(result.type, "run_result", JSON.stringify(result));
      assert.equal(result.mutant_id, null);
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.tests_executed, 1);
    });

    test("the dry run reports an unhandled error of the original code", async () => {
      const result = await request({ type: "dry_run", test_files: [UNHANDLED] });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      assert.deepEqual(result.unhandled_errors, [
        { message: "baseline unhandled", file: UNHANDLED },
      ]);
      assert.deepEqual(dryRun.unhandled_errors, []);
    });

    test("concurrent tests in a file share the hits of the file, so a covering test kills the mutant", async () => {
      const result = await request({ type: "dry_run", test_files: [CONCURRENT] });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      assert.deepEqual(result.hits, {
        [id("adds later", CONCURRENT)]: ["m1", "m2"],
        [id("checks the sign of zero", CONCURRENT)]: ["m1", "m2"],
      });
      const covering = Object.entries(result.hits)
        .filter(([, hits]) => hits.includes("m2"))
        .map(([testId]) => testId);
      const killed = await run("m2", covering);
      assert.equal(killed.status, "killed", JSON.stringify(killed));
      assert.equal(killed.killed_by, id("checks the sign of zero", CONCURRENT));
    });
  });
}
