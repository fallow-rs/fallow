import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";

// Like `bun-test.test.mjs`, the suite is skipped when `bun` is not on PATH.
const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const hasBun = bun.status === 0;
if (!hasBun && process.env.FALLOW_REQUIRE_BUN === "1") {
  throw new Error("FALLOW_REQUIRE_BUN=1 is set, but bun is not on PATH");
}

const REPEATED_FILE = "test/repeated.test.ts";
const TODO_FILE = "test/todo.test.ts";
const IMPORT = 'import { expect, test } from "bun:test";';

const createProject = () => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-mutation-bun-duplicates-"));
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/add.mjs": "export const add = (a, b) => a + b;\n",
    [`project/${REPEATED_FILE}`]: [
      IMPORT,
      'import { add } from "../src/add.mjs";',
      'test("twin", () => expect(add(1, 2)).toBe(3));',
      'test("twin", () => expect(add(2, 2)).toBe(4));',
      'test("solo", () => expect(add(3, 2)).toBe(5));',
      "",
    ].join("\n"),
    [`project/${TODO_FILE}`]: [
      IMPORT,
      'test.todo("twin");',
      'test.todo("twin");',
      'test("solo", () => expect(1).toBe(1));',
      "",
    ].join("\n"),
    "run/files/src/add.mjs": "export const add = (a, b) => a + b;\n",
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

describe("bun-test adapter with tests that share a title", { skip: !hasBun }, () => {
  let project;
  let companion;
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

  before(async () => {
    project = createProject();
    companion = startCompanion({ runtime: "bun" });
    const ready = await request({
      type: "init",
      root: project.root,
      runner: "bun-test",
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

  test("the dry run names the ids of twins that ran", async () => {
    const dryRun = await request({ type: "dry_run", test_files: [REPEATED_FILE] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.deepEqual(dryRun.duplicate_test_ids, [
      `${REPEATED_FILE}::twin`,
      `${REPEATED_FILE}::twin (2)`,
    ]);
  });

  test("todo twins are not duplicates", async () => {
    const dryRun = await request({ type: "dry_run", test_files: [TODO_FILE] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.deepEqual(dryRun.duplicate_test_ids, []);
  });
});
