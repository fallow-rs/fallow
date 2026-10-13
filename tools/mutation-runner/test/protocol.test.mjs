import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { startCompanion } from "./support/companion-process.mjs";

const withCompanion = (body) => async () => {
  const companion = startCompanion();
  try {
    await body(companion);
  } finally {
    companion.stop();
  }
};

test(
  "a line that is not JSON gives a bad-request error with a null id",
  withCompanion(async (companion) => {
    companion.sendRaw("{not json");
    assert.deepEqual(JSON.parse(await companion.nextLine()), {
      v: 1,
      id: null,
      type: "error",
      error: {
        code: "bad-request",
        message: "the request line is not valid JSON",
        retryable: false,
      },
    });
  }),
);

test(
  "a wrong wire version gives a protocol-version error that copies the id",
  withCompanion(async (companion) => {
    const response = await companion.request({ v: 2, id: 7, type: "reload" });
    assert.equal(response.id, 7);
    assert.equal(response.type, "error");
    assert.equal(response.error.code, "protocol-version");
    assert.equal(response.error.retryable, false);
  }),
);

test(
  "an unknown type and a run before init are bad requests",
  withCompanion(async (companion) => {
    const unknown = await companion.request({ v: 1, id: 1, type: "explode" });
    assert.deepEqual([unknown.id, unknown.error.code], [1, "bad-request"]);
    const early = await companion.request({
      v: 1,
      id: 2,
      type: "run",
      mutant_id: "m1",
      test_ids: ["a"],
      timeout_ms: 1000,
      bail: true,
    });
    assert.deepEqual([early.id, early.error.code], [2, "bad-request"]);
  }),
);

test(
  "a request without a numeric id is a bad request with a null id",
  withCompanion(async (companion) => {
    const response = await companion.request({ v: 1, id: "x", type: "reload" });
    assert.deepEqual([response.id, response.error.code], [null, "bad-request"]);
  }),
);

test(
  "responses keep the request order",
  withCompanion(async (companion) => {
    companion.send({ v: 1, id: 1, type: "explode" });
    companion.send({ v: 9, id: 2, type: "reload" });
    companion.send({ v: 1, id: 3, type: "dry_run", test_files: [] });
    const ids = [];
    for (let index = 0; index < 3; index += 1) {
      ids.push(JSON.parse(await companion.nextLine()).id);
    }
    assert.deepEqual(ids, [1, 2, 3]);
  }),
);

test(
  "init in a project without the runner gives runner-not-found",
  withCompanion(async (companion) => {
    const root = mkdtempSync(join(tmpdir(), "fallow-mutation-empty-"));
    const runDir = mkdtempSync(join(tmpdir(), "fallow-mutation-run-"));
    try {
      const response = await companion.request({
        v: 1,
        id: 1,
        type: "init",
        root,
        runner: "vitest",
        config_path: null,
        run_dir: runDir,
        path_map: {},
        options: { test_files: [], env: {} },
      });
      assert.equal(response.type, "error");
      assert.equal(response.error.code, "runner-not-found");
    } finally {
      rmSync(root, { recursive: true, force: true });
      rmSync(runDir, { recursive: true, force: true });
    }
  }),
);

test(
  "init with a runner name that has no adapter is a bad request",
  withCompanion(async (companion) => {
    const response = await companion.request({
      v: 1,
      id: 1,
      type: "init",
      root: tmpdir(),
      runner: "jest",
      config_path: null,
      run_dir: tmpdir(),
      path_map: {},
      options: {},
    });
    assert.deepEqual([response.id, response.error.code], [1, "bad-request"]);
  }),
);

test(
  "shutdown exits with code 0 and sends no response",
  withCompanion(async (companion) => {
    companion.send({ v: 1, id: 1, type: "shutdown" });
    assert.deepEqual(await companion.exited, { code: 0, signal: null });
    assert.deepEqual(companion.lines, []);
  }),
);

test(
  "the end of stdin exits with code 0",
  withCompanion(async (companion) => {
    companion.child.stdin.end();
    assert.deepEqual(await companion.exited, { code: 0, signal: null });
  }),
);

test("each protocol line starts with the session token of the companion", async () => {
  const companion = startCompanion({ token: "0123456789abcdefABCDEF" });
  try {
    companion.sendRaw("{not json");
    assert.equal(JSON.parse(await companion.nextLine()).error.code, "bad-request");
    assert.deepEqual(companion.strayLines, []);
  } finally {
    companion.stop();
  }
});

for (const [label, token] of [
  ["no session token", null],
  ["a session token with a space", "0123456789abcdef 0123"],
]) {
  test(`a companion with ${label} exits with code 2 and writes no protocol line`, async () => {
    const companion = startCompanion({ token });
    try {
      companion.send({ v: 1, id: 1, type: "reload" });
      assert.deepEqual(await companion.exited, { code: 2, signal: null });
      assert.deepEqual([companion.lines, companion.strayLines], [[], []]);
      assert.match(companion.stderr(), /FALLOW_MUTATION_SESSION_TOKEN/u);
    } finally {
      companion.stop();
    }
  });
}
