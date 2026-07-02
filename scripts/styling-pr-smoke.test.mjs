import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { remoteBranchRefspec } from "./styling-pr-smoke.mjs";

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
  assert.match(result.stderr, /--max-prs must be a positive number/);
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
