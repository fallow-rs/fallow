import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FIRST = "shared/first.test.ts::adds";
const SECOND = "shared/second.test.ts::adds";
const LEAK_A = "leak/a.test.ts::marks the module";
const LEAK_B = "leak/b.test.ts::expects a fresh worker";
const STORE_A = "storeleak/a.test.ts::fills the store";
const STORE_B = "storeleak/b.test.ts::starts empty";
const MEMO = "reuse/memo.test.ts::doubles";
const EXIT = "reuse/exit.test.ts::stays";

const startProject = async (packageName, configPath, env = {}, options = {}) => {
  const fixture = createFixtureProject({ packageName, ...options });
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
  const run = (mutantId, testIds, extra = {}) =>
    request({
      type: "run",
      mutant_id: mutantId,
      test_ids: testIds,
      timeout_ms: 10_000,
      bail: true,
      ...extra,
    });
  return { fixture, companion, run };
};

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} shared mutant runs`, () => {
    let shared;
    let leak;
    let storeLeak;
    let reuse;
    let pidDir;
    let pidLog;

    before(async () => {
      pidDir = mkdtempSync(join(tmpdir(), "fallow-mutation-pids-"));
      pidLog = join(pidDir, "pids.log");
      writeFileSync(pidLog, "");
      shared = await startProject(packageName, "vitest.shared.config.mjs", {
        FALLOW_PID_LOG: pidLog,
      });
      leak = await startProject(packageName, "vitest.leak.config.mjs");
      storeLeak = await startProject(
        packageName,
        "vitest.storeleak.config.mjs",
        {},
        { store: true },
      );
      reuse = await startProject(packageName, "vitest.reuse.config.mjs", {}, { reuse: true });
    });

    after(() => {
      for (const project of [shared, leak, storeLeak, reuse]) {
        project?.companion.stop();
        project?.fixture.cleanup();
      }
      if (pidDir) rmSync(pidDir, { recursive: true, force: true });
    });

    test("the test files of one mutant run share one worker", async () => {
      writeFileSync(pidLog, "");
      const result = await shared.run("m9", [FIRST, SECOND]);
      assert.equal(result.status, "survived", JSON.stringify(result));
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 1, `one worker for both files, got ${[...pids].join(", ")}`);
    });

    test("consecutive mutant runs reuse one worker process", async () => {
      writeFileSync(pidLog, "");
      await shared.run("m9", [FIRST, SECOND]);
      await shared.run("m9", [FIRST, SECOND]);
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 1, `one process for both runs, got ${[...pids].join(", ")}`);
    });

    test("a control run gets a new worker process", async () => {
      await shared.run("m9", [FIRST, SECOND]);
      writeFileSync(pidLog, "");
      await shared.run(null, [FIRST, SECOND]);
      await shared.run("m9", [FIRST, SECOND]);
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 2, "the control run does not use the reused process");
    });

    test("a project whose tests differ in a shared worker keeps isolated runs", async () => {
      const result = await leak.run("m9", [LEAK_A, LEAK_B]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("a kill that only the state of an earlier file of the worker causes is not a kill", async () => {
      const control = await storeLeak.run(null, [STORE_A, STORE_B]);
      assert.equal(control.status, "survived", JSON.stringify(control));
      const result = await storeLeak.run("m30", [STORE_A, STORE_B]);
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("a module cache of an earlier mutant run does not hide a later mutant", async () => {
      const first = await reuse.run("m41", [MEMO, EXIT].slice(0, 1));
      assert.equal(first.status, "survived", JSON.stringify(first));
      const later = await reuse.run("m40", [MEMO]);
      assert.equal(later.status, "killed", JSON.stringify(later));
    });

    test("a mutant that ends the worker process does not break the next run", async () => {
      const crashed = await reuse.run("m41", [MEMO, EXIT]);
      assert.equal(crashed.status, "killed", JSON.stringify(crashed));
      const next = await reuse.run("m9", [MEMO, EXIT]);
      assert.equal(next.status, "survived", JSON.stringify(next));
    });
  });
}
