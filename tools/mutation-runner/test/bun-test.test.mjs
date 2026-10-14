import { spawnSync } from "node:child_process";

import { childRunnerSuite } from "./support/child-runner-suite.mjs";

// The suite needs `bun` on PATH. Without it the suite is skipped.
const bun = spawnSync("bun", ["--version"], { encoding: "utf8" });
const bunVersion = bun.status === 0 ? bun.stdout.trim() : null;
if (bunVersion === null) {
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
