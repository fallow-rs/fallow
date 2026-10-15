import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { setTimeout as sleep } from "node:timers/promises";

import { startCompanion } from "./support/companion-process.mjs";

const EXIT_BOUND_MS = 5000;

// A test that starts a worker process, writes the worker pid, and never ends.
const HANGING_TEST = `import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { test } from "node:test";

test("hangs", async () => {
  const worker = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
    stdio: "ignore",
  });
  writeFileSync(process.env.FALLOW_TEST_WORKER_PID_FILE, String(worker.pid));
  await new Promise(() => setInterval(() => {}, 1000));
});
`;

const isAlive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

const waitFor = async (condition, milliseconds) => {
  const deadline = Date.now() + milliseconds;
  while (Date.now() < deadline) {
    if (condition()) return true;
    await sleep(50);
  }
  return condition();
};

const readPid = (path) => {
  try {
    const pid = Number.parseInt(readFileSync(path, "utf8"), 10);
    return Number.isInteger(pid) && pid > 0 ? pid : null;
  } catch {
    return null;
  }
};

test(
  "the end of stdin during a request kills the process group of the companion",
  { skip: process.platform === "win32" && "process groups are Unix only" },
  async () => {
    const base = mkdtempSync(join(tmpdir(), "fallow-mutation-orphan-"));
    const root = join(base, "project");
    const runDir = join(base, "run");
    const pidFile = join(base, "worker.pid");
    mkdirSync(join(root, "test"), { recursive: true });
    mkdirSync(runDir);
    writeFileSync(join(root, "package.json"), '{"type":"module"}\n');
    writeFileSync(join(root, "test/hang.test.mjs"), HANGING_TEST);
    const companion = startCompanion({ detached: true });
    let workerPid = null;
    try {
      const ready = await companion.request({
        v: 1,
        id: 1,
        type: "init",
        root,
        runner: "node-test",
        config_path: null,
        run_dir: runDir,
        path_map: {},
        options: { env: { FALLOW_TEST_WORKER_PID_FILE: pidFile } },
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
      companion.send({ v: 1, id: 2, type: "dry_run", test_files: ["test/hang.test.mjs"] });
      assert.ok(
        await waitFor(() => readPid(pidFile) !== null, 30_000),
        `the test did not start its worker:\n${companion.stderr()}`,
      );
      workerPid = readPid(pidFile);

      companion.child.stdin.end();

      const exit = await Promise.race([companion.exited, sleep(EXIT_BOUND_MS).then(() => null)]);
      assert.notEqual(exit, null, "the companion did not exit after the end of stdin");
      assert.ok(
        await waitFor(() => !isAlive(workerPid), EXIT_BOUND_MS),
        "the worker of the test outlived the companion",
      );
    } finally {
      if (workerPid !== null && isAlive(workerPid)) process.kill(workerPid, "SIGKILL");
      try {
        process.kill(-companion.child.pid, "SIGKILL");
      } catch {
        // The process group is gone.
      }
      rmSync(base, { recursive: true, force: true });
    }
  },
);
