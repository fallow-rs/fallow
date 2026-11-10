import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, before, describe, test } from "node:test";

import { installFallowMutation } from "../src/runtime/global.mjs";
import { RECORD_CALL_LIMIT, encodeArgs } from "../src/runtime/replay-value.mjs";
import { startCompanion } from "./support/companion-process.mjs";
import { VITEST_MAJORS, readMutationConstant } from "./support/vitest-fixture.mjs";

const PRELUDE = readMutationConstant("PRELUDE");
const RECORD_PRELUDE = readMutationConstant("RECORD_PRELUDE");
const PACKAGE_ROOT = fileURLToPath(new URL("..", import.meta.url));

test("a wrap records nothing and returns the same function when the runtime does not record", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  const add = (a, b) => a + b;
  assert.equal(runtime.w("src/a.js#add", add), add);
  control.startRecording(encodeArgs, 4);
  const wrapped = runtime.w("src/a.js#add", add);
  assert.notEqual(wrapped, add);
  assert.equal(wrapped.name, "add");
  assert.equal(wrapped.length, 2);
  assert.equal(wrapped(2, 3), 5);
  assert.equal(wrapped(2, 3), 5);
  wrapped(() => 1, 0);
  control.setActive("m1");
  wrapped(7, 7);
  control.setActive(null);
  assert.deepEqual(control.takeRecords(), [
    {
      key: "src/a.js#add",
      inputs: [[2, 3]],
      not_replayable: [{ reason: "a function", count: 1 }],
    },
  ]);
  assert.deepEqual(control.takeRecords(), []);
  control.stopRecording();
  assert.equal(runtime.w("src/a.js#add", add), add);
});

test("a wrap keeps this, new and a thrown error", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startRecording(encodeArgs, 4);
  function Point(x) {
    this.x = x;
  }
  const WrappedPoint = runtime.w("src/a.js#Point", Point);
  const point = new WrappedPoint(3);
  assert.equal(point.x, 3);
  assert.ok(point instanceof Point);
  const object = {
    base: 10,
    plus: runtime.w("src/a.js#plus", function plus(n) {
      return this.base + n;
    }),
  };
  assert.equal(object.plus(1), 11);
  const fail = runtime.w("src/a.js#fail", () => {
    throw new RangeError("no");
  });
  assert.throws(() => fail(1), RangeError);
  control.takeRecords();
  control.stopRecording();
});

test("the record limit keeps the first distinct inputs of a function", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startRecording(encodeArgs, 2);
  const id = runtime.w("src/a.js#id", (n) => n);
  for (const n of [1, 1, 2, 3, 4]) id(n);
  assert.deepEqual(control.takeRecords()[0].inputs, [[1], [2]]);
  control.stopRecording();
});

test("a function stops encoding after the call limit, also across tests", () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  let encoded = 0;
  const counting = (args) => {
    encoded += 1;
    return encodeArgs(args);
  };
  control.startRecording(counting, 8);
  const id = runtime.w("src/a.js#id", (n) => n);
  for (let call = 0; call < 500; call += 1) id(1);
  control.takeRecords();
  for (let call = 0; call < 500; call += 1) id(1);
  assert.equal(encoded, RECORD_CALL_LIMIT);
  control.stopRecording();
});

test("a large value is refused before the encoder copies it", () => {
  const started = performance.now();
  for (const value of [
    new Uint8Array(20_000_000),
    Array.from({ length: 1 }).concat(Array(30_000_000)),
    new Set(Array.from({ length: 20_000 }, (_, i) => i)),
  ]) {
    const encoded = encodeArgs([value]);
    assert.equal(encoded.ok, false);
    assert.equal(encoded.reason, "a value that is too large");
  }
  assert.ok(performance.now() - started < 1000, "the size check copies the value");
});

test("an accessor at an array index is refused and its getter does not run", () => {
  let reads = 0;
  const array = [];
  Object.defineProperty(array, 0, { get: () => (reads += 1), enumerable: true });
  const encoded = encodeArgs([array]);
  assert.equal(encoded.ok, false);
  assert.equal(encoded.reason, "an accessor property");
  assert.equal(reads, 0);
});

test("the size check reads no getter of a subclass of a built-in class", () => {
  let reads = 0;
  class Bytes extends Uint8Array {
    get length() {
      reads += 1;
      return 1;
    }
  }
  const encoded = encodeArgs([new Bytes(2)]);
  assert.equal(encoded.ok, false);
  assert.match(encoded.reason, /class instance/u);
  assert.equal(reads, 0);
});

test("the reasons are a closed set without user text", () => {
  const withKey = Object.defineProperty({}, "hidden_property_name", {
    get: () => 1,
    enumerable: true,
  });
  assert.equal(encodeArgs([withKey]).reason, "an accessor property");
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.startRecording(() => {
    throw new Error("Invalid token abc123");
  }, 8);
  runtime.w("src/a.js#f", () => 1)(1);
  assert.deepEqual(control.takeRecords()[0].not_replayable, [
    { reason: "a value that cannot be read", count: 1 },
  ]);
  control.stopRecording();
});

