import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const ADDS = "test/math.test.ts::add > adds numbers";

const startProject = async (packageName) => {
  const fixture = createFixtureProject({ packageName });
  const companion = startCompanion();
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const ready = await request({
    type: "init",
    root: fixture.root,
    runner: "vitest",
    config_path: "vitest.pre-load.config.mjs",
    run_dir: fixture.runDir,
    path_map: fixture.pathMap,
    options: {},
  });
  return {
    ready,
    request,
    stop: () => {
      companion.stop();
      fixture.cleanup();
    },
  };
};

// A project plugin with `enforce: "pre"` can load a mapped file before the
// overlay. The overlay then replaces the code in its `transform` hook, so the
// tests still run the instrumented copy.
for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with a project plugin that loads a mapped file`, () => {
    let project;
    let dryRun;

    before(async () => {
      project = await startProject(packageName);
      dryRun = await project.request({ type: "dry_run" });
    });

    after(() => project?.stop());

    test("the dry run records hits in the mapped file", () => {
      assert.equal(project.ready.type, "ready", JSON.stringify(project.ready));
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.hits[ADDS], ["m1"], JSON.stringify(dryRun.hits));
      assert.ok(!dryRun.unloaded_paths.includes("src/math.ts"), JSON.stringify(dryRun));
    });

    test("a mutant in the mapped file is killed", async () => {
      const result = await project.request({
        type: "run",
        mutant_id: "m1",
        test_ids: [ADDS],
        timeout_ms: 5000,
        bail: false,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
    });
  });
}
