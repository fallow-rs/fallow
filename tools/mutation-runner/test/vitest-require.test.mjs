import assert from "node:assert/strict";
import * as nodeModule from "node:module";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const REQUIRE_TEST = "require/shipping.test.ts";
const REQUIRED_FILE = "require/shipping.cjs";
const FREE_AT_MINIMUM = "gives free shipping at the minimum subtotal";
const TEST_ID = `${REQUIRE_TEST}::${FREE_AT_MINIMUM}`;

// The synchronous module hooks of Node serve the instrumented file to a
// native `require`. Without them, the original file stays in place.
const HAS_SYNC_HOOKS = typeof nodeModule.registerHooks === "function";

const CONFIGS = [
  { name: "forks pool", configPath: "vitest.require.config.mjs" },
  { name: "threads pool without isolation", configPath: "vitest.require-threads.config.mjs" },
];

const startProject = async ({ packageName, configPath }) => {
  const fixture = createFixtureProject({ packageName, commonjs: true });
  const companion = startCompanion();
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const ready = await request({
    type: "init",
    root: fixture.root,
    runner: "vitest",
    config_path: configPath,
    run_dir: fixture.runDir,
    path_map: fixture.pathMap,
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

for (const { packageName, version } of VITEST_MAJORS) {
  for (const { name, configPath } of CONFIGS) {
    describe(`vitest ${version} with a file that a test loads with require (${name})`, () => {
      let project;
      let dryRun;

      before(async () => {
        project = await startProject({ packageName, configPath });
        dryRun = await project.request({ type: "dry_run" });
      });

      after(() => project?.stop());

      test(
        "the dry run records the required file as loaded and covered",
        {
          skip: HAS_SYNC_HOOKS ? false : "node:module registerHooks is not available",
        },
        () => {
          assert.equal(project.ready.type, "ready", JSON.stringify(project.ready));
          assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
          assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
          assert.ok(!dryRun.unloaded_paths.includes(REQUIRED_FILE), JSON.stringify(dryRun));
          assert.ok(dryRun.hits[TEST_ID]?.includes("m20"), JSON.stringify(dryRun));
        },
      );

      test(
        "without the hook API the required file stays unloaded",
        {
          skip: HAS_SYNC_HOOKS ? "node:module registerHooks is available" : false,
        },
        () => {
          assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
          assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
          assert.ok(dryRun.unloaded_paths.includes(REQUIRED_FILE), JSON.stringify(dryRun));
        },
      );

      test(
        "a test that requires the file kills its mutant",
        {
          skip: HAS_SYNC_HOOKS ? false : "node:module registerHooks is not available",
        },
        async () => {
          const result = await project.request({
            type: "run",
            mutant_id: "m20",
            test_ids: [TEST_ID],
            timeout_ms: 5000,
            bail: true,
          });
          assert.equal(result.status, "killed", JSON.stringify(result));
          assert.equal(result.killed_by, TEST_ID);
        },
      );
    });
  }
}
