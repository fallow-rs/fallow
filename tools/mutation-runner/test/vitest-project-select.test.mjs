import assert from "node:assert/strict";
import { test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const CONFIG = "vitest.mixed-projects.config.mjs";
const FILE = "test/math.test.ts";

/** Sends `init` for the mixed-projects config with `projects` as the selection. */
const initWith = async (packageName, projects, task) => {
  const fixture = createFixtureProject({ packageName });
  const companion = startCompanion();
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  try {
    const ready = await request({
      type: "init",
      root: fixture.root,
      runner: "vitest",
      config_path: CONFIG,
      run_dir: fixture.runDir,
      path_map: fixture.pathMap,
      options: projects === undefined ? {} : { projects },
    });
    await task(ready, request);
  } finally {
    companion.stop();
    fixture.cleanup();
  }
};

const assertConfigError = (response, pattern) => {
  assert.deepEqual(
    [response.type, response.error?.code],
    ["error", "config-load-failed"],
    JSON.stringify(response),
  );
  assert.match(response.error.message, pattern);
};

for (const { packageName, version } of VITEST_MAJORS) {
  test(`vitest ${version}: a selected inline project runs next to an unselected file-based project`, async () => {
    await initWith(packageName, ["unit"], async (ready, request) => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.test_files, [FILE]);

      const testId = `${FILE}::add > adds numbers`;
      const dryRun = await request({ type: "dry_run", test_files: [FILE] });
      assert.deepEqual(dryRun.hits[testId], ["m1"], JSON.stringify(dryRun));

      const killed = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [testId],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(killed.status, "killed", JSON.stringify(killed));
    });
  });

  test(`vitest ${version}: without a selection the file-based project still fails closed`, async () => {
    await initWith(packageName, undefined, async (ready) => {
      assertConfigError(ready, /"child" comes from a separate config file/u);
    });
  });

  test(`vitest ${version}: a selected file-based project fails closed`, async () => {
    await initWith(packageName, ["child"], async (ready) => {
      assertConfigError(ready, /"child" comes from a separate config file/u);
    });
  });

  test(`vitest ${version}: a selection that matches no project fails with the name`, async () => {
    await initWith(packageName, ["missing"], async (ready) => {
      assertConfigError(ready, /matched no projects: missing|matched the filter "missing"/u);
    });
  });
}

test("init rejects a project selection that is not a list of names", async () => {
  await initWith("vitest", "unit", async (ready) => {
    assert.deepEqual(
      [ready.type, ready.error?.code],
      ["error", "bad-request"],
      JSON.stringify(ready),
    );
  });
});
