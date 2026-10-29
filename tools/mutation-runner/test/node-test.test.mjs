import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";

import { childRunnerSuite } from "./support/child-runner-suite.mjs";
import { startCompanion } from "./support/companion-process.mjs";
import { mappedTestFilesSuite } from "./support/mapped-test-files-suite.mjs";
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

mappedTestFilesSuite({
  runner: "node-test",
  runtime: process.execPath,
  testImport: 'import assert from "node:assert/strict";\nimport { test } from "node:test";',
  assertion: "assert.equal(add(2, 2), 4)",
});

const CLI_TEST = "test/cli.test.mjs";

/**
 * A project whose tests start Node child processes that load a source file.
 * The workspace path has a space, so the preload URL must survive the
 * NODE_OPTIONS parse.
 */
const createSubprocessProject = () => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow mutation node child-"));
  const root = join(workspace, "project");
  const runDir = join(workspace, "run");
  const main =
    "if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) console.log(add(Number(process.argv[2]), Number(process.argv[3])));\n";
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/math.mjs": `import { pathToFileURL } from "node:url";\nexport const add = (a, b) => a + b;\n${main}`,
    "project/src/forked.mjs": [
      'import { add } from "./math.mjs";',
      "process.send({ sum: add(2, 2) }, () => process.exit(0));",
      "",
    ].join("\n"),
    [`project/${CLI_TEST}`]: [
      'import assert from "node:assert/strict";',
      'import { execFileSync, fork } from "node:child_process";',
      'import { copyFileSync, mkdtempSync, realpathSync, rmSync } from "node:fs";',
      'import { tmpdir } from "node:os";',
      'import { join } from "node:path";',
      'import { test } from "node:test";',
      'import { fileURLToPath } from "node:url";',
      'import { add } from "../src/math.mjs";',
      "const path = (name) => fileURLToPath(new URL(`../src/${name}`, import.meta.url));",
      'test("cli", () => {',
      "  assert.ok(Number.isFinite(add(2, 2)));",
      '  const out = execFileSync(process.execPath, [path("math.mjs"), "2", "2"], { encoding: "utf8" });',
      '  assert.equal(out.trim(), "4");',
      "});",
      'test("fork", async () => {',
      "  assert.ok(Number.isFinite(add(2, 2)));",
      '  const child = fork(path("forked.mjs"), [], { silent: true });',
      '  let output = "";',
      '  child.stdout.on("data", (chunk) => { output += chunk; });',
      '  child.stderr.on("data", (chunk) => { output += chunk; });',
      '  const message = await new Promise((resolve) => child.once("message", resolve));',
      '  await new Promise((resolve) => child.once("close", resolve));',
      '  assert.equal(output, "");',
      "  assert.equal(message.sum, 4);",
      "});",
      'test("copy", (t) => {',
      "  assert.ok(Number.isFinite(add(2, 2)));",
      '  const directory = realpathSync(mkdtempSync(join(tmpdir(), "copied-cli-")));',
      "  t.after(() => rmSync(directory, { recursive: true, force: true }));",
      '  const copied = join(directory, "renamed.mjs");',
      '  copyFileSync(path("math.mjs"), copied);',
      '  const out = execFileSync(process.execPath, [copied, "2", "2"], { encoding: "utf8" });',
      '  assert.equal(out.trim(), "4");',
      "});",
      'test("env", () => {',
      "  assert.ok(Number.isFinite(add(2, 2)));",
      '  assert.deepEqual(Object.keys(process.env).filter((key) => key.startsWith("FALLOW_MUTATION")), []);',
      "});",
      "",
    ].join("\n"),
    "run/files/src/math.mjs": `${PRELUDE}
import { pathToFileURL } from "node:url";
export const add = (a, b) => (__fallow_m_a("s1") ? (a - b) : (__fallow_m_c("s1"), (a + b)));
${main}`,
  };
  for (const [path, text] of Object.entries(files)) {
    const target = join(workspace, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return {
    root,
    runDir,
    pathMap: { "src/math.mjs": "files/src/math.mjs" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

describe("node-test adapter with a test that starts a Node child process", () => {
  let project;
  let companion;
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

  before(async () => {
    project = createSubprocessProject();
    companion = startCompanion();
    const ready = await request({
      type: "init",
      root: project.root,
      runner: "node-test",
      config_path: null,
      run_dir: project.runDir,
      path_map: project.pathMap,
      options: { env: {} },
    });
    assert.equal(ready.type, "ready", JSON.stringify(ready));
  });

  after(async () => {
    await request({ type: "shutdown" }).catch(() => {});
    companion?.stop();
    project?.cleanup();
  });

  test("the dry run passes and the hits of the test process select each test", async () => {
    const dryRun = await request({ type: "dry_run", test_files: [CLI_TEST] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.deepEqual(dryRun.unhandled_errors, []);
    assert.deepEqual(
      dryRun.tests.map(({ name, status }) => ({ name, status })),
      [
        { name: "cli", status: "pass" },
        { name: "fork", status: "pass" },
        { name: "copy", status: "pass" },
        { name: "env", status: "pass" },
      ],
    );
    assert.deepEqual(dryRun.hits, {
      [`${CLI_TEST}::cli`]: ["s1"],
      [`${CLI_TEST}::fork`]: ["s1"],
      [`${CLI_TEST}::copy`]: ["s1"],
      [`${CLI_TEST}::env`]: ["s1"],
    });
  });

  for (const name of ["cli", "fork", "copy"]) {
    test(`the child process of the ${name} test loads the active mutant`, async () => {
      const result = await request({
        type: "run",
        mutant_id: "s1",
        test_ids: [`${CLI_TEST}::${name}`],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
      assert.deepEqual(
        result.tests.map(({ message: _message, ...rest }) => rest),
        [{ test_id: `${CLI_TEST}::${name}`, status: "fail", failure_kind: "assertion" }],
      );
      assert.match(result.tests[0].message, /^AssertionError: /u);
    });
  }

  test("the test process sees no Fallow variable in its environment", async () => {
    const result = await request({
      type: "run",
      mutant_id: "s1",
      test_ids: [`${CLI_TEST}::env`],
      timeout_ms: 10_000,
      bail: true,
    });
    assert.equal(result.status, "survived", JSON.stringify(result));
  });
});
