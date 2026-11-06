import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { isTrackedEntry, newEntries } from "./check-temp-leaks.mjs";
import { makeTempDir } from "./lib/temp-dir.mjs";

const SCRIPT = fileURLToPath(new URL("./check-temp-leaks.mjs", import.meta.url));

// Each run gets its own temp directory, so a parallel test file that writes
// to the system temp directory cannot change the result.
const runCheck = (command) => {
  const tempDir = makeTempDir("fallow-temp-leaks-test-");
  const env = { ...process.env, TMPDIR: tempDir, TMP: tempDir, TEMP: tempDir };
  const result = spawnSync(process.execPath, [SCRIPT, "--", ...command], {
    encoding: "utf8",
    env,
  });
  return { ...result, tempDir };
};

const nodeEval = (source) => [process.execPath, "-e", source];

test("tracks fallow entries and skips the designed caches", () => {
  assert.equal(isTrackedEntry("fallow-knowledge-abc123"), true);
  assert.equal(isTrackedEntry("fallow-audit-base-cache-abc123"), false);
  assert.equal(isTrackedEntry("fallow-mt-abc123"), false);
  assert.equal(isTrackedEntry("vitest-abc123"), false);
});

test("lists only the entries that the command added", () => {
  const before = new Set(["fallow-a", "fallow-b"]);
  const after = new Set(["fallow-b", "fallow-d", "fallow-c"]);
  assert.deepEqual(newEntries(before, after), ["fallow-c", "fallow-d"]);
});

test("fails and names each folder that the command leaves", () => {
  const leak =
    'require("node:fs").mkdtempSync(require("node:path").join(require("node:os").tmpdir(), "fallow-leak-"))';
  const result = runCheck(nodeEval(leak));
  assert.equal(result.status, 1, result.stderr);
  const [leaked] = readdirSync(result.tempDir);
  assert.match(leaked, /^fallow-leak-/);
  assert.match(result.stderr, new RegExp(leaked));
});

test("passes when the command removes its folders or leaves only ignored ones", () => {
  const source = [
    'const { mkdtempSync, rmSync } = require("node:fs");',
    'const { join } = require("node:path");',
    'const { tmpdir } = require("node:os");',
    'rmSync(mkdtempSync(join(tmpdir(), "fallow-clean-")), { recursive: true });',
    'mkdtempSync(join(tmpdir(), "fallow-mt-"));',
  ].join("\n");
  const result = runCheck(nodeEval(source));
  assert.equal(result.status, 0, result.stderr);
});

test("keeps the exit code of a failing command", () => {
  const result = runCheck(nodeEval("process.exit(3)"));
  assert.equal(result.status, 3);
});

test("rejects a call without a command", () => {
  const result = runCheck([]);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /usage:/);
});
