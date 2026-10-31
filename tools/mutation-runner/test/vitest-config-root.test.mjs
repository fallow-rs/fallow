import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

// The config sets the top-level root to `dashboard/`. The paths of the
// protocol stay relative to the project root.
const FILE = "dashboard/src/math.test.ts";
const TEST_ID = `${FILE}::adds values`;
const CONFIGS = ["vitest.subproject.config.mjs", "vitest.subproject-url.config.mjs"];

for (const { packageName, version } of VITEST_MAJORS) {
  for (const configPath of CONFIGS) {
    describe(`vitest ${version} keeps the top-level root of ${configPath}`, () => {
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
          config_path: configPath,
          run_dir: fixture.runDir,
          path_map: fixture.pathMap,
          options: { test_files: [FILE] },
        });
      });

      after(() => {
        companion?.stop();
        fixture?.cleanup();
      });

      test("ready lists the test file relative to the project root", () => {
        assert.equal(ready.type, "ready", JSON.stringify(ready));
        assert.deepEqual(ready.test_files, [FILE]);
      });

      test("the dry run reports the test and its hits with project-relative paths", async () => {
        const dryRun = await request({ type: "dry_run", test_files: [FILE] });
        assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
        assert.deepEqual(
          dryRun.tests.map(({ test_id, file, status }) => ({ test_id, file, status })),
          [{ test_id: TEST_ID, file: FILE, status: "pass" }],
        );
        assert.deepEqual(dryRun.hits, { [TEST_ID]: ["m1"] });
      });

      test("a run of the mapped mutant kills it", async () => {
        const result = await request({
          type: "run",
          mutant_id: "m1",
          test_ids: [TEST_ID],
          timeout_ms: 10_000,
          bail: true,
        });
        assert.equal(result.status, "killed", JSON.stringify(result));
        assert.equal(result.killed_by, TEST_ID);
      });
    });
  }
}
