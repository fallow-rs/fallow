import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  decide,
  diffTrees,
  listFiles,
  main,
  runCheck,
  runVendor,
  stripUnsupportedMetadata,
} from "./vendor-skills.mjs";

const makeTree = (files) => {
  const dir = mkdtempSync(join(tmpdir(), "vendor-skills-"));
  for (const [path, content] of Object.entries(files)) {
    const destination = join(dir, path);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
  }
  return dir;
};

test("listFiles is recursive, sorted, and skips dotfiles", () => {
  const dir = makeTree({
    "SKILL.md": "a",
    "references/mcp.md": "b",
    "references/cli.md": "c",
    ".DS_Store": "junk",
  });
  assert.deepEqual(listFiles(dir), ["SKILL.md", "references/cli.md", "references/mcp.md"]);
  rmSync(dir, { recursive: true });
});

test("listFiles rejects symlinks", () => {
  const dir = makeTree({ "SKILL.md": "a" });
  symlinkSync(join(dir, "SKILL.md"), join(dir, "linked.md"));
  assert.throws(() => listFiles(dir), /symlinks/u);
  rmSync(dir, { recursive: true });
});

test("metadata transform preserves the rest of SKILL.md", () => {
  const source = [
    "---",
    "name: fallow",
    "metadata:",
    "  version: 1.0.0",
    "license: MIT",
    "---",
    "# Fallow",
  ].join("\n");
  assert.equal(
    stripUnsupportedMetadata(source),
    ["---", "name: fallow", "license: MIT", "---", "# Fallow"].join("\n"),
  );
});

test("diffTrees ignores host adapters but reports contract drift", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n# Fallow\n",
    "references/mcp.md": "canonical",
  });
  const published = makeTree({
    "SKILL.md": "---\nname: fallow\n---\n# Fallow\n",
    "references/mcp.md": "published",
    "agents/openai.yaml": "interface: {}",
  });
  assert.deepEqual(diffTrees(canonical, published), {
    missing: [],
    extra: [],
    changed: ["references/mcp.md"],
  });
  rmSync(canonical, { recursive: true });
  rmSync(published, { recursive: true });
});

test("runCheck returns success only for the transformed exact contract", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n",
  });
  const matching = makeTree({ "SKILL.md": "---\nname: fallow\n---\n" });
  const drifted = makeTree({ "SKILL.md": "---\nname: other\n---\n" });
  assert.equal(runCheck(canonical, matching, { renderDiffs: false }), 0);
  assert.equal(runCheck(canonical, drifted, { renderDiffs: false }), 1);
  for (const dir of [canonical, matching, drifted]) {
    rmSync(dir, { recursive: true });
  }
});

test("runVendor updates contract files without deleting host adapters", () => {
  const canonical = makeTree({
    "SKILL.md": "---\nname: fallow\nmetadata:\n  version: 1.0.0\n---\n",
    "references/mcp.md": "new",
  });
  const published = makeTree({
    "SKILL.md": "old",
    "references/stale.md": "remove",
    "agents/openai.yaml": "interface: {}",
  });
  assert.equal(runVendor(canonical, published), 0);
  assert.deepEqual(diffTrees(canonical, published), {
    missing: [],
    extra: [],
    changed: [],
  });
  assert.equal(existsSync(join(published, "agents/openai.yaml")), true);
  assert.equal(readFileSync(join(published, "references/mcp.md"), "utf8"), "new");
  assert.equal(existsSync(join(published, "references/stale.md")), false);
  rmSync(canonical, { recursive: true });
  rmSync(published, { recursive: true });
});

test("missing companion checkout always fails closed", () => {
  assert.equal(decide({ present: false, check: true }).action, "error");
  assert.equal(decide({ present: false, check: false }).action, "error");
  assert.equal(decide({ present: true, check: true }).action, "check");
  assert.equal(decide({ present: true, check: false }).action, "vendor");
});

test("main throws when the explicit public consumer is missing", () => {
  const previous = process.env.FALLOW_SKILLS_DIR;
  process.env.FALLOW_SKILLS_DIR = join(tmpdir(), "vendor-skills-missing-consumer");
  try {
    assert.throws(() => main(["--check"]), /not found/u);
    assert.throws(() => main([]), /not found/u);
  } finally {
    if (previous === undefined) {
      delete process.env.FALLOW_SKILLS_DIR;
    } else {
      process.env.FALLOW_SKILLS_DIR = previous;
    }
  }
});
