import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const ENV_TEST = "env/env.test.ts";
const LOADS = "loads the env module and checks add";
const CLI_VARS = "sees the variables of the Vitest CLI";
const id = (name) => `${ENV_TEST}::${name}`;
const VITEST_CLI_ENV = ["NODE_ENV", "TEST", "VITEST"];

// The companion gets none of the variables that the Vitest CLI sets, as when
// `fallow mutation` runs from a plain shell.
const startProject = async ({ packageName, env }) => {
  const fixture = createFixtureProject({ packageName });
  const companion = startCompanion({ unsetEnv: VITEST_CLI_ENV });
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const ready = await request({
    type: "init",
    root: fixture.root,
    runner: "vitest",
    config_path: "vitest.env.config.mjs",
    run_dir: fixture.runDir,
    path_map: fixture.pathMap,
    options: env === undefined ? {} : { env },
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

const statuses = (dryRun) =>
  Object.fromEntries(dryRun.tests.map((entry) => [entry.name, entry.status]));

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with no NODE_ENV in the environment`, () => {
    let project;
    let dryRun;

    before(async () => {
      project = await startProject({ packageName });
      dryRun = await project.request({ type: "dry_run" });
    });

    after(() => project?.stop());

    test("the tests run under NODE_ENV=test, TEST=true and VITEST=true", () => {
      assert.equal(project.ready.type, "ready", JSON.stringify(project.ready));
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
      assert.deepEqual(statuses(dryRun), { [LOADS]: "pass", [CLI_VARS]: "pass" });
    });

    test("a test that loads the env module kills a mutant", async () => {
      const result = await project.request({
        type: "run",
        mutant_id: "m1",
        test_ids: [id(LOADS)],
        timeout_ms: 5000,
        bail: true,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.killed_by, id(LOADS));
    });
  });

  describe(`vitest ${version} with NODE_ENV in options.env`, () => {
    let project;

    before(async () => {
      project = await startProject({
        packageName,
        env: { NODE_ENV: "production", EXPECTED_NODE_ENV: "production" },
      });
    });

    after(() => project?.stop());

    test("the NODE_ENV value of options.env wins", async () => {
      const dryRun = await project.request({ type: "dry_run" });
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(statuses(dryRun), { [LOADS]: "pass", [CLI_VARS]: "pass" });
    });
  });
}
