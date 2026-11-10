import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, describe, test } from "node:test";

import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, readMutationConstant } from "./support/vitest-fixture.mjs";

const PRELUDE = readMutationConstant("PRELUDE");
const PACKAGE_ROOT = fileURLToPath(new URL("..", import.meta.url));
const CLOCK_MS = 1_767_225_600_000;

const ORIGINAL = `import { spawn } from "node:child_process";
import { appendFileSync } from "node:fs";

let calls = 0;

export function next(): number {
  calls = calls + 1;
  return calls;
}

export async function later(n: number): Promise<number> {
  await new Promise((resolve) => setTimeout(resolve, 5));
  return n + 1;
}

export function age(born: number): number {
  return Date.now() - born;
}

export function roll(): number {
  return Math.floor(Math.random() * 6) + 1;
}

export function token(): string {
  return crypto.randomUUID();
}

const OFFSET = 1 + 1;

export function shift(n: number): number {
  return n + OFFSET;
}

export function fail(n: number): number {
  if (n > 0) throw new RangeError("too big");
  return n;
}

export function drain(limit: number): number {
  const step = 1;
  let count = 0;
  while (count < limit) {
    count += step;
  }
  return count;
}

export async function pause(): Promise<number> {
  await new Promise((resolve) => setTimeout(resolve, 1));
  return 1;
}

export function tally(): number {
  globalThis.fallowTally = (globalThis.fallowTally ?? 0) + 1;
  return globalThis.fallowTally;
}

export function busy(): number {
  const end = Date.now() + 20;
  while (Date.now() < end) {}
  return 1;
}

export function spawnLong(): number {
  const child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 60000)"], { stdio: "inherit" });
  if (process.env.REPLAY_PID_FILE) appendFileSync(process.env.REPLAY_PID_FILE, \`\${child.pid}\\n\`);
  return 1;
}
export async function holdOn(): Promise<number> {
  const child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 60000)"], { stdio: "ignore" });
  if (process.env.REPLAY_PID_FILE) {
    appendFileSync(process.env.REPLAY_PID_FILE, \`\${process.pid}\\n\${child.pid}\\n\`);
  }
  await new Promise((resolve) => setTimeout(resolve, 60000));
  return 1;
}
`;

const m = (id, mutated, original) =>
  `(__fallow_m_a("${id}") ? (${mutated}) : (__fallow_m_c("${id}"), (${original})))`;

const INSTRUMENTED = `// @ts-nocheck
${PRELUDE}
import { spawn } from "node:child_process";
import { appendFileSync } from "node:fs";

let calls = 0;

export function next(): number {
  calls = ${m("r1", "calls - 1", "calls + 1")};
  return calls;
}

export async function later(n: number): Promise<number> {
  await new Promise((resolve) => setTimeout(resolve, 5));
  return ${m("r2", "n - 1", "n + 1")};
}

export function age(born: number): number {
  return ${m("r3", "Date.now() + born", "Date.now() - born")};
}

export function roll(): number {
  return ${m("r4", "Math.floor(Math.random() * 6) - 1", "Math.floor(Math.random() * 6) + 1")};
}

export function token(): string {
  return ${m("r5", 'crypto.randomUUID() + "x"', "crypto.randomUUID()")};
}

const OFFSET = ${m("r6", "1 - 1", "1 + 1")};

export function shift(n: number): number {
  return n + OFFSET;
}

export function fail(n: number): number {
  if (${m("r7", "n >= 0", "n > 0")}) throw new RangeError("too big");
  return n;
}

export function drain(limit: number): number {
  const step = ${m("r8", "0", "1")};
  let count = 0;
  while (${m("r9", "count < limit", "count < limit")}) {
    count += step;
  }
  return count;
}

export async function pause(): Promise<number> {
  await new Promise((resolve) => setTimeout(resolve, ${m("r10", "60000", "1")}));
  return 1;
}

export function tally(): number {
  globalThis.fallowTally = (globalThis.fallowTally ?? 0) + ${m("r11", "10", "1")};
  return globalThis.fallowTally;
}

export function busy(): number {
  const end = Date.now() + 20;
  while (Date.now() < end) {}
  return ${m("r12", "2", "1")};
}

export function spawnLong(): number {
  const child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 60000)"], { stdio: "inherit" });
  if (process.env.REPLAY_PID_FILE) appendFileSync(process.env.REPLAY_PID_FILE, \`\${child.pid}\\n\`);
  return ${m("r13", "2", "1")};
}
export async function holdOn(): Promise<number> {
  const child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 60000)"], { stdio: "ignore" });
  if (process.env.REPLAY_PID_FILE) {
    appendFileSync(process.env.REPLAY_PID_FILE, \`\${process.pid}\\n\${child.pid}\\n\`);
  }
  await new Promise((resolve) => setTimeout(resolve, 60000));
  return ${m("r14", "2", "1")};
}
`;

