import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

// Vitest reads the first argument of each hook to find the fixtures of a
// `test.extend` test. The setup file of the companion registers hooks for
// each test, so a hook that Vitest cannot parse fails each such test.
const FIXTURE_TEST = "test/extend.test.ts";
const TEST_ID = `${FIXTURE_TEST}::adds with a fixture`;

const FIXTURE_SOURCE = [
  'import { expect, test } from "vitest";',
  'import { add } from "../src/math";',
  "",
  "const it = test.extend({",
  "  base: async ({}, use) => {",
  "    await use(1);",
  "  },",
  "});",
  "",
  'it("adds with a fixture", ({ base }) => {',
  "  expect(add(base, 1)).toBe(2);",
  "});",
  "",
].join("\n");

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with a test.extend fixture`, () => {
    let project;
    let companion;
    let dryRun;
    let nextId = 1;
    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      project = createFixtureProject({ packageName });
      const path = join(project.root, FIXTURE_TEST);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, FIXTURE_SOURCE);
      companion = startCompanion();
      const ready = await request({
        type: "init",
        root: project.root,
        runner: "vitest",
        config_path: "vitest.config.mjs",
        run_dir: project.runDir,
        path_map: project.pathMap,
        options: { test_files: [FIXTURE_TEST], env: {} },
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      dryRun = await request({ type: "dry_run", test_files: [FIXTURE_TEST] });
    });

    after(async () => {
      await request({ type: "shutdown" }).catch(() => {});
      companion?.stop();
      project?.cleanup();
    });

    test("the dry run passes the fixture test and records its hits", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.unhandled_errors ?? [], [], JSON.stringify(dryRun));
      const entry = dryRun.tests.find((candidate) => candidate.test_id === TEST_ID);
      assert.equal(entry?.status, "pass", JSON.stringify(dryRun.tests));
      assert.ok(dryRun.hits[TEST_ID]?.includes("m1"), JSON.stringify(dryRun.hits));
    });

    // The dry-run test guards the fixture parse. This test checks that a
    // fixture test also reports a kill when a mutant is active.
    test("a mutant run of the fixture test kills the mutant of add", async () => {
      const run = await request({
        type: "run",
        mutant_id: "m1",
        test_ids: [TEST_ID],
        timeout_ms: 10_000,
        bail: false,
      });
      assert.equal(run.type, "run_result", JSON.stringify(run));
      assert.equal(run.tests[0]?.status, "fail", JSON.stringify(run));
      assert.equal(run.tests[0]?.failure_kind, "assertion", JSON.stringify(run));
    });
  });
}
