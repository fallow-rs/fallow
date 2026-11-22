import assert from "node:assert/strict";
import { test } from "node:test";

import { fallowEnv, parseCompact } from "../benchmarks/compare-dupes-rolling.mjs";

// Lines in the shape that `fallow dupes --format compact` prints.
const COMPACT_REPORT = [
  "code-duplication:src/a.ts:2-6:fingerprint=dupe:0123456789abcdef,group=1,tokens=48,lines=5,instances=2",
  "code-duplication:src/b.ts:10-14:fingerprint=dupe:0123456789abcdef,group=1,tokens=48,lines=5,instances=2,symlink=true",
  "",
].join("\n");

test("parseCompact reads the file, the range and the tokens of each clone instance", () => {
  const lines = parseCompact(COMPACT_REPORT);

  assert.deepEqual(
    lines.map(({ file, start, end, tokens }) => ({ file, start, end, tokens })),
    [
      { file: "src/a.ts", start: 2, end: 6, tokens: 48 },
      { file: "src/b.ts", start: 10, end: 14, tokens: 48 },
    ],
  );
});

test("parseCompact gives one key per instance that ignores the group number", () => {
  const renumbered = COMPACT_REPORT.replaceAll("group=1", "group=7");

  assert.deepEqual(
    parseCompact(renumbered).map((line) => line.key),
    parseCompact(COMPACT_REPORT).map((line) => line.key),
  );
});

test("parseCompact gives no instances for an empty report", () => {
  assert.deepEqual(parseCompact("\n"), []);
});

test("parseCompact rejects a line that is not a duplication line", () => {
  assert.throws(() => parseCompact("unused-file:src/a.ts\n"), /unexpected compact line/);
});

test("fallowEnv removes the rolling detector switch for the reference run", () => {
  const env = fallowEnv(false, { FALLOW_DUPES_ROLLING: "1", HOME: "/home/user" });

  assert.equal(Object.hasOwn(env, "FALLOW_DUPES_ROLLING"), false);
  assert.equal(env.HOME, "/home/user");
});

test("fallowEnv sets the rolling detector switch for the rolling run", () => {
  assert.equal(fallowEnv(true, { HOME: "/home/user" }).FALLOW_DUPES_ROLLING, "1");
});