const TEST_FILE = `import { test } from "node:test";
import assert from "node:assert/strict";
import { shift } from "../src/replay.ts";
test("shift", () => assert.equal(shift(1), 3));
`;

const VITEST_TEST_FILE = `import { expect, test } from "vitest";
import { shift } from "../src/replay";
test("shift", () => expect(shift(1)).toBe(3));
`;

/** A project with one source file, its instrumented copy and one test file. */
const createReplayProject = ({ runner, packageName }) => {
  const workspace = mkdtempSync(join(tmpdir(), `fallow-replay-${runner}-`));
  const root = join(workspace, "project");
  const runDir = join(workspace, "run");
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/replay.ts": ORIGINAL,
    "run/files/src/replay.ts": INSTRUMENTED,
    "project/test/replay.test.ts": runner === "vitest" ? VITEST_TEST_FILE : TEST_FILE,
  };
  for (const [path, text] of Object.entries(files)) {
    const target = join(workspace, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  if (runner === "vitest") {
    mkdirSync(join(root, "node_modules"));
    symlinkSync(
      join(PACKAGE_ROOT, "node_modules", packageName),
      join(root, "node_modules", "vitest"),
      "dir",
    );
  }
  return {
    root,
    runDir,
    pathMap: { "src/replay.ts": "files/src/replay.ts" },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

const startProject = async (options, setEnv = {}) => {
  const project = createReplayProject(options);
  // The companion leads its own process group, as the API starts it on Unix.
  const companion = startCompanion({ detached: process.platform !== "win32", setEnv });
  let nextId = 1;
  const request = (message) => companion.request({ v: 1, id: nextId++, ...message });
  const ready = await request({
    type: "init",
    root: project.root,
    runner: options.runner,
    config_path: null,
    run_dir: project.runDir,
    path_map: project.pathMap,
    options: {},
  });
  return {
    ready,
    companion,
    replay: (fields) =>
      request({
        type: "replay",
        file: "src/replay.ts",
        args: [],
        calls: 3,
        call_timeout_ms: 2000,
        clock_ms: CLOCK_MS,
        seed: 1,
        ...fields,
      }),
    stop: () => {
      companion.stop();
      project.cleanup();
    },
  };
};

const valuesOf = (result, side) =>
  result.calls.filter((call) => call.side === side).map((call) => call.value);
const outcomesOf = (result, side) =>
  result.calls.filter((call) => call.side === side).map((call) => call.outcome);

const replaySuite = (label, options) => {
  describe(`replay with ${label}`, () => {
    let project;

    before(async () => {
      project = await startProject(options);
    });
    after(() => project?.stop());

    test("the ready message states the replay capability", () => {
      assert.equal(project.ready.type, "ready", JSON.stringify(project.ready));
      assert.equal(project.ready.capabilities.replay, true);
    });

    test("the calls alternate between the original and the mutant", async () => {
      const result = await project.replay({ export: "next", mutant_id: "r1" });
      assert.equal(result.type, "replay_result", JSON.stringify(result));
      assert.deepEqual(
        result.calls.map((call) => `${call.side}:${call.index}`),
        ["original:1", "mutant:1", "original:2", "mutant:2", "original:3", "mutant:3"],
      );
    });

    test("each call gets a fresh module instance, so module state does not carry over", async () => {
      const result = await project.replay({ export: "next", mutant_id: "r1" });
      assert.deepEqual(valuesOf(result, "original"), [1, 1, 1]);
      assert.deepEqual(valuesOf(result, "mutant"), [-1, -1, -1]);
    });

    test("an async function gives its awaited value on each side", async () => {
      const result = await project.replay({ export: "later", mutant_id: "r2", args: [1] });
      assert.deepEqual(valuesOf(result, "original"), [2, 2, 2]);
      assert.deepEqual(valuesOf(result, "mutant"), [0, 0, 0]);
    });

    test("the clock is fixed, so a value from the clock is the same on each call", async () => {
      const result = await project.replay({ export: "age", mutant_id: "r3", args: [10] });
      const original = valuesOf(result, "original");
      const mutant = valuesOf(result, "mutant");
      // The runner can read the clock while it loads the module, so the
      // value is near the fixed start, and the same on each call of a side.
      assert.equal(new Set(original).size, 1);
      assert.equal(new Set(mutant).size, 1);
      assert.equal(mutant[0] - original[0], 20);
      assert.ok(Math.abs(original[0] - (CLOCK_MS - 10)) < 1000, String(original[0]));
    });

    test("Math.random has a fixed seed, so each call of a side gets the same value", async () => {
      const result = await project.replay({ export: "roll", mutant_id: "r4" });
      const original = valuesOf(result, "original");
      const mutant = valuesOf(result, "mutant");
      assert.equal(new Set(original).size, 1);
      assert.equal(new Set(mutant).size, 1);
      assert.equal(original[0] - mutant[0], 2);
    });

    test("a random source without a seed gives other values on each call", async () => {
      const result = await project.replay({ export: "token", mutant_id: "r5" });
      assert.equal(new Set(valuesOf(result, "original")).size, 3);
    });

    test("a mutant in top-level code is active when the module loads", async () => {
      const result = await project.replay({ export: "shift", mutant_id: "r6", args: [1] });
      assert.deepEqual(valuesOf(result, "original"), [3, 3, 3]);
      assert.deepEqual(valuesOf(result, "mutant"), [1, 1, 1]);
    });

    test("a thrown error is an outcome with its name and message", async () => {
      const result = await project.replay({ export: "fail", mutant_id: "r7", args: [0] });
      assert.deepEqual(outcomesOf(result, "original"), ["returned", "returned", "returned"]);
      assert.deepEqual(outcomesOf(result, "mutant"), ["threw", "threw", "threw"]);
      assert.deepEqual(valuesOf(result, "mutant")[0], {
        $replay: "error",
        name: "RangeError",
        message: "too big",
      });
    });

    test("a mutant in a synchronous endless loop times out and the companion stays alive", async () => {
      const result = await project.replay({
        export: "drain",
        mutant_id: "r8",
        args: [3],
        call_timeout_ms: 300,
      });
      assert.equal(result.type, "replay_result", JSON.stringify(result));
      assert.deepEqual(valuesOf(result, "original"), [3, 3, 3]);
      assert.deepEqual(outcomesOf(result, "mutant"), ["timed_out", "timed_out", "timed_out"]);
      const later = await project.replay({ export: "next", mutant_id: "r1" });
      assert.equal(later.type, "replay_result");
    });

    test("an async call past the call timeout times out", async () => {
      const result = await project.replay({
        export: "pause",
        mutant_id: "r10",
        call_timeout_ms: 300,
      });
      assert.deepEqual(outcomesOf(result, "original"), ["returned", "returned", "returned"]);
      assert.deepEqual(outcomesOf(result, "mutant"), ["timed_out", "timed_out", "timed_out"]);
    });

    test("a global that one call sets never reaches the next call or the other side", async () => {
      const result = await project.replay({ export: "tally", mutant_id: "r11" });
      assert.deepEqual(valuesOf(result, "original"), [1, 1, 1]);
      assert.deepEqual(valuesOf(result, "mutant"), [10, 10, 10]);
    });

    test("a busy wait on the clock ends, because each clock read moves the fixed clock", async () => {
      const result = await project.replay({
        export: "busy",
        mutant_id: "r12",
        call_timeout_ms: 2000,
      });
      assert.deepEqual(outcomesOf(result, "original"), ["returned", "returned", "returned"]);
      assert.deepEqual(valuesOf(result, "mutant"), [2, 2, 2]);
    });

    test("a child process that outlives its call neither delays nor fails the replay", async () => {
      const started = performance.now();
      const result = await project.replay({
        export: "spawnLong",
        mutant_id: "r13",
        calls: 1,
        call_timeout_ms: 2000,
      });
      assert.deepEqual(valuesOf(result, "original"), [1]);
      assert.deepEqual(valuesOf(result, "mutant"), [2]);
      assert.ok(performance.now() - started < 20_000, "the replay waited for the child");
    });

    test(
      "a kill of the companion group also ends the processes of a replay",
      { skip: process.platform === "win32" && "no process group on Windows" },
      async () => {
        const pidFile = join(tmpdir(), `fallow-replay-pids-${process.pid}-${Date.now()}`);
        writeFileSync(pidFile, "");
        const other = await startProject(options, { REPLAY_PID_FILE: pidFile });
        // The call waits for a minute, so the kill comes during the call.
        other
          .replay({ export: "holdOn", mutant_id: "r14", calls: 1, call_timeout_ms: 120_000 })
          .catch(() => {});
        while (readFileSync(pidFile, "utf8").trim() === "") {
          await new Promise((resolve) => setTimeout(resolve, 100));
        }
        const pids = readFileSync(pidFile, "utf8").trim().split("\n").map(Number);
        process.kill(-other.companion.child.pid, "SIGKILL");
        await other.companion.exited;
        await new Promise((resolve) => setTimeout(resolve, 500));
        const alive = pids.filter((pid) => {
          try {
            process.kill(pid, 0);
            return true;
          } catch {
            return false;
          }
        });
        for (const pid of alive) process.kill(pid, "SIGKILL");
        other.stop();
        rmSync(pidFile, { force: true });
        assert.deepEqual(alive, [], "a child of a replay call outlived the companion");
      },
    );

    test("a call limit that a timer cannot hold is a bad request", async () => {
      const result = await project.replay({
        export: "next",
        mutant_id: "r1",
        call_timeout_ms: 2 ** 31,
      });
      assert.equal(result.type, "error");
      assert.equal(result.error.code, "bad-request");
    });

    test("an export that is not a function is not callable", async () => {
      const result = await project.replay({ export: "missing", mutant_id: "r1" });
      assert.deepEqual(outcomesOf(result, "original"), [
        "not_callable",
        "not_callable",
        "not_callable",
      ]);
    });

    test("a request with arguments that do not decode is a bad request", async () => {
      const result = await project.replay({ export: "next", mutant_id: "r1", args: "1" });
      assert.equal(result.type, "error");
      assert.equal(result.error.code, "bad-request");
    });

    test("a file outside the path map is a bad request", async () => {
      const result = await project.replay({
        export: "next",
        mutant_id: "r1",
        file: "src/other.ts",
      });
      assert.equal(result.type, "error");
      assert.equal(result.error.code, "bad-request");
      assert.match(result.error.message, /path_map/u);
    });
  });
};

for (const { packageName, version } of VITEST_MAJORS) {
  replaySuite(`vitest ${version}`, { runner: "vitest", packageName });
}
replaySuite("node:test", { runner: "node-test" });
