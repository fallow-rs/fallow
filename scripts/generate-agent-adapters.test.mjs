import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { tmpdir } from "node:os";
import { mkdtempSync } from "node:fs";

import { generateAgentAdapters } from "./generate-agent-adapters.mjs";

const createRepo = () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-agent-adapters-"));
  const skill = join(root, ".agents", "skills", "review");
  mkdirSync(skill, { recursive: true });
  writeFileSync(
    join(skill, "SKILL.md"),
    "---\nname: review\ndescription: Review a Fallow change.\n---\n\n# Review\n",
  );
  return root;
};

test("generates Claude adapters from canonical Agent Skills", () => {
  const repoRoot = createRepo();
  assert.deepEqual(generateAgentAdapters({ repoRoot }), [".claude/skills/review/SKILL.md"]);
  const generated = readFileSync(join(repoRoot, ".claude", "skills", "review", "SKILL.md"), "utf8");
  assert.match(generated, /Generated from \.agents\/skills/);
  assert.match(generated, /# Review/);
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), []);
});

test("check mode reports drift without overwriting it", () => {
  const repoRoot = createRepo();
  generateAgentAdapters({ repoRoot });
  const target = join(repoRoot, ".claude", "skills", "review", "SKILL.md");
  writeFileSync(target, "manually edited\n");
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), [
    ".claude/skills/review/SKILL.md",
  ]);
  assert.equal(readFileSync(target, "utf8"), "manually edited\n");
});

test("rejects directory and frontmatter name drift", () => {
  const repoRoot = createRepo();
  const target = join(repoRoot, ".agents", "skills", "review", "SKILL.md");
  writeFileSync(target, "---\nname: ship\ndescription: Review a Fallow change.\n---\n\n# Review\n");
  assert.throws(
    () => generateAgentAdapters({ repoRoot }),
    /name ship does not match directory review/,
  );
});
