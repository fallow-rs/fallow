import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

// The instrumented code of a dry run is slower than the original code. A test
// that is close to the test timeout of the project must still pass in the dry
// run, or it becomes a baseline failure (#4409).

const SLOW = "test/slow.test.ts";
const id = (name) => `${SLOW}::${name}`;

const VITEST_TIMEOUT_MS = 300;
const VITEST_CONFIG = "vitest.slow.config.mjs";
// The Bun default test timeout. `bunfig.toml` cannot change it.
const BUN_DEFAULT_TIMEOUT_MS = 5000;

/**
 * A test file whose test takes longer than `timeoutMs`. With `hook`, a
 * `beforeAll` hook also takes longer than `timeoutMs`.
 */
const slowTest = (testImport, timeoutMs, hook) => {
  const pause = `await new Promise((resolve) => setTimeout(resolve, ${Math.ceil(timeoutMs * 1.2)}))`;
  return [
    testImport,
    ...(hook ? [`beforeAll(async () => { ${pause}; });`] : []),
    `test("is slower than the project timeout", async () => { ${pause}; });`,
    "",
  ].join("\n");
};

const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const hasBun = bun.status === 0;

const ADAPTERS = [
  ...VITEST_MAJORS.map(({ packageName, version }) => ({
    label: `vitest ${version}`,
    runner: "vitest",
    runtime: process.execPath,
    fixture: { packageName },
    configPath: VITEST_CONFIG,
    timeoutMs: VITEST_TIMEOUT_MS,
    testImport: 'import { beforeAll, test } from "vitest";',
    hook: true,
  })),
  {
    label: "bun-test",
    runner: "bun-test",
    runtime: "bun",
    fixture: { fixture: "bun-basic" },
    configPath: null,
    timeoutMs: BUN_DEFAULT_TIMEOUT_MS,
    testImport: 'import { test } from "bun:test";',
    // The hook would double the run time, and Bun applies one timeout to both.
    hook: false,
    skip: !hasBun && process.env.FALLOW_REQUIRE_BUN !== "1",
  },
];

const writeProjectFile = (root, path, text) => {
  const target = join(root, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, text);
};

for (const adapter of ADAPTERS) {
  describe(
    `${adapter.label} dry run of a test close to its timeout`,
    { skip: adapter.skip },
    () => {
      let project;
      let companion;
      let dryRun;
      let nextId = 1;
      const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

      before(async () => {
        project = createFixtureProject(adapter.fixture);
        writeProjectFile(
          project.root,
          SLOW,
          slowTest(adapter.testImport, adapter.timeoutMs, adapter.hook),
        );
        writeProjectFile(
          project.root,
          VITEST_CONFIG,
          `export default { test: { include: ["${SLOW}"], testTimeout: ${VITEST_TIMEOUT_MS}, hookTimeout: ${VITEST_TIMEOUT_MS} } };\n`,
        );
        companion = startCompanion({ runtime: adapter.runtime });
        const ready = await request({
          type: "init",
          root: project.root,
          runner: adapter.runner,
          config_path: adapter.configPath,
          run_dir: project.runDir,
          path_map: project.pathMap,
          options: { test_files: [SLOW], env: {} },
        });
        assert.equal(ready.type, "ready", JSON.stringify(ready));
        dryRun = await request({ type: "dry_run", test_files: [SLOW] });
      });

      after(async () => {
        await request({ type: "shutdown" }).catch(() => {});
        companion?.stop();
        project?.cleanup();
      });

      test("the dry run scales the project timeout, so the slow test passes", () => {
        assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
        assert.deepEqual(
          dryRun.tests.map(({ test_id, status }) => ({ test_id, status })),
          [{ test_id: id("is slower than the project timeout"), status: "pass" }],
          JSON.stringify(dryRun.tests),
        );
      });
    },
  );
}
