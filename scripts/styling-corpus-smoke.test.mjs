import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { makeTempDir } from "./lib/temp-dir.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const SCRIPT_PATH = resolve(SCRIPT_DIR, "styling-corpus-smoke.mjs");

const runScript = (args) =>
  spawnSync(process.execPath, [SCRIPT_PATH, ...args], {
    encoding: "utf8",
    timeout: 10_000,
  });

const outputLines = (stdout) => stdout.split("\n").filter(Boolean);

test("styling corpus smoke rejects a --max-projects value that is not a number", () => {
  const result = runScript(["--list", "--max-projects", "abc"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--max-projects must be a positive integer/);
  assert.equal(result.stdout, "");
});

test("styling corpus smoke rejects a fractional --max-projects", () => {
  const result = runScript(["--list", "--max-projects", "0.5"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--max-projects must be a positive integer/);
  assert.equal(result.stdout, "");
});

test("styling corpus smoke rejects an inherited object key as an argument", () => {
  const result = runScript(["--list", "toString"]);

  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unknown argument: toString/);
});

test("styling corpus smoke lists the whole corpus without a cap", () => {
  const result = runScript(["--list"]);

  assert.equal(result.status, 0, result.stderr);
  assert.ok(outputLines(result.stdout).length > 1);
});

test("styling corpus smoke applies --max-projects to the list", () => {
  const result = runScript(["--list", "--max-projects", "1"]);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(outputLines(result.stdout).length, 1);
});

test("styling corpus smoke selects each repeated --project", () => {
  const result = runScript(["--list", "--project", "tailwindcss", "--project", "stylex"]);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(outputLines(result.stdout).length, 2);
});

// A fallow binary that answers --version. For an analysis command it prints
// `{}` and exits with `healthStatus` for `health` and 2 for other commands.
const fakeFallow = (dir, healthStatus) => {
  const path = join(dir, "fallow");
  writeFileSync(
    path,
    `#!${process.execPath}
const command = process.argv[2];
if (command === "--version") {
  console.log("fallow 0.0.0");
} else {
  process.stdout.write("{}");
  process.exitCode = command === "health" ? ${healthStatus} : 2;
}
`,
  );
  chmodSync(path, 0o755);
  return path;
};

const runCachedProject = (dir, fallowBin) => {
  const cacheDir = join(dir, "cache");
  mkdirSync(join(cacheDir, "tailwindcss", ".git"), { recursive: true });
  return runScript([
    "--skip-clone",
    "--project",
    "tailwindcss",
    "--cache-dir",
    cacheDir,
    "--out-dir",
    join(dir, "out"),
    "--fallow-bin",
    fallowBin,
  ]);
};

test("styling corpus smoke fails when every fallow command of every project fails", () => {
  const dir = makeTempDir("fallow-styling-corpus-smoke-");
  const result = runCachedProject(dir, fakeFallow(dir, 2));

  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /no project gave a usable fallow result/);
});

test("styling corpus smoke passes when one fallow command of a project works", () => {
  const dir = makeTempDir("fallow-styling-corpus-smoke-");
  const result = runCachedProject(dir, fakeFallow(dir, 1));

  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.stderr, /no project gave a usable fallow result/);
});