const ORIGINAL = `export function add(a: number, b: number): number {
  return a + b;
}

export const apply = (fn: (n: number) => number, n: number): number => fn(n);
`;

const m = (id, mutated, original) =>
  `(__fallow_m_a("${id}") ? (${mutated}) : (__fallow_m_c("${id}"), (${original})))`;

const INSTRUMENTED = `// @ts-nocheck
${PRELUDE}${RECORD_PRELUDE}
export function add(a: number, b: number): number {
  return ${m("k1", "a - b", "a + b")};
}
;add = __fallow_m_w("src/rec.ts#add", add);

export const apply = __fallow_m_w("src/rec.ts#apply", (fn: (n: number) => number, n: number): number => ${m("k2", "n", "fn(n)")});
`;

const NODE_TEST = `import { test } from "node:test";
import assert from "node:assert/strict";
import { add, apply } from "../src/rec.ts";
test("add", () => {
  assert.equal(add(2, 3), 5);
  assert.equal(add(2, 3), 5);
  assert.equal(add(-1, 1), 0);
});
test("apply", () => assert.equal(apply((n) => n + 1, 1), 2));
test("name", () => assert.equal(add.name, "add"));
test("source", () => assert.match(add.toString(), /a [+] b|a - b/));
`;

const VITEST_TEST = `import { expect, test } from "vitest";
import { add, apply } from "../src/rec";
test("add", () => {
  expect(add(2, 3)).toBe(5);
  expect(add(2, 3)).toBe(5);
  expect(add(-1, 1)).toBe(0);
});
test("apply", () => expect(apply((n) => n + 1, 1)).toBe(2));
test("name", () => expect(add.name).toBe("add"));
test("source", () => expect(add.toString()).toMatch(/a [+] b|a - b/));
`;

const createProject = ({ runner, packageName }) => {
  const workspace = mkdtempSync(join(tmpdir(), `fallow-record-${runner}-`));
  const root = join(workspace, "project");
  const files = {
    "project/package.json": '{"name":"p","type":"module"}\n',
    "project/src/rec.ts": ORIGINAL,
    "run/files/src/rec.ts": INSTRUMENTED,
    "project/test/rec.test.ts": runner === "vitest" ? VITEST_TEST : NODE_TEST,
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
    runDir: join(workspace, "run"),
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};

const recordSuite = (label, options) => {
  describe(`argument recording with ${label}`, () => {
    let project;
    let companion;
    let request;

    before(async () => {
      project = createProject(options);
      companion = startCompanion();
      let nextId = 1;
      request = (message) => companion.request({ v: 1, id: nextId++, ...message });
      const ready = await request({
        type: "init",
        root: project.root,
        runner: options.runner,
        config_path: null,
        run_dir: project.runDir,
        path_map: { "src/rec.ts": "files/src/rec.ts" },
        options: {},
      });
      assert.equal(ready.type, "ready", JSON.stringify(ready));
    });
    after(() => {
      companion?.stop();
      project?.cleanup();
    });

    test("a dry run never records and runs the functions without the wrap", async () => {
      const result = await request({ type: "dry_run" });
      assert.equal(result.type, "dry_run_result", JSON.stringify(result));
      assert.equal(result.recorded_args, undefined);
      assert.ok(
        result.tests.every((entry) => entry.status === "pass"),
        JSON.stringify(result),
      );
    });

    test("a record request gives the distinct inputs and the reasons of each function", async () => {
      const result = await request({ type: "record" });
      assert.equal(result.type, "record_result", JSON.stringify(result));
      const byKey = Object.fromEntries(result.recorded_args.map((entry) => [entry.key, entry]));
      assert.deepEqual(byKey["src/rec.ts#add"].inputs, [
        [2, 3],
        [-1, 1],
      ]);
      assert.deepEqual(byKey["src/rec.ts#add"].not_replayable, []);
      assert.deepEqual(byKey["src/rec.ts#apply"].inputs, []);
      assert.deepEqual(byKey["src/rec.ts#apply"].not_replayable, [
        { reason: "a function", count: 1 },
      ]);
    });

    test("a test that the wrap fails does not fail the record request", async () => {
      const result = await request({ type: "record" });
      assert.equal(result.type, "record_result");
      assert.ok(result.recorded_args.length > 0);
    });

    test("a dry run after a record request still runs unwrapped functions", async () => {
      const dry = await request({ type: "dry_run" });
      assert.ok(
        dry.tests.every((entry) => entry.status === "pass"),
        JSON.stringify(dry),
      );
    });
  });
};

for (const { packageName, version } of VITEST_MAJORS) {
  recordSuite(`vitest ${version}`, { runner: "vitest", packageName });
}
recordSuite("node:test", { runner: "node-test" });
