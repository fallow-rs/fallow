import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FIRST = "shared/first.test.ts::adds";
const SECOND = "shared/second.test.ts::adds";
const LEAK_A = "leak/a.test.ts::marks the module";
const LEAK_B = "leak/b.test.ts::expects a fresh worker";

const startProject = async (packageName, configPath, env = {}) => {
  const fixture = createFixtureProject({ packageName });
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
    options: { env },
  });
  assert.equal(ready.type, "ready", JSON.stringify(ready));
  const dryRun = await request({ type: "dry_run", test_files: [] });
  assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
  const run = (mutantId, testIds) =>
    request({
      type: "run",
      mutant_id: mutantId,
      test_ids: testIds,
      timeout_ms: 10_000,
      bail: true,
    });
  return { fixture, companion, run };
};

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} shared mutant runs`, () => {
    let shared;
    let leak;
    let pidLog;

    before(async () => {
      pidLog = join(createFixtureProject({ packageName }).root, "..", `pids-${version}.log`);
      writeFileSync(pidLog, "");
      shared = await startProject(packageName, "vitest.shared.config.mjs", {
        FALLOW_PID_LOG: pidLog,
      });
      leak = await startProject(packageName, "vitest.leak.config.mjs");
    });

    after(() => {
      for (const project of [shared, leak]) {
        project?.companion.stop();
        project?.fixture.cleanup();
      }
    });

    test("the test files of one mutant run share one worker", async () => {
      writeFileSync(pidLog, "");
      const result = await shared.run("m9", [FIRST, SECOND]);
      assert.equal(result.status, "survived", JSON.stringify(result));
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 1, `one worker for both files, got ${[...pids].join(", ")}`);
    });

    test("a run of each file stays a new worker for each mutant run", async () => {
      writeFileSync(pidLog, "");
      await shared.run("m9", [FIRST, SECOND]);
      await shared.run("m9", [FIRST, SECOND]);
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 2, "each run gets a fresh worker");
    });

    test("a project whose tests differ in a shared worker keeps isolated runs", async () => {
      const result = await leak.run("m9", [LEAK_A, LEAK_B]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });
  });
}
