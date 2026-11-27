import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, afterEach, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { readMutationConstant } from "./support/vitest-fixture.mjs";

const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const skip = bun.status !== 0;

const WEB_TEST = "packages/web/src/greet.test.ts";
const PLAIN_TEST = "packages/plain/src/plain.test.ts";
const SETUP_ERROR = 'throw new Error("a bunfig.toml above the root ran");\n';

/**
 * Creates a Bun workspace. `packages/web` has its own `bunfig.toml` with a
 * preload. `packages/plain` has none, so the root `bunfig.toml` applies. A
 * `bunfig.toml` above the project root must not apply.
 */
const createWorkspace = () => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-mutation-bunfig-"));
  const files = {
    "bunfig.toml": '[test]\npreload = ["./outside.ts"]\n',
    "outside.ts": SETUP_ERROR,
    "project/package.json": '{"name":"p","private":true,"workspaces":["packages/*"]}\n',
    "project/bunfig.toml": '[test]\npreload = ["./root-setup.ts"]\n',
    "project/root-setup.ts": "globalThis.ROOT_SETUP = true;\n",
    "project/packages/web/package.json": '{"name":"web","private":true}\n',
    "project/packages/web/bunfig.toml": '[test]\npreload = ["./setup.ts"]\n',
    "project/packages/web/setup.ts": 'globalThis.GREETING = "hello";\n',
    "project/packages/web/src/greet.ts":
      "export const greet = (name: string): string => `${globalThis.GREETING} ${name}`;\n",
    [`project/${WEB_TEST}`]: [
      'import { expect, test } from "bun:test";',
      'import { greet } from "./greet";',
      'test("greet names the person", () => {',
      '  expect(greet("ada")).toBe("hello ada");',
      '  expect(process.cwd().endsWith("packages/web")).toBe(true);',
      "});",
      "",
    ].join("\n"),
    "project/packages/plain/package.json": '{"name":"plain","private":true}\n',
    [`project/${PLAIN_TEST}`]: [
      'import { expect, test } from "bun:test";',
      'test("the root preload runs", () => {',
      "  expect(globalThis.ROOT_SETUP).toBe(true);",
      "});",
      "",
    ].join("\n"),
    "run/files/packages/web/src/greet.ts": `${readMutationConstant("PRELUDE")}
export const greet = (name: string): string => (__fallow_m_a("e1") ? (\`\${globalThis.GREETING}\`) : (__fallow_m_c("e1"), (\`\${globalThis.GREETING} \${name}\`)));
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
    pathMap: { "packages/web/src/greet.ts": "files/packages/web/src/greet.ts" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

describe("bun-test adapter in a workspace with a package bunfig.toml", { skip }, () => {
  let project;
  let companion;
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

  before(() => {
    project = createWorkspace();
  });

  afterEach(async () => {
    await request({ type: "shutdown" }).catch(() => {});
    companion?.stop();
  });

  after(() => project?.cleanup());

  test("each test file runs in the folder of its nearest bunfig.toml", async () => {
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
    assert.deepEqual(ready.test_files.toSorted(), [PLAIN_TEST, WEB_TEST]);

    const dryRun = await request({ type: "dry_run", test_files: [WEB_TEST, PLAIN_TEST] });
    assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
    assert.deepEqual(dryRun.unhandled_errors, []);
    assert.deepEqual(
      dryRun.tests
        .map(({ test_id, status }) => ({ test_id, status }))
        .toSorted((a, b) => (a.test_id < b.test_id ? -1 : 1)),
      [
        { test_id: `${PLAIN_TEST}::the root preload runs`, status: "pass" },
        { test_id: `${WEB_TEST}::greet names the person`, status: "pass" },
      ],
    );

    const result = await request({
      type: "run",
      mutant_id: "e1",
      test_ids: [`${WEB_TEST}::greet names the person`],
      timeout_ms: 10_000,
      bail: true,
    });
    assert.equal(result.status, "killed", JSON.stringify(result));
  });
});
