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
const REGISTRY_A = "registry/a.test.ts::registers once";
const REGISTRY_B = "registry/b.test.ts::adds";
const PAIR_A = "pair/a.test.ts::runs before b";
const PAIR_B = "pair/b.test.ts::runs before a";
const CROSS_A = "crossset/a.test.ts::counts a run";
const CROSS_B = "crossset/b.test.ts::expects no run of a before it";
const NOHIT_FIRST = "nohit/first.test.ts::adds";
const NOHIT_PLAIN = "nohit/plain.test.ts::runs no source code";

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
  assert.equal(ready.capabilities.worker_check, true, JSON.stringify(ready));
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
  const check = async (testIds) => {
    const checked = await request({ type: "check_workers", test_ids: testIds, timeout_ms: 10_000 });
    assert.equal(checked.type, "workers_checked", JSON.stringify(checked));
    return checked.worker_mode;
  };
  return { fixture, companion, run, check, dryRun };
};

const pidsOf = (pidLog) => readFileSync(pidLog, "utf8").trim().split("\n").filter(Boolean);

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} shared mutant runs`, () => {
    let shared;
    let leak;
    let storeLeak;
    let reuse;
    let registry;
    let pair;
    let crossSet;
    let noHit;
    let pidDir;
    let pidLog;
    let plainLog;

    before(async () => {
      pidDir = mkdtempSync(join(tmpdir(), "fallow-mutation-pids-"));
      pidLog = join(pidDir, "pids.log");
      writeFileSync(pidLog, "");
      plainLog = join(pidDir, "plain.log");
      writeFileSync(plainLog, "");
      noHit = await startProject(packageName, "vitest.nohit.config.mjs", {
        FALLOW_PID_LOG: pidLog,
        FALLOW_PLAIN_LOG: plainLog,
      });
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
      reuse = await startProject(
        packageName,
        "vitest.reuse.config.mjs",
        { FALLOW_PID_LOG: pidLog },
        { reuse: true },
      );
      registry = await startProject(packageName, "vitest.registry.config.mjs", {
        FALLOW_PID_LOG: pidLog,
      });
      pair = await startProject(packageName, "vitest.pair.config.mjs", {
        FALLOW_PID_LOG: pidLog,
      });
      crossSet = await startProject(packageName, "vitest.crossset.config.mjs");
    });

    after(() => {
      for (const project of [shared, leak, storeLeak, reuse, registry, pair, crossSet, noHit]) {
        project?.companion.stop();
        project?.fixture.cleanup();
      }
      if (pidDir) rmSync(pidDir, { recursive: true, force: true });
    });

    test("the dry run runs each test file one time and states no worker mode", () => {
      assert.deepEqual(pidsOf(plainLog).length, 1, "the dry run repeats no test file");
      assert.equal(noHit.dryRun.worker_mode, undefined, JSON.stringify(noHit.dryRun));
    });

    test("a run with no worker mode gives each test file its own worker", async () => {
      writeFileSync(pidLog, "");
      const result = await shared.run("m9", [FIRST, SECOND]);
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.worker_mode, "isolated", JSON.stringify(result));
      assert.equal(new Set(pidsOf(pidLog)).size, 2, "one worker for each file");
    });

    test("a check of tests that agree in one reused worker allows the reused mode", async () => {
      assert.equal(await shared.check([FIRST, SECOND]), "reused");
      writeFileSync(pidLog, "");
      const result = await shared.run("m9", [FIRST, SECOND], { worker_mode: "reused" });
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.worker_mode, "reused", JSON.stringify(result));
      await shared.run("m9", [FIRST, SECOND], { worker_mode: "reused" });
      const pids = new Set(pidsOf(pidLog));
      assert.equal(pids.size, 1, `one process for both runs, got ${[...pids].join(", ")}`);
    });

    test("a run in the shared mode shares one new worker", async () => {
      writeFileSync(pidLog, "");
      const result = await shared.run("m9", [FIRST, SECOND], { worker_mode: "shared" });
      assert.equal(result.worker_mode, "shared", JSON.stringify(result));
      await shared.run("m9", [FIRST, SECOND], { worker_mode: "shared" });
      const counts = new Map();
      for (const pid of pidsOf(pidLog)) counts.set(pid, (counts.get(pid) ?? 0) + 1);
      assert.deepEqual([...counts.values()], [2, 2], "each run had one worker for both files");
    });

    test("a control run gets a new worker process", async () => {
      await shared.run("m9", [FIRST, SECOND], { worker_mode: "reused" });
      writeFileSync(pidLog, "");
      const control = await shared.run(null, [FIRST, SECOND], { worker_mode: "reused" });
      assert.equal(
        control.worker_mode,
        "shared",
        "a control run shares the worker, not the process",
      );
      await shared.run("m9", [FIRST, SECOND], { worker_mode: "reused" });
      assert.equal(
        new Set(pidsOf(pidLog)).size,
        2,
        "the control run does not use the reused process",
      );
    });

    test("a check of tests that differ in one worker keeps isolated runs", async () => {
      assert.equal(await pair.check([PAIR_A, PAIR_B]), "isolated");
      const result = await pair.run("m9", [PAIR_A, PAIR_B], { worker_mode: "isolated" });
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("test files that differ in one worker keep their results when no run has both", async () => {
      assert.equal(await pair.check([PAIR_A]), "reused");
      assert.equal(await pair.check([PAIR_B]), "reused");
      for (const ids of [[PAIR_A], [PAIR_B], [PAIR_A], [PAIR_B]]) {
        const result = await pair.run("m9", ids, { worker_mode: "reused" });
        assert.equal(result.status, "survived", `${ids}: ${JSON.stringify(result)}`);
      }
    });

    test("a kill in the first run of a set after another set in the process is confirmed", async () => {
      assert.equal(await crossSet.check([CROSS_A]), "reused");
      assert.equal(await crossSet.check([CROSS_B]), "reused");
      const first = await crossSet.run("m9", [CROSS_A], { worker_mode: "reused" });
      assert.equal(first.status, "survived", JSON.stringify(first));
      const next = await crossSet.run("m9", [CROSS_B], { worker_mode: "reused" });
      assert.equal(next.status, "survived", JSON.stringify(next));
    });

    test("a check of a file with no source code allows sharing with it", async () => {
      assert.equal(await noHit.check([NOHIT_FIRST, NOHIT_PLAIN]), "reused");
    });

    test("a project with a global set in a test keeps the result of each run", async () => {
      const mode = await leak.check([LEAK_A, LEAK_B]);
      const result = await leak.run("m9", [LEAK_A, LEAK_B], { worker_mode: mode });
      assert.equal(result.status, "survived", JSON.stringify(result));
    });

    test("a kill that only the state of an earlier file of the worker causes is not a kill", async () => {
      const mode = await storeLeak.check([STORE_A, STORE_B]);
      assert.notEqual(mode, "isolated", "the tests agree in one worker without a mutant");
      const control = await storeLeak.run(null, [STORE_A, STORE_B], { worker_mode: mode });
      assert.equal(control.status, "survived", JSON.stringify(control));
      const result = await storeLeak.run("m30", [STORE_A, STORE_B], { worker_mode: mode });
      assert.equal(result.status, "survived", JSON.stringify(result));
      assert.equal(result.worker_mode, "isolated", "the result comes from the isolated run again");
    });

    test("a module cache of an earlier mutant run does not hide a later mutant", async () => {
      assert.equal(await reuse.check([MEMO]), "reused");
      writeFileSync(pidLog, "");
      const first = await reuse.run("m9", [MEMO], { worker_mode: "reused" });
      assert.equal(first.status, "survived", JSON.stringify(first));
      const later = await reuse.run("m40", [MEMO], { worker_mode: "reused" });
      assert.equal(later.status, "killed", JSON.stringify(later));
      assert.equal(new Set(pidsOf(pidLog)).size, 1, "both runs used one reused process");
    });

    test("a module that fails on a second load allows only the shared mode", async () => {
      assert.equal(await registry.check([REGISTRY_A, REGISTRY_B]), "shared");
      writeFileSync(pidLog, "");
      for (let run = 0; run < 3; run += 1) {
        const result = await registry.run("m9", [REGISTRY_A, REGISTRY_B], {
          worker_mode: "shared",
        });
        assert.equal(result.status, "survived", JSON.stringify(result));
        assert.equal(result.worker_mode, "shared", JSON.stringify(result));
      }
      const counts = new Map();
      for (const pid of pidsOf(pidLog)) counts.set(pid, (counts.get(pid) ?? 0) + 1);
      assert.deepEqual(
        [...counts.values()],
        [2, 2, 2],
        "each run used its own process for both of its test files",
      );
    });

    test("a mutant that ends the worker process does not break the next run", async () => {
      const mode = await reuse.check([MEMO, EXIT]);
      assert.equal(mode, "reused");
      const crashed = await reuse.run("m41", [MEMO, EXIT], { worker_mode: mode });
      assert.equal(crashed.status, "killed", JSON.stringify(crashed));
      const next = await reuse.run("m9", [MEMO, EXIT], { worker_mode: mode });
      assert.equal(next.status, "survived", JSON.stringify(next));
    });

    test("a check request with an unknown test is a bad request", async () => {
      const checked = await shared.companion.request({
        v: 1,
        id: 9_999,
        type: "check_workers",
        test_ids: ["shared/none.test.ts::missing"],
        timeout_ms: 10_000,
      });
      assert.equal(checked.type, "error", JSON.stringify(checked));
      assert.equal(checked.error.code, "bad-request", JSON.stringify(checked));
    });
  });
}
