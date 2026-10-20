// Tests for dependabot-repair-guard.mjs, the patch check in the publish job of
// dependabot-repair.yml.
//
// Run: node --test .github/scripts/dependabot-repair-guard.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  addedUrls,
  checkRepair,
  MAX_CHANGED_LINES,
  parseNumstat,
  parseRawModes,
} from "./dependabot-repair-guard.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./dependabot-repair-guard.mjs", import.meta.url));
const file = (path, added = 1, deleted = 1, mode = "100644") => ({ path, added, deleted, mode });

test("one commit with a test and a lockfile change passes and needs review", () => {
  const files = [file("scripts/workflow-policy.test.mjs"), file("package-lock.json", 900, 900)];
  assert.deepEqual(checkRepair({ commits: 1, files }), { ok: true, problems: [], merge: "review" });
});

test("more than one commit is rejected", () => {
  assert.deepEqual(checkRepair({ commits: 2, files: [file("src/a.ts")] }).problems, [
    "the patch has 2 commits, not 1",
  ]);
});

test("a change under .github or to the changelog is rejected", () => {
  for (const path of [
    ".github/workflows/ci.yml",
    ".github/scripts/dependabot-triage.mjs",
    "CHANGELOG.md",
    "changelog.d/fix.md",
  ]) {
    const result = checkRepair({ commits: 1, files: [file(path)] });
    assert.equal(result.ok, false, path);
    assert.match(result.problems[0], /may not change/u, path);
  }
});

test("a large hand-written change is rejected, while generated files do not count", () => {
  const big = MAX_CHANGED_LINES;
  assert.equal(checkRepair({ commits: 1, files: [file("crates/a/src/lib.rs", big, 1)] }).ok, false);
  const generated = [
    file("Cargo.lock", big, big),
    file("editors/vscode/pnpm-lock.yaml", big, big),
    file("crates/cli/viz-assets/viz.js", big, big),
    file("tests/coverage-producer-corpus/manifest.json", big, big),
  ];
  assert.equal(checkRepair({ commits: 1, files: generated }).ok, true);
});

test("a path with a control character is rejected and does not break the parse", () => {
  const records = ["1\t0\tsrc/a.ts", "2\t0\tdocs/x\n.github/workflows/ci.yml", "-\t-\tlogo.png"];
  const files = parseNumstat(`${records.join("\0")}\0`);
  assert.deepEqual(
    files.map(({ path }) => path),
    ["src/a.ts", "docs/x\n.github/workflows/ci.yml", "logo.png"],
  );
  assert.match(checkRepair({ commits: 1, files: [files[1]] }).problems[0], /control character/u);
});

test("a binary file is rejected", () => {
  const files = [file("assets/logo.png", null, null)];
  assert.match(checkRepair({ commits: 1, files }).problems[0], /binary/u);
});

test("a symbolic link or a submodule is rejected", () => {
  const link = checkRepair({ commits: 1, files: [file("docs/link", 1, 0, "120000")] });
  assert.match(link.problems[0], /symbolic link/u);
  const module = checkRepair({ commits: 1, files: [file("vendor/x", 1, 0, "160000")] });
  assert.match(module.problems[0], /submodule/u);
});

test("a change to code that a build runs is accepted but needs review", () => {
  for (const path of [
    "crates/mcp/src/server.rs",
    "crates/cli/build.rs",
    "package.json",
    "hk.pkl",
    "scripts/workflow-policy.test.mjs",
  ]) {
    const result = checkRepair({ commits: 1, files: [file(path), file("Cargo.lock")] });
    assert.equal(result.ok, true, path);
    assert.equal(result.merge, "review", path);
  }
  const generated = [file("crates/cli/tests/snapshots/a.snap"), file("Cargo.lock")];
  assert.equal(checkRepair({ commits: 1, files: generated }).merge, "auto");
});

test("a lockfile URL outside the official registry needs review", () => {
  const files = [file("package-lock.json")];
  const npm = "https://registry.npmjs.org/acorn/-/acorn-8.19.0.tgz";
  const crates = "registry+https://github.com/rust-lang/crates.io-index";
  assert.equal(checkRepair({ commits: 1, files, lockfileUrls: [npm, crates] }).merge, "auto");
  for (const url of [
    "https://evil.example/acorn.tgz",
    "git+https://github.com/x/y",
    "http://registry.npmjs.org/a",
  ]) {
    assert.equal(checkRepair({ commits: 1, files, lockfileUrls: [url] }).merge, "review", url);
  }
});

test("only added diff lines give URLs", () => {
  const diff = [
    "+++ b/package-lock.json",
    '-      "resolved": "https://registry.npmjs.org/old.tgz",',
    '+      "resolved": "https://registry.npmjs.org/new.tgz",',
    '+source = "registry+https://github.com/rust-lang/crates.io-index"',
  ].join("\n");
  assert.deepEqual(addedUrls(diff), [
    "https://registry.npmjs.org/new.tgz",
    "registry+https://github.com/rust-lang/crates.io-index",
  ]);
});

test("the raw diff gives the new mode of each path", () => {
  const raw = [":100644 120000 aaa bbb T", "docs/link", ":000000 100644 000 ccc A", "a b.txt", ""];
  assert.deepEqual(
    [...parseRawModes(raw.join("\0"))],
    [
      ["docs/link", "120000"],
      ["a b.txt", "100644"],
    ],
  );
});

test("the script started as a process checks the patch in a git repository", () => {
  const dir = mkdtempSync(join(tmpdir(), "repair-guard-"));
  const git = (...args) => {
    const run = spawnSync("git", args, { cwd: dir, encoding: "utf8" });
    assert.equal(run.status, 0, run.stderr);
    return run.stdout.trim();
  };
  git("init", "--quiet");
  git("config", "user.email", "test@example.com");
  git("config", "user.name", "Test");
  git("config", "commit.gpgsign", "false");
  writeFileSync(join(dir, "a.txt"), "one\n");
  git("add", ".");
  git("commit", "--quiet", "-m", "base");
  const before = git("rev-parse", "HEAD");
  writeFileSync(join(dir, "CHANGELOG.md"), "entry\n");
  git("add", ".");
  git("commit", "--quiet", "-m", "repair");

  const run = spawnSync(process.execPath, [SCRIPT_PATH], {
    cwd: dir,
    encoding: "utf8",
    env: { PATH: process.env.PATH, BEFORE: before },
  });
  assert.equal(run.status, 1);
  assert.match(run.stdout, /CHANGELOG\.md may not change/u);
});
