import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FILE = "names/each.test.ts";
const id = (name) => `${FILE}::${name}`;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with tests that share a title`, () => {
    let fixture;
    let companion;
    let nextId = 1;
    let dryRun;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      const ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.names.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      dryRun = await request({ type: "dry_run", test_files: [FILE] });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("the first id keeps its name and the second gets a suffix", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(
        dryRun.tests.map((entry) => entry.test_id),
        [id("adds"), id("adds (2)"), id("alone")],
      );
    });

    test("the dry run names the ids of the tests with a repeated title", () => {
      assert.deepEqual(dryRun.duplicate_test_ids, [id("adds"), id("adds (2)")]);
    });
  });
}
