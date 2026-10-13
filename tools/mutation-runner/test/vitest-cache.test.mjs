import assert from "node:assert/strict";
import { after, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FILE = "test/math.test.ts";
const id = (name) => `${FILE}::${name}`;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version}`, () => {
    const fixture = createFixtureProject({ packageName });
    after(() => fixture.cleanup());

    const dryRunWithNewCompanion = async () => {
      const companion = startCompanion();
      try {
        const ready = await companion.request({
          v: 1,
          id: 1,
          type: "init",
          root: fixture.root,
          runner: "vitest",
          config_path: "vitest.cache.config.mjs",
          run_dir: fixture.runDir,
          path_map: fixture.pathMap,
          options: { test_files: [FILE] },
        });
        assert.equal(ready.type, "ready", JSON.stringify(ready));
        return await companion.request({ v: 1, id: 2, type: "dry_run", test_files: [FILE] });
      } finally {
        companion.stop();
      }
    };

    test("a new companion on the same project reads the current run directory, not a cached transform", async () => {
      const first = await dryRunWithNewCompanion();
      assert.deepEqual(first.hits[id("add > adds numbers")], ["m1"], JSON.stringify(first));

      fixture.renameMutants("b");
      const second = await dryRunWithNewCompanion();
      assert.deepEqual(second.hits, {
        [id("add > adds numbers")]: ["m1b"],
        [id("add > adds again")]: ["m1b"],
        [id("positive without an assertion")]: ["m2b"],
      });
      assert.deepEqual(second.static_hits, ["m3b"]);
      assert.deepEqual(second.unloaded_paths, ["src/faults.ts", "src/unused.ts"]);
    });
  });
}
