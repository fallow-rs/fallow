import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const CLIENT_TEST = "client/client.test.ts";
const TEST_NAME = "adds in each environment";
const id = (project) => `${project}::${CLIENT_TEST}::${TEST_NAME}`;

// The companion files are outside the temp project, as with a
// `FALLOW_CACHE_DIR` outside the project root. A client environment, such
// as `happy-dom`, loads the setup file of the runtime through Vite, and Vite
// serves it only from a directory in `server.fs.allow`.
for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with an inline project in a client environment`, () => {
    let fixture;
    let companion;
    let ready;
    let dryRun;
    let nextId = 1;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.client-env.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      dryRun = await request({ type: "dry_run" });
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("the client project loads the setup file and passes its baseline", () => {
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
      assert.deepEqual(dryRun.unhandled_errors ?? [], [], JSON.stringify(dryRun));
      assert.deepEqual(
        dryRun.tests.map(({ test_id, status }) => ({ test_id, status })),
        [
          { test_id: id("client"), status: "pass" },
          { test_id: id("node"), status: "pass" },
        ],
        JSON.stringify(dryRun),
      );
      assert.deepEqual(dryRun.hits[id("client")], ["m1"], JSON.stringify(dryRun.hits));
    });

    test("a test of the client project kills a mutant", async () => {
      const result = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [id("client")],
        timeout_ms: 5000,
        bail: true,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.equal(result.killed_by, id("client"));
    });
  });
}
