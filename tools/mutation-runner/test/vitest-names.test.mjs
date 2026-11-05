import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, createFixtureProject } from "./support/vitest-fixture.mjs";

const FILE = "names/surrogate.test.ts";
const id = (name) => `${FILE}::${name}`;
// JSON.stringify writes a lone surrogate as an escape such as `\ud800`, and a
// strict JSON parser (serde_json in the API) rejects that escape.
const LONE_SURROGATE_ESCAPE = /(?<!\\)(?:\\\\)*\\ud[89a-f][0-9a-f]{2}/iu;

for (const { packageName, version } of VITEST_MAJORS) {
  describe(`vitest ${version} with lone surrogates in test names`, () => {
    let fixture;
    let companion;
    let nextId = 1;
    let dryRunLine;

    const request = (message) => companion.request({ v: 1, id: nextId++, ...message });

    before(async () => {
      fixture = createFixtureProject({ packageName });
      companion = startCompanion();
      const ready = await request({
        type: "init",
        root: fixture.root,
        runner: "vitest",
        config_path: "vitest.names.config.mjs",
        run_dir: fixture.runDir,
        path_map: fixture.pathMap,
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      companion.send({ v: 1, id: nextId++, type: "dry_run", test_files: [FILE] });
      dryRunLine = await companion.nextLine();
    });

    after(() => {
      companion?.stop();
      fixture?.cleanup();
    });

    test("the dry run line has no lone surrogate escape", () => {
      assert.doesNotMatch(dryRunLine, LONE_SURROGATE_ESCAPE);
    });

    test("each lone surrogate becomes U+FFFD, and the ids stay distinct", () => {
      const dryRun = JSON.parse(dryRunLine);
      assert.equal(dryRun.type, "dry_run_result", dryRunLine);
      assert.deepEqual(
        dryRun.tests.map((entry) => entry.test_id),
        [id("lone �"), id("lone � (2)"), id("pair 😀")],
      );
    });

    test("a run selects each test by the id that the dry run gave", async () => {
      const ids = [id("lone �"), id("lone � (2)"), id("pair 😀")];
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
}
