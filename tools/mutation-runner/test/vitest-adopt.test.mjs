import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FIRST = "shared/first.test.ts::adds";
const SECOND = "shared/second.test.ts::adds";
const PROJECT_FILE = "projects/shared.test.ts";
const PROJECT_A = `a::${PROJECT_FILE}::checks per project`;
const PROJECT_B = `b::${PROJECT_FILE}::checks per project`;

/** Starts a companion on `fixture` with `configPath` and sends `init`. */
const startOn = async (fixture, configPath, env = {}) => {
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
  assert.equal(ready.capabilities.adopt, true);
  const run = (mutantId, testIds) =>
    request({
      type: "run",
      mutant_id: mutantId,
      test_ids: testIds,
      timeout_ms: 10_000,
      bail: true,
    });
  return { companion, request, run };
};

/** The `adopt` body of the passing tests of `dryRun`. */
const adoptBody = (dryRun) => ({
  type: "adopt",
  tests: dryRun.tests
    .filter((entry) => entry.status === "pass")
    .map(({ test_id, index }) => ({ test_id, index })),
  worker_modes: dryRun.worker_mode === undefined ? [] : [dryRun.worker_mode],
});

const outcome = ({ status, killed_by }) => ({ status, killed_by });

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} adopt`, () => {
    let fixture;
    let pidDir;
    let pidLog;
    const companions = [];

    before(() => {
      fixture = createFixtureProject({ packageName });
      pidDir = mkdtempSync(join(tmpdir(), "fallow-mutation-adopt-"));
      pidLog = join(pidDir, "pids.log");
      writeFileSync(pidLog, "");
    });

    after(() => {
      for (const companion of companions) companion.stop();
      fixture?.cleanup();
      if (pidDir) rmSync(pidDir, { recursive: true, force: true });
    });

    test("a companion runs the tests of the dry run of another companion", async () => {
      const first = await startOn(fixture, "vitest.shared.config.mjs", { FALLOW_PID_LOG: pidLog });
      const second = await startOn(fixture, "vitest.shared.config.mjs", { FALLOW_PID_LOG: pidLog });
      companions.push(first.companion, second.companion);
      const dryRun = await first.request({ type: "dry_run", test_files: [] });
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.ok(
        dryRun.tests.every((entry) => entry.status !== "pass" || typeof entry.index === "object"),
        JSON.stringify(dryRun.tests),
      );
      assert.deepEqual(dryRun.worker_mode?.files, [
        "shared/first.test.ts",
        "shared/second.test.ts",
      ]);

      const unknown = await second.run("m1", [FIRST]);
      assert.equal(unknown.type, "error", "a companion with no dry run and no adopt knows no test");

      const adopted = await second.request(adoptBody(dryRun));
      assert.deepEqual([adopted.type, adopted.tests], ["adopted", 2], JSON.stringify(adopted));
      for (const mutant of ["m1", "m9"]) {
        const expected = await first.run(mutant, [FIRST, SECOND]);
        const actual = await second.run(mutant, [FIRST, SECOND]);
        assert.equal(actual.type, "run_result", JSON.stringify(actual));
        assert.equal(actual.status, expected.status, mutant);
        // Both tests detect m1, and the first file of the shared worker names
        // the killer, so only the set of possible killers is fixed.
        assert.ok(
          actual.killed_by === undefined ||
            actual.killed_by === null ||
            [FIRST, SECOND].includes(actual.killed_by),
          JSON.stringify(actual),
        );
      }

      writeFileSync(pidLog, "");
      const survived = await second.run("m9", [FIRST, SECOND]);
      assert.equal(survived.status, "survived", JSON.stringify(survived));
      const pids = new Set(readFileSync(pidLog, "utf8").trim().split("\n"));
      assert.equal(pids.size, 1, "the taken worker mode shares one worker for both files");
    });

    test("a taken test keeps its project", async () => {
      const first = await startOn(fixture, "vitest.projects.config.mjs");
      const second = await startOn(fixture, "vitest.projects.config.mjs");
      companions.push(first.companion, second.companion);
      const dryRun = await first.request({ type: "dry_run", test_files: [PROJECT_FILE] });
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      const adopted = await second.request(adoptBody(dryRun));
      assert.equal(adopted.type, "adopted", JSON.stringify(adopted));

      const killed = await second.run("m1", [PROJECT_A]);
      assert.deepEqual(outcome(killed), { status: "killed", killed_by: PROJECT_A });
      const survived = await second.run("m1", [PROJECT_B]);
      assert.equal(survived.status, "survived", "only project a calls the mutated function");
    });

    test("an adopt request with a malformed test is refused", async () => {
      const companion = await startOn(fixture, "vitest.shared.config.mjs");
      companions.push(companion.companion);
      const refused = await companion.request({
        type: "adopt",
        tests: [{ test_id: FIRST }],
        worker_modes: [],
      });
      assert.deepEqual([refused.type, refused.error?.code], ["error", "bad-request"]);
      const wrongTypes = await companion.request({
        type: "adopt",
        tests: [{ test_id: FIRST, index: { project: "", module: 1, task: null } }],
        worker_modes: [],
      });
      assert.deepEqual([wrongTypes.type, wrongTypes.error?.code], ["error", "bad-request"]);
    });
  });
}
