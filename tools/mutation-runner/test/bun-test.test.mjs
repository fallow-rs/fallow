import { spawnSync } from "node:child_process";

import { childRunnerSuite } from "./support/child-runner-suite.mjs";
import { mappedTestFilesSuite } from "./support/mapped-test-files-suite.mjs";

// The suite needs `bun` on PATH. Without it the suite is skipped, unless
// `FALLOW_REQUIRE_BUN=1` is set. A CI job that installs Bun sets it, so that
// a failed install fails the job and does not skip the suite.
const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const bunVersion = bun.status === 0 ? bun.stdout.trim() : null;
if (bunVersion === null) {
  if (process.env.FALLOW_REQUIRE_BUN === "1") {
    throw new Error("FALLOW_REQUIRE_BUN=1 is set, but bun is not on PATH");
  }
  process.stderr.write("bun is not on PATH; the bun-test adapter suite is skipped\n");
}

childRunnerSuite({
  runner: "bun-test",
  fixture: "bun-basic",
  runtime: "bun",
  runnerVersion: bunVersion,
  testFiles: ["test/concurrent.test.ts", "test/faults.test.ts", "test/math.test.ts"],
  skip: bunVersion === null,
});

mappedTestFilesSuite({
  runner: "bun-test",
  runtime: "bun",
  testImport: 'import { expect, test } from "bun:test";',
  assertion: "expect(add(2, 2)).toBe(4)",
  skip: bunVersion === null,
});
