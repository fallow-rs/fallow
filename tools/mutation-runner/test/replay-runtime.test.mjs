import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";
import test from "node:test";

import { installFallowMutation } from "../src/runtime/global.mjs";
import { decodeValue, encodeArgs, encodeResult } from "../src/runtime/replay-value.mjs";
import { installDeterminism, runCall } from "../src/runtime/replay.mjs";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("each replay call reads its own active mutant across awaits", async () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.enableContext(new AsyncLocalStorage());
  const read = async (ms) => {
    await pause(ms);
    return runtime.a("m1");
  };
  // The mutant call awaits longer than the original call starts and ends, so
  // a process-wide active mutant would give both calls the same answer.
  const [mutant, original] = await Promise.all([
    control.runWithActive("m1", () => read(20)),
    control.runWithActive(null, () => read(1)),
  ]);
  assert.equal(mutant, true);
  assert.equal(original, false);
  // Outside a replay call, the process-wide value applies again.
  control.setActive("m1");
  assert.equal(runtime.a("m1"), true);
  control.setActive(null);
});

test("a timer of the mutant call that fires later still sees the mutant", async () => {
  const control = installFallowMutation();
  const runtime = globalThis.__fallowMutation;
  control.enableContext(new AsyncLocalStorage());
  let late = null;
  control.runWithActive("m2", () => {
    setTimeout(() => {
      late = runtime.a("m2");
    }, 5);
  });
  await control.runWithActive(null, () => pause(20));
  assert.equal(late, true);
});

test("the replay clock and random source are fixed and restored", () => {
  const realNow = Date.now;
  const realRandom = Math.random;
  const first = installDeterminism({ clockMs: 1_767_225_600_000, seed: 7 });
  const values = [Date.now(), new Date().getTime(), Math.random(), Math.random()];
  first.restore();
  const second = installDeterminism({ clockMs: 1_767_225_600_000, seed: 7 });
  const again = [Date.now(), new Date().getTime(), Math.random(), Math.random()];
  second.restore();
  assert.deepEqual(again, values);
  // Each read moves the clock by 1 ms, so a busy wait on the clock ends.
  assert.equal(values[0], 1_767_225_600_000);
  assert.equal(values[1], 1_767_225_600_001);
  assert.notEqual(values[2], values[3]);
  assert.ok(values[2] >= 0 && values[2] < 1);
  assert.equal(Date.now, realNow);
  assert.equal(Math.random, realRandom);
  assert.equal(new Date(0).getTime(), 0);
});

test("arguments round-trip through the replay encoding", () => {
  const args = [
    1,
    -0,
    Number.NaN,
    Number.POSITIVE_INFINITY,
    undefined,
    null,
    12n,
    "text",
    [1, [2, undefined]],
    { a: 1, nested: { b: "x" } },
    new Date(0),
    /a+b/giu,
    new Map([["k", 1]]),
    new Set([1, 2]),
    new Uint8Array([1, 2, 3]),
    { $replay: "a key that looks like a tag" },
  ];
  const encoded = encodeArgs(args);
  assert.equal(encoded.ok, true, JSON.stringify(encoded));
  const decoded = decodeValue(JSON.parse(JSON.stringify(encoded.value)));
  assert.equal(Object.is(decoded[1], -0), true);
  assert.equal(Number.isNaN(decoded[2]), true);
  assert.equal(decoded[3], Number.POSITIVE_INFINITY);
  assert.equal(decoded[4], undefined);
  assert.equal(decoded[6], 12n);
  assert.deepEqual(decoded[8], [1, [2, undefined]]);
  assert.deepEqual(decoded[9], { a: 1, nested: { b: "x" } });
  assert.equal(decoded[10].getTime(), 0);
  assert.equal(String(decoded[11]), "/a+b/giu");
  assert.deepEqual([...decoded[12]], [["k", 1]]);
  assert.deepEqual([...decoded[13]], [1, 2]);
  assert.deepEqual([...decoded[14]], [1, 2, 3]);
  assert.ok(decoded[14] instanceof Uint8Array);
  assert.deepEqual(decoded[15], { $replay: "a key that looks like a tag" });
});

