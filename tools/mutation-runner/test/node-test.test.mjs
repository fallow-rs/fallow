import { childRunnerSuite } from "./support/child-runner-suite.mjs";

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
