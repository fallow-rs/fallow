import assert from "node:assert/strict";
import * as nodeModule from "node:module";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const CHILD_TEST = "child/child.test.ts";
const TEST_ID = `${CHILD_TEST}::adds in a Node child process`;
const ONE_PRELOAD_ID = `${CHILD_TEST}::keeps one subprocess preload in NODE_OPTIONS`;

// The child process serves the instrumented file through the synchronous
// module hooks of Node. Without them, the child loads the original file.
const SKIP = typeof nodeModule.registerHooks === "function" ? false : "no registerHooks";

const CONFIGS = [
  { name: "forks pool", configPath: "vitest.child.config.mjs" },
  { name: "threads pool without isolation", configPath: "vitest.child-threads.config.mjs" },
];

for (const { packageName, version } of VITEST_MAJORS) {
  for (const { name, configPath } of CONFIGS) {
    describe(`vitest ${version} with a test that imports a source file in a Node child (${name})`, () => {
      let fixture;
      let companion;
      let nextId = 1;
      const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
      const run = (mutantId, testId = TEST_ID) =>
        request({
          type: "run",
          mutant_id: mutantId,
          test_ids: [testId],
          timeout_ms: 10_000,
          bail: true,
        });

      before(async () => {
        fixture = createFixtureProject({ packageName });
        companion = startCompanion();
        const ready = await request({
          type: "init",
          root: fixture.root,
          runner: "vitest",
          config_path: configPath,
          run_dir: fixture.runDir,
          path_map: fixture.pathMap,
        });
        assert.equal(ready.type, "ready", JSON.stringify(ready));
      });

      after(async () => {
        await request({ type: "shutdown" }).catch(() => {});
        companion?.stop();
        fixture?.cleanup();
      });

      test("the dry run passes", { skip: SKIP }, async () => {
        const dryRun = await request({ type: "dry_run" });
        assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
        assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
        assert.deepEqual(
          dryRun.tests.map(({ test_id, status }) => ({ test_id, status })),
          [
            { test_id: TEST_ID, status: "pass" },
            { test_id: ONE_PRELOAD_ID, status: "pass" },
          ],
        );
      });

      test("the assertion in the child process kills the mutant", { skip: SKIP }, async () => {
        const result = await run("m1");
        assert.equal(result.status, "killed", JSON.stringify(result));
        assert.equal(result.killed_by, TEST_ID);
      });

      test("the child process of the next run gets the next mutant", { skip: SKIP }, async () => {
        const result = await run("m3");
        assert.equal(result.status, "survived", JSON.stringify(result));
      });

      test("a later run replaces the preload of an earlier run", { skip: SKIP }, async () => {
        const result = await run("m3", ONE_PRELOAD_ID);
        assert.equal(result.status, "survived", JSON.stringify(result));
      });
    });
  }
}
