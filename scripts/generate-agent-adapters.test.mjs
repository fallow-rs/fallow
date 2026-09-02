import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
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
  const agents = join(root, ".agents", "agents");
  mkdirSync(agents, { recursive: true });
  writeFileSync(
    join(agents, "rust-reviewer.md"),
    "---\nname: rust-reviewer\ndescription: Reviews Rust changes.\ntools: Glob, Grep, Read, Bash\nmodel: sonnet\n---\n\nReview Rust code.\n",
  );
  writeFileSync(
    join(agents, "_template.md"),
    "---\nname: agent-name\ndescription: Template.\n---\n\nTemplate body.\n",
  );
  return root;
};

test("generates Claude adapters from canonical Agent Skills", () => {
  const repoRoot = createRepo();
  const drifted = generateAgentAdapters({ repoRoot });
  assert.deepEqual(drifted, [".claude/agents/rust-reviewer.md", ".claude/skills/review/SKILL.md"]);
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

test("generates Claude reviewer-agent adapters from canonical .agents/agents, skipping the template", () => {
  const repoRoot = createRepo();
  generateAgentAdapters({ repoRoot });
  const destination = join(repoRoot, ".claude", "agents", "rust-reviewer.md");
  const generated = readFileSync(destination, "utf8");
  assert.match(generated, /^---\nname: rust-reviewer\n/);
  assert.match(generated, /Generated from \.agents\/agents/);
  assert.match(generated, /Review Rust code\./);
  // The frontmatter closing `---` must precede the marker so Claude Code still parses it first.
  const frontmatterEnd = generated.indexOf("\n---\n", 4);
  const markerStart = generated.indexOf("<!-- Generated from .agents/agents");
  assert.ok(frontmatterEnd > 0 && markerStart > frontmatterEnd);
  assert.equal(existsSync(join(repoRoot, ".claude", "agents", "_template.md")), false);
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), []);
});

test("agent check mode reports drift without overwriting it, and removes orphaned generated agents", () => {
  const repoRoot = createRepo();
  generateAgentAdapters({ repoRoot });
  const target = join(repoRoot, ".claude", "agents", "rust-reviewer.md");
  writeFileSync(target, "manually edited\n");
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), [
    ".claude/agents/rust-reviewer.md",
  ]);
  assert.equal(readFileSync(target, "utf8"), "manually edited\n");

  generateAgentAdapters({ repoRoot });
  rmSync(join(repoRoot, ".agents", "agents", "rust-reviewer.md"));
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), [
    ".claude/agents/rust-reviewer.md",
  ]);
  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(target), false);
});

test("rejects agent filename and frontmatter name drift", () => {
  const repoRoot = createRepo();
  const target = join(repoRoot, ".agents", "agents", "rust-reviewer.md");
  writeFileSync(
    target,
    "---\nname: mcp-reviewer\ndescription: Reviews Rust changes.\ntools: Glob, Grep, Read, Bash\nmodel: sonnet\n---\n\nReview Rust code.\n",
  );
  assert.throws(
    () => generateAgentAdapters({ repoRoot }),
    /name mcp-reviewer does not match file rust-reviewer/,
  );
});
