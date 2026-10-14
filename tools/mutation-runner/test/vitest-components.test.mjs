import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FILE = "components/badge.test.ts";
const id = (name) => `${FILE}::${name}`;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with a component file`, () => {
    let fixture;
    let companion;
    let dryRun;
    let nextId = 1;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName, components: true });
      companion = startCompanion();
      const ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.component.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: { test_files: [FILE] },
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      dryRun = await request({ type: "dry_run", test_files: [FILE] });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("the framework plugin compiles the instrumented component and keeps its style request", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      const statuses = Object.fromEntries(dryRun.tests.map((entry) => [entry.name, entry.status]));
      assert.deepEqual(statuses, {
        "label of one": "pass",
        "label of two": "pass",
        "style block": "pass",
      });
      assert.deepEqual(dryRun.hits[id("label of one")], ["m10"]);
      assert.deepEqual(dryRun.hits[id("label of two")], ["m10"]);
      assert.ok(!dryRun.unloaded_paths.includes("src/Badge.vue"), JSON.stringify(dryRun));
    });

    test("a mutant in the script block is killed", async () => {
      const result = await request({
        type: "run",
        mutant_id: "m10",
        test_ids: [id("label of one")],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.killed_by, id("label of one"));
    });
  });
}
