import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";

import { childRunnerSuite } from "./support/child-runner-suite.mjs";
import { startCompanion } from "./support/companion-process.mjs";
import { readMutationConstant } from "./support/vitest-fixture.mjs";

childRunnerSuite({
  runner: "node-test",
  fixture: "node-basic",
  runtime: process.execPath,
  runnerVersion: process.versions.node,
  testFiles: [
    "test/concurrent.test.ts",
    "test/faults.test.ts",
    "test/hook.test.ts",
    "test/math.test.ts",
  ],
});

const PRELUDE = readMutationConstant("PRELUDE");
const COLOR_TEST = "test/color.test.ts";

/**
 * A project with a TypeScript enum. Type stripping refuses an enum, so the
 * test file loads only with `--experimental-transform-types`. A setup file
 * outside the test patterns sets a global value when `--import` loads it.
 */
const createEnumProject = () => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-mutation-node-flags-"));
  const root = join(workspace, "project");
  const runDir = join(workspace, "run");
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/color.ts":
      "export enum Color { Red = 1 }\nexport const isRed = (value: number): boolean => value === Color.Red;\n",
    "project/support/setup.mjs": 'globalThis.fallowSetup = "loaded";\n',
    "project/support/noisy.mjs": 'console.error("Error: retrying the connection");\n',
    [`project/${COLOR_TEST}`]: [
      'import assert from "node:assert/strict";',
      'import { test } from "node:test";',
      'import { isRed } from "../src/color.ts";',
      'test("red", () => assert.equal(isRed(1), true));',
      'test("setup", () => assert.equal(globalThis.fallowSetup, "loaded"));',
      "",
    ].join("\n"),
    "run/files/src/color.ts": `// @ts-nocheck
${PRELUDE}
export enum Color { Red = 1 }
export const isRed = (value: number): boolean => (__fallow_m_a("e1") ? (value !== Color.Red) : (__fallow_m_c("e1"), (value === Color.Red)));
`,
  };
  for (const [path, text] of Object.entries(files)) {
    const target = join(workspace, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return {
    root,
    runDir,
    pathMap: { "src/color.ts": "files/src/color.ts" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

describe("node-test adapter with the Node flags of the test script", () => {
  let project;
  let companion;
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const init = (options) =>
    request({
      type: "init",
      root: project.root,
      runner: "node-test",
      config_path: null,
      run_dir: project.runDir,
      path_map: project.pathMap,
      options,
    });

  before(() => {
    project = createEnumProject();
  });

  afterEach(async () => {
    await request({ type: "shutdown" }).catch(() => {});
    companion?.stop();
  });

  after(() => project?.cleanup());

  test("the test processes get node_args, with a relative --import from the root", async () => {
    companion = startCompanion();
    const ready = await init({
      env: {},
      node_args: ["--experimental-transform-types", "--import", "./support/setup.mjs"],
    });
    assert.equal(ready.type, "ready", JSON.stringify(ready));
    const dryRun = await request({ type: "dry_run", test_files: [COLOR_TEST] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.deepEqual(dryRun.unhandled_errors, []);
    assert.deepEqual(
      dryRun.tests.map(({ name, status }) => ({ name, status })),
      [
        { name: "red", status: "pass" },
        { name: "setup", status: "pass" },
      ],
    );
    const result = await request({
      type: "run",
      mutant_id: "e1",
      test_ids: [`${COLOR_TEST}::red`],
      timeout_ms: 10_000,
      bail: true,
    });
    assert.equal(result.status, "killed", JSON.stringify(result));
  });

  test("a test file that does not load gives a message that starts with the error line", async () => {
    companion = startCompanion();
    const ready = await init({ env: {} });
    assert.equal(ready.type, "ready", JSON.stringify(ready));
    const dryRun = await request({ type: "dry_run", test_files: [COLOR_TEST] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.equal(dryRun.unhandled_errors.length, 1, JSON.stringify(dryRun));
    assert.match(
      dryRun.unhandled_errors[0].message,
      /^SyntaxError \[ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX\]: TypeScript enum is not supported/u,
    );
  });

  test("an earlier unrelated error line in the stderr does not replace the load error", async () => {
    companion = startCompanion();
    const ready = await init({ env: {}, node_args: ["--import", "./support/noisy.mjs"] });
    assert.equal(ready.type, "ready", JSON.stringify(ready));
    const dryRun = await request({ type: "dry_run", test_files: [COLOR_TEST] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.equal(dryRun.unhandled_errors.length, 1, JSON.stringify(dryRun));
    assert.match(
      dryRun.unhandled_errors[0].message,
      /^SyntaxError \[ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX\]: TypeScript enum is not supported/u,
    );
  });

  test("node_args that is not a list of strings is a bad request", async () => {
    companion = startCompanion();
    const ready = await init({ env: {}, node_args: "--experimental-transform-types" });
    assert.equal(ready.type, "error", JSON.stringify(ready));
    assert.equal(ready.error.code, "bad-request");
  });
});
