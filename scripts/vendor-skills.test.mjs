import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { decide, diffTrees, listFiles, main, runCheck, runVendor } from "./vendor-skills.mjs";

/** Materialize { "rel/path": "contents" } into a fresh temp dir; returns its path. */
const makeTree = (files) => {
  const dir = mkdtempSync(join(tmpdir(), "vendor-skills-"));
  for (const [rel, contents] of Object.entries(files)) {
    const dest = join(dir, rel);
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, contents);
  }
  return dir;
};

test("listFiles is recursive, sorted, and skips dotfiles", () => {
  const dir = makeTree({
    "SKILL.md": "a",
    "references/mcp.md": "b",
    "references/cli.md": "c",
    ".DS_Store": "junk",
    "references/.hidden": "junk",
  });
  assert.deepEqual(listFiles(dir), ["SKILL.md", "references/cli.md", "references/mcp.md"]);
  rmSync(dir, { recursive: true });
});

test("diffTrees reports missing, extra, and changed by relative path", () => {
  const canonical = makeTree({
    "SKILL.md": "same",
    "references/mcp.md": "canonical",
    "only-canonical.md": "x",
  });
  const vendored = makeTree({
    "SKILL.md": "same",
    "references/mcp.md": "vendored",
    "only-vendored.md": "y",
  });
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  assert.deepEqual(missing, ["only-canonical.md"]);
  assert.deepEqual(extra, ["only-vendored.md"]);
  assert.deepEqual(changed, ["references/mcp.md"]);
  rmSync(canonical, { recursive: true });
  rmSync(vendored, { recursive: true });
});

test("diffTrees is empty when the trees are byte-identical", () => {
  const canonical = makeTree({ "SKILL.md": "x", "references/mcp.md": "y" });
  const vendored = makeTree({ "SKILL.md": "x", "references/mcp.md": "y" });
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  assert.deepEqual([missing, extra, changed], [[], [], []]);
  rmSync(canonical, { recursive: true });
  rmSync(vendored, { recursive: true });
});

test('diffTrees ignores `"version"` string differences (staggered release bump)', () => {
  const canonical = makeTree({ "references/cli.md": 'before\n  "version": "3.2.0",\nafter' });
  const vendored = makeTree({ "references/cli.md": 'before\n  "version": "3.3.0",\nafter' });
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  assert.deepEqual([missing, extra, changed], [[], [], []]);
  rmSync(canonical, { recursive: true });
  rmSync(vendored, { recursive: true });
});

test("diffTrees still catches content drift alongside a version diff", () => {
  // The exact rot the gate exists to prevent: a plugin-count drift that also
  // happens to carry a version-string difference must NOT be masked.
  const canonical = makeTree({ "SKILL.md": '114 framework plugins\n  "version": "3.2.0"' });
  const vendored = makeTree({ "SKILL.md": '97 framework plugins\n  "version": "3.3.0"' });
  assert.deepEqual(diffTrees(canonical, vendored).changed, ["SKILL.md"]);
  rmSync(canonical, { recursive: true });
  rmSync(vendored, { recursive: true });
});

test("diffTrees treats a missing vendored tree as all-missing (not a crash)", () => {
  const canonical = makeTree({ "SKILL.md": "x" });
  const { missing, extra, changed } = diffTrees(canonical, join(canonical, "does-not-exist"));
  assert.deepEqual(missing, ["SKILL.md"]);
  assert.deepEqual([extra, changed], [[], []]);
  rmSync(canonical, { recursive: true });
});

test("runCheck returns 1 on drift and 0 when in sync", () => {
  const canonical = makeTree({ "SKILL.md": "x" });
  const drifted = makeTree({ "SKILL.md": "y" });
  const identical = makeTree({ "SKILL.md": "x" });
  assert.equal(runCheck(canonical, drifted, { renderDiffs: false }), 1);
  assert.equal(runCheck(canonical, identical, { renderDiffs: false }), 0);
  for (const d of [canonical, drifted, identical]) {
    rmSync(d, { recursive: true });
  }
});

test("runVendor mirrors canonical into vendored: adds, updates, removes extras", () => {
  const canonical = makeTree({ "SKILL.md": "new", "references/mcp.md": "added" });
  const vendored = makeTree({ "SKILL.md": "old", "stale.md": "remove-me" });
  assert.equal(runVendor(canonical, vendored), 0);
  // vendored is now byte-identical to canonical
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  assert.deepEqual([missing, extra, changed], [[], [], []]);
  assert.equal(readFileSync(join(vendored, "SKILL.md"), "utf8"), "new");
  assert.equal(readFileSync(join(vendored, "references/mcp.md"), "utf8"), "added");
  assert.equal(existsSync(join(vendored, "stale.md")), false);
  rmSync(canonical, { recursive: true });
  rmSync(vendored, { recursive: true });
});

test("decide dispatches skip / error / check / vendor by resolved state", () => {
  assert.equal(decide({ present: false, explicit: false, check: true }).action, "skip");
  assert.equal(decide({ present: false, explicit: true, check: true }).action, "error");
  assert.equal(decide({ present: false, explicit: false, check: false }).action, "error");
  assert.equal(decide({ present: true, explicit: false, check: true }).action, "check");
  assert.equal(decide({ present: true, explicit: false, check: false }).action, "vendor");
});

test("main throws when FALLOW_SKILLS_DIR is set but missing (both modes)", () => {
  const prev = process.env.FALLOW_SKILLS_DIR;
  process.env.FALLOW_SKILLS_DIR = join(tmpdir(), "vendor-skills-nonexistent-canonical");
  try {
    assert.throws(() => main(["--check"]), /FALLOW_SKILLS_DIR is set/);
    assert.throws(() => main([]), /not found/);
  } finally {
    if (prev === undefined) {
      delete process.env.FALLOW_SKILLS_DIR;
    } else {
      process.env.FALLOW_SKILLS_DIR = prev;
    }
  }
});
