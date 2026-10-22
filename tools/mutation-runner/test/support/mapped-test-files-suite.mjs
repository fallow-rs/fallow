import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";

import { startCompanion } from "./companion-process.mjs";
import { readMutationConstant } from "./vitest-fixture.mjs";

const HIDDEN_TEST = ".github/tests/hidden.test.mjs";
const UNMATCHED_FILE = "src/math.check.mjs";

/**
 * Creates a project with a test file in a dot directory. The default
 * patterns of the runner do not match a path in a dot directory.
 */
const createHiddenTestProject = ({ testImport, assertion }) => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-mutation-hidden-"));
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "outside.test.mjs": 'throw new Error("outside the root");\n',
    "project/src/math.mjs": "export const add = (a, b) => a + b;\n",
    [`project/${UNMATCHED_FILE}`]: 'throw new Error("not a test file");\n',
    [`project/${HIDDEN_TEST}`]: [
      testImport,
      'import { add } from "../../src/math.mjs";',
      `test("adds exactly", () => ${assertion});`,
      "",
    ].join("\n"),
    "run/files/src/math.mjs": `${readMutationConstant("PRELUDE")}
export const add = (a, b) => (__fallow_m_a("e1") ? (a - b) : (__fallow_m_c("e1"), (a + b)));
`,
  };
  for (const [path, text] of Object.entries(files)) {
    const target = join(workspace, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return {
    root: join(workspace, "project"),
    runDir: join(workspace, "run"),
    pathMap: { "src/math.mjs": "files/src/math.mjs" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

/**
 * Checks that a runner that discovers its test files with glob patterns
 * also runs the mapped test files of `options.extra_test_files`, when the
 * default patterns match them outside a dot directory.
 */
export const mappedTestFilesSuite = ({ runner, runtime, testImport, assertion, skip = false }) => {
  describe(`${runner} adapter with the mapped test files of Fallow`, { skip }, () => {
    let project;
    let companion;
    let nextId = 1;
    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
    const init = (options) =>
      request({
        type: "init",
        root: project.root,
        runner,
        config_path: null,
        run_dir: project.runDir,
        path_map: project.pathMap,
        options,
      });

    before(() => {
      project = createHiddenTestProject({ testImport, assertion });
    });

    afterEach(async () => {
      await request({ type: "shutdown" }).catch(() => {});
      companion?.stop();
    });

    after(() => project?.cleanup());

    test("a mapped test file in a dot directory runs and kills the mutant", async () => {
      companion = startCompanion({ runtime });
      const ready = await init({ env: {}, extra_test_files: [HIDDEN_TEST] });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.test_files, [HIDDEN_TEST]);
      const dryRun = await request({ type: "dry_run", test_files: [HIDDEN_TEST] });
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.unhandled_errors, []);
      assert.deepEqual(
        dryRun.tests.map(({ name, status }) => ({ name, status })),
        [{ name: "adds exactly", status: "pass" }],
      );
      const result = await request({
        type: "run",
        mutant_id: "e1",
        test_ids: [`${HIDDEN_TEST}::adds exactly`],
        timeout_ms: 10_000,
        bail: true,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
    });

    test("a mapped file outside the default patterns, a missing file, a node_modules file, or a file outside the root is not listed", async () => {
      companion = startCompanion({ runtime });
      const ready = await init({
        env: {},
        extra_test_files: [
          UNMATCHED_FILE,
          ".github/tests/gone.test.mjs",
          "node_modules/x/a.test.mjs",
          "../outside.test.mjs",
          join(project.root, "..", "outside.test.mjs"),
        ],
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      assert.deepEqual(ready.test_files, []);
    });

    test("extra_test_files that is not a list of strings is a bad request", async () => {
      companion = startCompanion({ runtime });
      const ready = await init({ env: {}, extra_test_files: HIDDEN_TEST });
      assert.equal(ready.type, "error", JSON.stringify(ready));
      assert.equal(ready.error.code, "bad-request");
    });
  });
};
