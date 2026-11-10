import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import {
  VITEST_MAJORS,
  createFixtureProject,
  readMutationConstant,
} from "./support/vitest-fixture.mjs";

const FLAG_SOURCE = "meta-env/flag.ts";
const FLAG_TEST = "meta-env/flag.test.ts";
const OFF = "is off without the variable";
const STUBBED = "reads a stubbed variable";
const id = (name) => `${FLAG_TEST}::${name}`;

// The mutant `m50` changes `===` to `!==` in the `import.meta.env` read.
const INSTRUMENTED_FLAG = `// @ts-nocheck
${readMutationConstant("PRELUDE")}
export const readFlag = (): string => ((__fallow_m_a("m50") ? (import.meta.env.VITE_FLAG !== "on") : (__fallow_m_c("m50"), (import.meta.env.VITE_FLAG === "on"))) ? "on" : "off");
`;

const startProject = async (packageName) => {
  const fixture = createFixtureProject({ packageName });
  const target = `files/${FLAG_SOURCE}`;
  mkdirSync(join(fixture.runDir, "files/meta-env"), { recursive: true });
  writeFileSync(join(fixture.runDir, target), INSTRUMENTED_FLAG);
  const companion = startCompanion();
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const ready = await request({
    type: "init",
    root: fixture.root,
    runner: "vitest",
    config_path: "vitest.meta-env.config.mjs",
    run_dir: fixture.runDir,
    path_map: { ...fixture.pathMap, [FLAG_SOURCE]: target },
    options: {},
  });
  return {
    ready,
    request,
    stop: () => {
      companion.stop();
      fixture.cleanup();
    },
  };
};

// Vitest rewrites each `import.meta.env` read of a module to the environment
// object of the worker, which `vi.stubEnv` changes. An instrumented file
// must get the same rewrite, or a stubbed value is not visible to it.
for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with an import.meta.env read in an instrumented file`, () => {
    let project;
    let dryRun;

    before(async () => {
      project = await startProject(packageName);
      dryRun = await project.request({ type: "dry_run" });
    });

    after(() => project?.stop());

    test("a vi.stubEnv value is visible through import.meta.env in the baseline", () => {
      assert.equal(project.ready.type, "ready", JSON.stringify(project.ready));
      assert.equal(dryRun.type, "dry_run_result", JSON.stringify(dryRun));
      assert.deepEqual(dryRun.errors ?? [], [], JSON.stringify(dryRun));
      assert.deepEqual(
        Object.fromEntries(dryRun.tests.map((entry) => [entry.name, entry.status])),
        { [OFF]: "pass", [STUBBED]: "pass" },
        JSON.stringify(dryRun),
      );
      assert.deepEqual(dryRun.hits[id(STUBBED)], ["m50"], JSON.stringify(dryRun.hits));
    });

    test("a mutant in the import.meta.env read is killed", async () => {
      const result = await project.request({
        type: "run",
        mutant_id: "m50",
        test_ids: [id(OFF), id(STUBBED)],
        timeout_ms: 5000,
        bail: false,
      });
      assert.equal(result.status, "killed", JSON.stringify(result));
    });
  });
}
