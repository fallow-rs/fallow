import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const BASELINE = "test/baseline.test.ts";
const MARKER = "DRY_RUN_BASELINE_ASSERTION";
const id = (name) => `${BASELINE}::${name}`;

const baselineTest = (testImport) =>
  [
    'import assert from "node:assert/strict";',
    testImport,
    'test("passes", () => assert.equal(1, 1));',
    `test("fails before any mutant", () => assert.equal(1, 2, "${MARKER}"));`,
    "",
  ].join("\n");

const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const hasBun = bun.status === 0;

const ADAPTERS = [
  ...VITEST_MAJORS.map(({ packageName, version }) => ({
    label: `vitest ${version}`,
    runner: "vitest",
    runtime: process.execPath,
    fixture: { packageName },
    configPath: "vitest.config.mjs",
    testImport: 'import { test } from "vitest";',
  })),
  {
    label: "node-test",
    runner: "node-test",
    runtime: process.execPath,
    fixture: { fixture: "node-basic" },
    configPath: null,
    testImport: 'import { test } from "node:test";',
  },
  {
    label: "bun-test",
    runner: "bun-test",
    runtime: "bun",
    fixture: { fixture: "bun-basic" },
    configPath: null,
    testImport: 'import { test } from "bun:test";',
    // A Bun test process reports no stack frame of the project for this error.
    stackFrames: false,
    skip: !hasBun && process.env.FALLOW_REQUIRE_BUN !== "1",
  },
];

for (const adapter of ADAPTERS) {
  describe(`${adapter.label} dry run of a failing test`, { skip: adapter.skip }, () => {
    let project;
    let companion;
    let dryRun;
    let nextId = 1;
    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      project = createFixtureProject(adapter.fixture);
      const path = join(project.root, BASELINE);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, baselineTest(adapter.testImport));
      companion = startCompanion({ runtime: adapter.runtime });
      const ready = await request({
        type: "init",
        root: project.root,
        runner: adapter.runner,
        config_path: adapter.configPath,
        run_dir: project.runDir,
        path_map: project.pathMap,
        options: { test_files: [BASELINE], env: {} },
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      dryRun = await request({ type: "dry_run", test_files: [BASELINE] });
    });

    after(async () => {
      await request({ type: "shutdown" }).catch(() => {});
      companion?.stop();
      project?.cleanup();
    });

    test("the failed entry keeps its failure kind and assertion, and a passed entry has neither", () => {
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      const byId = new Map(dryRun.tests.map((entry) => [entry.test_id, entry]));
      const passed = byId.get(id("passes"));
      const failed = byId.get(id("fails before any mutant"));
      assert.equal(passed?.status, "pass", JSON.stringify(dryRun.tests));
      assert.equal(passed.message, undefined);
      assert.equal(passed.failure_kind, undefined);
      assert.equal(failed?.status, "fail", JSON.stringify(dryRun.tests));
      assert.equal(failed.failure_kind, "assertion", JSON.stringify(failed));
      const { message } = failed;
      assert.match(message ?? "", new RegExp(`^AssertionError: ${MARKER}`, "u"), message);
      if (adapter.stackFrames !== false) {
        assert.match(message, /\n {4}at .*test\/baseline\.test\.ts:\d+:\d+/u, message);
      }
      assert.ok(!message.includes(project.root), message);
      assert.ok(!message.includes(project.runDir), message);
      assert.ok(!message.includes("node_modules"), message);
    });

    test("the failed dry-run entry has the failure fields of a control run of the same test", async () => {
      const control = await request({
        type: "run",
        mutant_id: null,
        test_ids: [id("fails before any mutant")],
        timeout_ms: 10_000,
        bail: false,
      });
      assert.equal(control.type, "run_result", JSON.stringify(control));
      const failureFieldsOf = ({ failure_kind, message }) => ({ failure_kind, message });
      const dry = dryRun.tests.find((entry) => entry.test_id === id("fails before any mutant"));
      assert.equal(control.tests.length, 1, JSON.stringify(control));
      assert.deepEqual(failureFieldsOf(dry), failureFieldsOf(control.tests[0]));
    });
  });
}
