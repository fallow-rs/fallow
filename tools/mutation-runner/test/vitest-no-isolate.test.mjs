import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const MATH = "test/math.test.ts";
const DUPLICATES = "test/Duplicates.test.ts";
const id = (name, file = MATH) => `${file}::${name}`;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} without module isolation`, () => {
    let fixture;
    let companion;
    let ready;
    let nextId = 1;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.no-isolate.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("modules are not fresh per run and reload asks for a restart", async () => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.equal(ready.capabilities.fresh_modules_per_run, false);
      const reloaded = await request({ type: "reload" });
      assert.equal(reloaded.method, "restart_required");
    });

    test("ready lists the test files in code-unit order", () => {
      assert.deepEqual(ready.test_files, [
        DUPLICATES,
        "test/concurrent.test.ts",
        "test/faults.test.ts",
        MATH,
        "test/unhandled.test.ts",
      ]);
    });

    test("two tests with the same name get two distinct ids", async () => {
      const result = await request({ type: "dry_run", test_files: [DUPLICATES, MATH] });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      assert.deepEqual(
        result.tests.filter((entry) => entry.file === DUPLICATES).map((entry) => entry.test_id),
        [id("repeats", DUPLICATES), id("repeats (2)", DUPLICATES)],
      );
      assert.deepEqual(result.hits[id("repeats (2)", DUPLICATES)], ["m1"]);
    });

    test("the active mutant switches between runs in the cached module", async () => {
      const killed = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [id("repeats (2)", DUPLICATES)],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(killed.status, "killed", JSON.stringify(killed));
      assert.equal(killed.killed_by, id("repeats (2)", DUPLICATES));
      const survived = await request({
        type: "run",
        mutant_id: "m9",
        test_ids: [id("repeats (2)", DUPLICATES), id("add > adds numbers")],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(survived.status, "survived", JSON.stringify(survived));
      assert.equal(survived.tests_executed, 2);
    });
  });
}

let fixture;

before(() => {
  fixture = createFixtureProject();
});

after(() => {
  fixture?.cleanup();
});

test("a config path that does not exist gives config-load-failed", async () => {
  const other = startCompanion();
  try {
    const response = await other.request({
      v: 1,
      id: 1,
      type: "init",
      root: fixture.root,
      runner: "vitest",
      config_path: "missing.config.mjs",
      run_dir: fixture.runDir,
      path_map: {},
      options: {},
    });
    assert.deepEqual([response.type, response.error?.code], ["error", "config-load-failed"]);
  } finally {
    other.stop();
  }
});

test("a test file that is not a Vitest test file gives test-file-missing", async () => {
  const other = startCompanion();
  try {
    const response = await other.request({
      v: 1,
      id: 1,
      type: "init",
      root: fixture.root,
      runner: "vitest",
      config_path: "vitest.config.mjs",
      run_dir: fixture.runDir,
      path_map: {},
      options: { test_files: ["test/missing.test.ts"] },
    });
    assert.deepEqual([response.type, response.error?.code], ["error", "test-file-missing"]);
  } finally {
    other.stop();
  }
});

test("a path map value outside the run directory is a bad request", async () => {
  const other = startCompanion();
  try {
    const response = await other.request({
      v: 1,
      id: 1,
      type: "init",
      root: fixture.root,
      runner: "vitest",
      config_path: "vitest.config.mjs",
      run_dir: fixture.runDir,
      path_map: { "src/math.ts": "../project/src/math.ts" },
      options: {},
    });
    assert.deepEqual([response.type, response.error?.code], ["error", "bad-request"]);
  } finally {
    other.stop();
  }
});
