import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const SLOW_HOOK = "guards/slow-hook.test.ts";
const RETRY = "guards/retry.test.ts";
const FD1 = "guards/fd1.test.ts";
const id = (file, name) => `${file}::${name}`;

const startProject = async ({ packageName, configPath }) => {
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
    options: {},
  });
  return {
    companion,
    fixture,
    ready,
    request,
    stop: () => {
      companion.stop();
      fixture.cleanup();
    },
  };
};

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with a project retry and a slow suite hook`, () => {
    let project;
    let dryRun;

    before(async () => {
      project = await startProject({ packageName, configPath: "vitest.guards.config.mjs" });
      dryRun = await project.request({ type: "dry_run" });
    });

    after(() => project?.stop());

    test("the dry run reports a test that fails on the first attempt as a failure", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      const statuses = Object.fromEntries(dryRun.tests.map((entry) => [entry.name, entry.status]));
      assert.deepEqual(statuses, {
        "checks add on the first attempt only": "pass",
        "fails on the first attempt only": "fail",
        "retries by test option": "pass",
        "retries by suite option > checks add on the first attempt only": "pass",
        "fast after a slow suite hook": "pass",
      });
    });

    test("the dry run reports the setup time of each test file outside its tests", () => {
      assert.deepEqual(Object.keys(dryRun.file_setup_ms), [RETRY, SLOW_HOOK]);
      assert.ok(dryRun.file_setup_ms[SLOW_HOOK] >= 1400, JSON.stringify(dryRun.file_setup_ms));
      assert.ok(dryRun.file_setup_ms[RETRY] < 1000, JSON.stringify(dryRun.file_setup_ms));
    });

    for (const name of [
      "checks add on the first attempt only",
      "retries by test option",
      "retries by suite option > checks add on the first attempt only",
    ]) {
      test(`a mutant that only the first attempt detects is killed (${name})`, async () => {
        const result = await project.request({
          type: "run",
          mutant_id: "m1",
          test_ids: [id(RETRY, name)],
          timeout_ms: 10_000,
          bail: true,
        });
        assert.equal(result.status, "killed", JSON.stringify(result));
        assert.equal(result.killed_by, id(RETRY, name));
      });
    }

    test("a suite hook slower than timeout_ms keeps the project hook timeout", async () => {
      const result = await project.request({
        type: "run",
        mutant_id: "m9",
        test_ids: [id(SLOW_HOOK, "fast after a slow suite hook")],
        timeout_ms: 1000,
        bail: true,
      });
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.tests_executed, 1);
    });
  });

  describe(`vitest ${version} with no project hook timeout`, () => {
    let project;

    before(async () => {
      project = await startProject({
        packageName,
        configPath: "vitest.no-hook-timeout.config.mjs",
      });
    });

    after(() => project?.stop());

    test("a run keeps a hook timeout of 0 as no limit", async () => {
      const dryRun = await project.request({ type: "dry_run" });
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      const result = await project.request({
        type: "run",
        mutant_id: "m9",
        test_ids: [id(SLOW_HOOK, "fast after a slow suite hook")],
        timeout_ms: 1000,
        bail: true,
      });
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.deepEqual(result.unhandled_errors, []);
    });
  });

  describe(`vitest ${version} with a test that writes to file descriptor 1`, () => {
    let project;

    before(async () => {
      project = await startProject({ packageName, configPath: "vitest.threads.config.mjs" });
    });

    after(() => project?.stop());

    test("a raw write to file descriptor 1 never becomes a protocol line", async () => {
      const result = await project.request({ type: "dry_run" });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      assert.deepEqual(
        result.tests.map((entry) => [entry.test_id, entry.status]),
        [[id(FD1, "writes to file descriptor 1"), "pass"]],
      );
      assert.ok(
        project.companion.strayLines.includes("FD1 NOISE"),
        `the write reached stdout: ${JSON.stringify(project.companion.strayLines)}`,
      );
    });
  });
}