test("values that cannot be replayed name the reason", () => {
  class Point {
    constructor() {
      this.x = 1;
    }
  }
  const cyclic = {};
  cyclic.self = cyclic;
  const cases = [
    [() => 1, "function"],
    [new Point(), "class instance"],
    [Symbol("s"), "symbol"],
    [new Proxy({}, {}), "proxy"],
    [cyclic, "cyclic"],
    [Promise.resolve(1), "class instance"],
    [Object.defineProperty({}, "x", { get: () => 1, enumerable: true }), "accessor"],
  ];
  for (const [value, reason] of cases) {
    const encoded = encodeArgs([value]);
    assert.equal(encoded.ok, false, `${String(reason)} must not replay`);
    assert.match(encoded.reason, new RegExp(reason, "u"));
  }
});

test("a result encodes each value, also one that cannot be an argument", () => {
  class Point {
    constructor() {
      this.x = 1;
    }
  }
  const error = new TypeError("bad");
  assert.deepEqual(encodeResult(new Point()), {
    $replay: "instance",
    class: "Point",
    fields: { x: 1 },
  });
  assert.deepEqual(encodeResult(error), { $replay: "error", name: "TypeError", message: "bad" });
  assert.deepEqual(
    encodeResult(() => 1),
    { $replay: "function", name: "" },
  );
  const cyclic = { a: 1 };
  cyclic.self = cyclic;
  assert.deepEqual(encodeResult(cyclic), { a: 1, self: { $replay: "cycle" } });
});

test("typed arrays keep NaN, Infinity and -0", () => {
  const encoded = encodeArgs([new Float64Array([Number.NaN, -0, Number.POSITIVE_INFINITY])]);
  assert.equal(encoded.ok, true, JSON.stringify(encoded));
  const [decoded] = decodeValue(JSON.parse(JSON.stringify(encoded.value)));
  assert.equal(Number.isNaN(decoded[0]), true);
  assert.equal(Object.is(decoded[1], -0), true);
  assert.equal(decoded[2], Number.POSITIVE_INFINITY);
  assert.notDeepEqual(
    JSON.stringify(encodeResult(new Float64Array([Number.NaN]))),
    JSON.stringify(encodeResult(new Float64Array([Number.POSITIVE_INFINITY]))),
  );
});

test("a result keeps an own __proto__ key", () => {
  assert.notDeepEqual(encodeResult(JSON.parse('{"__proto__":{"x":1}}')), encodeResult({}));
});

test("a subclass of a built-in class is not replayable as an argument", () => {
  class Registry extends Map {}
  for (const value of [new Registry(), Buffer.from("a")]) {
    const encoded = encodeArgs([value]);
    assert.equal(encoded.ok, false, value.constructor.name);
    assert.match(encoded.reason, /class instance/u);
  }
});

test("a value whose text cannot be read still returns", async () => {
  const control = installFallowMutation();
  control.enableContext(new AsyncLocalStorage());
  const odd = {
    [Symbol.for("nodejs.util.inspect.custom")]: () => {
      throw new Error("no text");
    },
  };
  const record = await runCall(
    { export: "f", args: [], mutant_id: "m1", call_timeout_ms: 1000, clock_ms: 0, seed: 1 },
    "original",
    { control, load: async () => ({ f: () => odd }) },
  );
  assert.equal(record.outcome, "returned");
  const badError = new Error("x");
  Object.defineProperty(badError, "name", {
    get: () => {
      throw new Error("no name");
    },
  });
  const thrown = await runCall(
    { export: "f", args: [], mutant_id: "m1", call_timeout_ms: 1000, clock_ms: 0, seed: 1 },
    "original",
    {
      control,
      load: async () => ({
        f: () => {
          throw badError;
        },
      }),
    },
  );
  assert.equal(thrown.outcome, "threw");
});
