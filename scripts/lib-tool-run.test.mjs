import assert from "node:assert/strict";
import { join } from "node:path";
import { test } from "node:test";

import { makeTempDir } from "./lib/temp-dir.mjs";
import { classifyRun, parseJsonReport, resolveFallowBin, runTool } from "./lib/tool-run.mjs";

const ENOENT_ERROR = { code: "ENOENT", message: "spawn x ENOENT" };

test("classifyRun reports a binary that does not start", () => {
  const run = classifyRun({ status: null, signal: null, error: ENOENT_ERROR });
  assert.equal(run.ok, false);
  assert.equal(run.stdout, "");
  assert.equal(run.stderr, "");
  assert.equal(run.reason, "failed to start: spawn x ENOENT");
});

test("classifyRun reports a timeout", () => {
  const run = classifyRun({
    status: null,
    signal: "SIGTERM",
    stdout: "",
    error: { code: "ETIMEDOUT" },
  });
  assert.equal(run.timedOut, true);
  assert.equal(run.spawnError, null);
  assert.equal(run.reason, "timed out");
});

test("classifyRun accepts only the listed exit codes", () => {
  const proc = { status: 1, signal: null, stdout: "{}", stderr: "" };
  const strict = classifyRun(proc);
  assert.equal(strict.ok, false);
  assert.equal(strict.reason, "exited with status 1");
  const lenient = classifyRun(proc, { okStatuses: [0, 1] });
  assert.equal(lenient.ok, true);
  assert.equal(lenient.reason, null);
});

test("classifyRun turns buffer output into a string", () => {
  const run = classifyRun({ status: 0, signal: null, stdout: Buffer.from("x") });
  assert.equal(run.stdout, "x");
});

test("runTool reports the exit status of a real child", () => {
  const run = runTool(process.execPath, ["-e", "process.exit(3)"]);
  assert.equal(run.ok, false);
  assert.equal(run.status, 3);
  assert.equal(run.reason, "exited with status 3");
});

test("runTool reports a missing executable", () => {
  const dir = makeTempDir("fallow-lib-tool-run-");
  const run = runTool(join(dir, "missing-binary"), []);
  assert.equal(run.ok, false);
  assert.notEqual(run.spawnError, null);
});

test("parseJsonReport rejects empty and non-object output", () => {
  assert.deepEqual(parseJsonReport(""), { ok: false, error: "no JSON output" });
  const withBom = parseJsonReport(`${String.fromCodePoint(0xfeff)}{"a":1}`);
  assert.equal(withBom.ok, true);
  assert.equal(withBom.value.a, 1);
  assert.deepEqual(parseJsonReport("3"), { ok: false, error: "unexpected JSON shape" });
  assert.equal(parseJsonReport("{").ok, false);
});

test("resolveFallowBin returns the first candidate that runs", () => {
  const dir = makeTempDir("fallow-lib-tool-run-");
  assert.throws(() => resolveFallowBin([join(dir, "missing")]), /fallow binary not found/);
  assert.equal(resolveFallowBin(["", process.execPath]), process.execPath);
});

test("runTool names output that exceeds maxBuffer", () => {
  const run = runTool(process.execPath, ["-e", "process.stdout.write('x'.repeat(4096))"], {
    maxBuffer: 16,
  });
  assert.equal(run.ok, false);
  assert.match(run.reason, /^output exceeded maxBuffer/);
});
