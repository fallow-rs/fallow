import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { makeTempDir } from "./lib/temp-dir.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "public-config-corpus.py");
const FIXTURES = join(REPO_ROOT, "scripts", "fixtures", "public-config-corpus");
const RUNBOOK = join(REPO_ROOT, "docs", "public-config-corpus.md");

// Runs the offline fixture pass from the repository root, as the runbook does.
const runCorpus = (extraArgs) => {
  const dir = makeTempDir("fallow-public-config-corpus-");
  const cacheDir = join(dir, "cache");
  cpSync(join(FIXTURES, "cache"), cacheDir, { recursive: true });
  const result = spawnSync(
    "python3",
    [
      SCRIPT,
      "--from-search-fixture",
      join(FIXTURES, "search.json"),
      "--cache-dir",
      cacheDir,
      "--fetched-at",
      "2026-05-27T00:00:00Z",
      "--gh-version",
      "gh version fixture",
      ...extraArgs,
    ],
    { cwd: REPO_ROOT, encoding: "utf8", timeout: 60_000 },
  );
  return { result, cacheDir };
};

test("the corpus report goes to the cache folder by default and leaves the runbook alone", () => {
  const runbook = readFileSync(RUNBOOK, "utf8");
  const { result, cacheDir } = runCorpus([]);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(RUNBOOK, "utf8"), runbook);
  assert.match(readFileSync(join(cacheDir, "report.md"), "utf8"), /Fetched configs: 3/);
});

test("the corpus script refuses to write its report over the maintained runbook", () => {
  const runbook = readFileSync(RUNBOOK, "utf8");
  const { result, cacheDir } = runCorpus(["--output", "docs/public-config-corpus.md"]);

  assert.equal(result.status, 2);
  assert.match(result.stderr, /maintained runbook/);
  assert.equal(readFileSync(RUNBOOK, "utf8"), runbook);
  assert.equal(existsSync(join(cacheDir, "manifest.json")), false);
});
