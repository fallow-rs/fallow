import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { readMutationConstant } from "./support/vitest-fixture.mjs";

const PRELUDE = readMutationConstant("PRELUDE");
const TEST_FILE = "test/cli.test.mjs";

const createProject = (extension, ambiguous) => {
  const workspace = mkdtempSync(join(tmpdir(), "fallow-copied-source-"));
  const root = join(workspace, "project");
  const runDir = join(workspace, "run");
  const source = `src/cli.${extension}`;
  const declaration = "const value = () => 1;";
  const main = extension === "mjs" ? "import.meta.main" : "require.main === module";
  const exports = extension === "mjs" ? "export { value };" : "exports.value = value;";
  const code = `${declaration}\n${exports}\nif (${main}) console.log(value());\n`;
  const instrumented = `${PRELUDE}\n${code.replace(declaration, 'const value = () => (__fallow_m_a("c1") ? 2 : (__fallow_m_c("c1"), 1));')}`;
  const files = {
    "project/package.json": '{"type":"module"}\n',
    [`project/${source}`]: code,
    [`run/files/${source}`]: instrumented,
    [`project/${TEST_FILE}`]: [
      'import assert from "node:assert/strict";',
      'import { spawnSync } from "node:child_process";',
      'import { copyFileSync, existsSync, mkdtempSync, realpathSync, rmSync } from "node:fs";',
      'import { tmpdir } from "node:os";',
      'import { join } from "node:path";',
      'import { test } from "node:test";',
      'import { fileURLToPath } from "node:url";',
      `import { value } from "../${source}";`,
      'test("cli", (t) => {',
      "  assert.ok(Number.isFinite(value()));",
      '  const directory = realpathSync(mkdtempSync(join(tmpdir(), "copied-cli-")));',
      "  t.after(() => rmSync(directory, { recursive: true, force: true }));",
      `  const original = fileURLToPath(new URL("../${source}", import.meta.url));`,
      `  const copied = join(directory, "renamed.${extension}");`,
      "  copyFileSync(original, copied);",
      '  const copy = existsSync(new URL("../copy", import.meta.url));',
      '  const child = spawnSync(process.execPath, [copy ? copied : original], { encoding: "utf8" });',
      '  if (copy && existsSync(new URL("../swallow", import.meta.url))) return;',
      "  assert.equal(child.status, 0, child.stderr);",
      '  assert.equal(child.stdout, "1\\n");',
      "});",
    ].join("\n"),
  };
  const pathMap = { [source]: `files/${source}` };
  if (ambiguous) {
    const duplicate = `src/other.${extension}`;
    files[`project/${duplicate}`] = code;
    files[`run/files/${duplicate}`] = instrumented.replaceAll('"c1"', '"c2"');
    pathMap[duplicate] = `files/${duplicate}`;
  }
  for (const [path, text] of Object.entries(files)) {
    const target = join(workspace, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return { root, runDir, pathMap, workspace };
};

for (const extension of ["mjs", "cjs"]) {
  for (const ambiguous of [false, true]) {
    test(`${extension}: ${ambiguous ? "ambiguous copies fail even when the child result is ignored" : "a renamed copy loads its unique planned source"}`, async (t) => {
      const project = createProject(extension, ambiguous);
      const companion = startCompanion();
      let nextId = 1;
      const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
      t.after(async () => {
        await request({ type: "shutdown" }).catch(() => {});
        companion.stop();
        rmSync(project.workspace, { recursive: true, force: true });
      });
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
      const baseline = await request({ type: "dry_run", test_files: [TEST_FILE] });
      assert.equal(baseline.type, "dry_run_result", JSON.stringify(baseline));
      assert.deepEqual(
        baseline.tests.map(({ status }) => status),
        ["pass"],
      );
      assert.deepEqual(baseline.unhandled_errors, []);
      assert.deepEqual(baseline.hits[`${TEST_FILE}::cli`], ["c1"]);
      const run = () =>
        request({
          type: "run",
          mutant_id: "c1",
          test_ids: [`${TEST_FILE}::cli`],
          timeout_ms: 10000,
          bail: true,
        });
      const original = await run();
      assert.equal(original.status, "killed", JSON.stringify(original));
      writeFileSync(join(project.root, "copy"), "");
      if (ambiguous) writeFileSync(join(project.root, "swallow"), "");
      const copied = await run();
      if (ambiguous) {
        assert.equal(copied.type, "error", JSON.stringify(copied));
        assert.equal(copied.error.code, "overlay-incomplete");
        assert.match(copied.error.message, /ambiguous.*src\/cli.*src\/other/iu);
        const baselineCopy = await request({ type: "dry_run", test_files: [TEST_FILE] });
        assert.equal(baselineCopy.type, "error", JSON.stringify(baselineCopy));
        assert.equal(baselineCopy.error.code, "overlay-incomplete");
        rmSync(join(project.root, "copy"));
        const restored = await run();
        assert.equal(restored.status, "killed", JSON.stringify(restored));
      } else {
        assert.equal(copied.status, "killed", JSON.stringify(copied));
        assert.equal(copied.killed_by, `${TEST_FILE}::cli`);
        assert.match(copied.tests[0].message, /2\\n.*1\\n/su);
      }
    });
  }
}
