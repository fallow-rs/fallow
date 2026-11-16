import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

// Vitest reads `VITEST_MAX_WORKERS` after the config options. A value in the
// shell of `fallow mutation` must not give each companion more than the one
// worker that the adapter asks for, because the mutation run already starts
// one companion for each unit of `--concurrency`.
const INHERITED_MAX_WORKERS = "4";
const DRY_RUN_LIMIT_MS = 60_000;
const POOL_LOG_POLL_MS = 100;

const readPoolIds = (logPath) =>
  existsSync(logPath) ? [...new Set(readFileSync(logPath, "utf8").trim().split("\n"))] : [];

// A second worker can crash Vitest before the dry run returns. The poll stops
// the wait as soon as the log shows a second pool id, so a regression fails
// fast with the pool ids instead of at the dry-run time limit.
const withLimit = (promise, companion, logPath) => {
  let timer;
  let poll;
  const guard = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`no dry run result:\n${companion.stderr()}`)),
      DRY_RUN_LIMIT_MS,
    );
    poll = setInterval(() => {
      const poolIds = readPoolIds(logPath);
      if (poolIds.some((id) => id !== "1")) {
        reject(new Error(`tests ran in more than one worker: ${poolIds.join(", ")}`));
      }
    }, POOL_LOG_POLL_MS);
  });
  return Promise.race([promise, guard]).finally(() => {
    clearTimeout(timer);
    clearInterval(poll);
  });
};

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with VITEST_MAX_WORKERS in the environment`, () => {
    let fixture;
    let companion;
    let logDir;
    let logPath;
    let dryRun;

    before(async () => {
      fixture = createFixtureProject({ packageName });
      logDir = mkdtempSync(join(tmpdir(), "fallow-pool-"));
      logPath = join(logDir, "pool.log");
      companion = startCompanion({
        setEnv: { VITEST_MAX_WORKERS: INHERITED_MAX_WORKERS, FALLOW_POOL_LOG: logPath },
      });
      let nextId = 1;
      const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
      const ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.workers.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      dryRun = await withLimit(request({ type: "dry_run" }), companion, logPath);
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
      if (logDir !== undefined) rmSync(logDir, { recursive: true, force: true });
    });

    test("every test file runs in the same single worker", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.equal(dryRun.tests.length, 4, JSON.stringify(dryRun));
      assert.deepEqual(readPoolIds(logPath), ["1"]);
    });
  });
}
