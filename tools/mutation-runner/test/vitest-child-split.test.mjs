import assert from "node:assert/strict";
import * as nodeModule from "node:module";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const SPLIT_TEST = "child-split/split.test.ts";
const PARENT_ID = `${SPLIT_TEST}::calls add in the test process`;
const CHILD_ID = `${SPLIT_TEST}::asserts only in a Node child process`;
const NO_CHILD_ID = `${SPLIT_TEST}::starts no child process`;

// The child process serves the instrumented file through the synchronous
// module hooks of Node. Without them, the child loads the original file.
const SKIP = typeof nodeModule.registerHooks === "function" ? false : "no registerHooks";

const CONFIGS = [
  { name: "forks pool", configPath: "vitest.child-split.config.mjs" },
  { name: "threads pool without isolation", configPath: "vitest.child-split-threads.config.mjs" },
];

for (const { packageName, version } of VITEST_MAJORS) {
  for (const { name, configPath } of CONFIGS) {
    describe(`vitest ${version} with a test that asserts only in a Node child (${name})`, () => {
      let fixture;
      let companion;
      let nextId = 1;
      const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

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

      test(
        "the hits of the child process select the test that started it",
        { skip: SKIP },
        async () => {
          const dryRun = await request({ type: "dry_run" });
          assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
          assert.deepEqual(dryRun.unhandled_errors, [], JSON.stringify(dryRun));
          assert.deepEqual(dryRun.hits[PARENT_ID], ["m1"]);
          assert.deepEqual(dryRun.hits[CHILD_ID], ["m1", "m3"]);
          assert.equal(dryRun.hits[NO_CHILD_ID], undefined);
        },
      );

      test("the child-only assertion kills the mutant", { skip: SKIP }, async () => {
        const result = await request({
          type: "run",
          mutant_id: "m1",
          test_ids: [CHILD_ID],
          timeout_ms: 10_000,
          bail: true,
        });
        assert.equal(result.status, "killed", JSON.stringify(result));
        assert.equal(result.killed_by, CHILD_ID);
      });

      test("a second dry run gives the same child hits", { skip: SKIP }, async () => {
        const dryRun = await request({ type: "dry_run" });
        assert.deepEqual(dryRun.hits[CHILD_ID], ["m1", "m3"]);
        assert.equal(dryRun.hits[NO_CHILD_ID], undefined);
      });
    });
  }
}
