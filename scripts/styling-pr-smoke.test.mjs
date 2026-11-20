import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { makeTempDir } from "./lib/temp-dir.mjs";
import { classifyRun } from "./lib/tool-run.mjs";
import { remoteBranchRefspec, summarizeAudit } from "./styling-pr-smoke.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const SCRIPT_PATH = resolve(SCRIPT_DIR, "styling-pr-smoke.mjs");

const runScript = (args) =>
  spawnSync(process.execPath, [SCRIPT_PATH, ...args], {
    encoding: "utf8",
    timeout: 10_000,
  });

test("styling PR smoke exposes help without network access", () => {
  const result = runScript(["--help"]);

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Usage: node scripts\/styling-pr-smoke\.mjs/);
  assert.match(result.stdout, /--run-only/);
  assert.equal(result.stderr, "");
});

test("styling PR smoke lists candidate repos as JSON", () => {
  const result = runScript(["--list"]);

  assert.equal(result.status, 0, result.stderr);
  const candidates = JSON.parse(result.stdout);
  assert.ok(candidates.some((candidate) => candidate.repo === "AWeber-Imbi/imbi-ui"));
  assert.ok(candidates.every((candidate) => candidate.repo.includes("/")));
});

test("styling PR smoke rejects invalid numeric options before network work", () => {
  const result = runScript(["--select-only", "--max-prs", "0"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--max-prs must be a positive integer/);
});

test("styling PR smoke rejects mutually exclusive modes", () => {
  const result = runScript(["--run-only", "--select-only"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--run-only and --select-only are mutually exclusive/);
});

test("styling PR smoke fetches base refs into origin remote tracking branches", () => {
  assert.equal(remoteBranchRefspec("main"), "refs/heads/main:refs/remotes/origin/main");
  assert.equal(remoteBranchRefspec("master"), "refs/heads/master:refs/remotes/origin/master");
});

test("styling PR smoke rejects a fractional --prs-per-repo", () => {
  const result = runScript(["--list", "--prs-per-repo", "1.5"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--prs-per-repo must be a positive integer/);
});

test("styling PR smoke rejects a fractional --clone-depth", () => {
  const result = runScript(["--list", "--clone-depth", "1.5"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--clone-depth must be a positive integer/);
});

test("styling PR smoke rejects an inherited object key as an argument", () => {
  const result = runScript(["constructor", "x", "--list"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unknown argument: constructor/);
});

test("styling PR smoke stops before the audit when the fallow binary is missing", () => {
  const dir = makeTempDir("fallow-styling-pr-smoke-");
  const result = runScript([
    "--run-only",
    "--out-dir",
    dir,
    "--fallow-bin",
    join(dir, "missing-fallow"),
  ]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /fallow binary not found/);
});

const AUDIT_STATUSES = { okStatuses: [0, 1] };

test("summarizeAudit rejects an audit binary that did not start", () => {
  const run = classifyRun(
    { status: null, error: { code: "ENOENT", message: "spawn fallow ENOENT" } },
    AUDIT_STATUSES,
  );
  const audit = summarizeAudit(run);

  assert.equal(audit.usable, false);
  assert.match(audit.reason, /failed to start/);
});

test("summarizeAudit rejects empty audit output", () => {
  const run = classifyRun({ status: 0, signal: null, stdout: "", stderr: "" }, AUDIT_STATUSES);
  const audit = summarizeAudit(run);

  assert.equal(audit.usable, false);
  assert.equal(audit.reason, "no JSON output");
});

test("summarizeAudit rejects JSON that is not an audit report", () => {
  const run = classifyRun({ status: 0, signal: null, stdout: "{}", stderr: "" }, AUDIT_STATUSES);
  const audit = summarizeAudit(run);

  assert.equal(audit.usable, false);
  assert.equal(audit.reason, "no audit report in the output");
});

test("summarizeAudit accepts an audit report with a failed verdict", () => {
  const run = classifyRun(
    { status: 1, signal: null, stdout: '{"kind":"audit","verdict":"fail"}', stderr: "" },
    AUDIT_STATUSES,
  );
  const audit = summarizeAudit(run);

  assert.equal(audit.usable, true);
  assert.equal(audit.verdict, "fail");
  assert.equal(audit.styling_findings, 0);
});

test("summarizeAudit keeps the message of a fallow error report", () => {
  const stdout = '{"error":true,"message":"could not determine changed files","exit_code":2}';
  const run = classifyRun({ status: 2, signal: null, stdout, stderr: "" }, AUDIT_STATUSES);
  const audit = summarizeAudit(run);

  assert.equal(audit.usable, false);
  assert.equal(audit.reason, "could not determine changed files");
});
