import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { readMutationConstant } from "./support/vitest-fixture.mjs";

const PRELUDE = readMutationConstant("PRELUDE");
const FILE = "test/names.test.mjs";
const id = (name) => `${FILE}::${name}`;

/** A project with test names that hold a lone UTF-16 surrogate. */
const createProject = () => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-mutation-node-names-"));
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/add.mjs": "export const add = (a, b) => a + b;\n",
    [`project/${FILE}`]: [
      'import assert from "node:assert/strict";',
      'import { test } from "node:test";',
      'import { add } from "../src/add.mjs";',
      'test("lone \\uD800", () => assert.equal(add(1, 2), 3));',
      'test("lone \\uD801", () => assert.equal(add(2, 2), 4));',
      'test("pair \\uD83D\\uDE00", () => assert.equal(add(3, 2), 5));',
      "",
    ].join("\n"),
    "run/files/src/add.mjs": `${PRELUDE}
export const add = (a, b) => (__fallow_m_a("m1") ? (a - b) : (__fallow_m_c("m1"), (a + b)));
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
    pathMap: { "src/add.mjs": "files/src/add.mjs" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

describe("node-test adapter with lone surrogates in test names", () => {
  let project;
  let companion;
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

  before(async () => {
    project = createProject();
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

  after(() => {
    companion?.stop();
    project?.cleanup();
  });

  test("a run selects each test by the id that the dry run gave", async () => {
    const dryRun = await request({ type: "dry_run", test_files: [FILE] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    const ids = [id("lone �"), id("lone � (2)"), id("pair 😀")];
    assert.deepEqual(
      dryRun.tests.map((entry) => entry.test_id),
      ids,
    );
    const result = await request({
      type: "run",
      mutant_id: "m1",
      test_ids: ids,
      timeout_ms: 10_000,
      bail: false,
    });
    assert.equal(result.type, "run_result", JSON.stringify(result));
    assert.deepEqual(
      result.tests.map((entry) => [entry.test_id, entry.status]),
      ids.map((testId) => [testId, "fail"]),
    );
  });
});
