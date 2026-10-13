import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const SHARED = "projects/shared.test.ts";
const TEST_NAME = "checks per project";

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with two projects that include one file`, () => {
    let fixture;
    let companion;
    let ready;
    let dryRun;
    let nextId = 1;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.projects.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      dryRun = await request({ type: "dry_run", test_files: [SHARED] });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("ready lists the shared file once", () => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.test_files, [SHARED]);
    });

    test("the dry run reports the test of each project with a project-qualified id", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(
        dryRun.tests.map(({ test_id, file, name, status }) => ({ test_id, file, name, status })),
        [
          { test_id: `a::${SHARED}::${TEST_NAME}`, file: SHARED, name: TEST_NAME, status: "pass" },
          { test_id: `b::${SHARED}::${TEST_NAME}`, file: SHARED, name: TEST_NAME, status: "pass" },
        ],
      );
      assert.deepEqual(dryRun.hits, { [`a::${SHARED}::${TEST_NAME}`]: ["m1"] });
    });

    test("a run selects the test of one project only", async () => {
      const killed = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [`a::${SHARED}::${TEST_NAME}`],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(killed.status, "killed", JSON.stringify(killed));
      assert.equal(killed.killed_by, `a::${SHARED}::${TEST_NAME}`);
      assert.equal(killed.tests_executed, 1);

      const survived = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [`b::${SHARED}::${TEST_NAME}`],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(survived.status, "survived", JSON.stringify(survived));
      assert.deepEqual(survived.tests, [{ test_id: `b::${SHARED}::${TEST_NAME}`, status: "pass" }]);
    });
  });

  test(`vitest ${version}: a project from a separate config file fails closed`, async () => {
    const fixture = createFixtureProject({ packageName });
    const companion = startCompanion();
    try {
      const response = await companion.request({
        v: 1,
        id: 1,
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.file-projects.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      // Vitest gives the plugins of the companion to inline projects only. A
      // project without the overlay never runs a mutant, so init must fail.
      assert.deepEqual(
        [response.type, response.error?.code],
        ["error", "config-load-failed"],
        JSON.stringify(response),
      );
      assert.match(response.error.message, /\bchild\b/u);
      assert.match(response.error.message, /separate config file/u);
    } finally {
      companion.stop();
      fixture.cleanup();
    }
  });

  test(`vitest ${version}: an inline project with a disabled browser block runs the mutants`, async () => {
    const fixture = createFixtureProject({ packageName });
    const companion = startCompanion();
    const file = "test/math.test.ts";
    const testId = `${file}::add > adds numbers`;
    try {
      const ready = await companion.request({
        v: 1,
        id: 1,
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.browser-projects.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.test_files, [file]);

      const dryRun = await companion.request({ v: 1, id: 2, type: "dry_run", test_files: [file] });
      assert.deepEqual(dryRun.hits[testId], ["m1"], JSON.stringify(dryRun));

      const killed = await companion.request({
        v: 1,
        id: 3,
        type: "run",
        mutant_id: "m1",
        test_ids: [testId],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(killed.status, "killed", JSON.stringify(killed));
    } finally {
      companion.stop();
      fixture.cleanup();
    }
  });

  test(`vitest ${version}: ready notes that the type-check pass does not run`, async () => {
    const fixture = createFixtureProject({ packageName });
    const companion = startCompanion();
    try {
      const ready = await companion.request({
        v: 1,
        id: 1,
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.typecheck.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.notes, ["typecheck-not-run"]);
    } finally {
      companion.stop();
      fixture.cleanup();
    }
  });
}
