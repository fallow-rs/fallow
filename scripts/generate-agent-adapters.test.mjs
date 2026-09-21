import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { tmpdir } from "node:os";
import { mkdtempSync } from "node:fs";

import { generateAgentAdapters } from "./generate-agent-adapters.mjs";

const createRepo = (root = mkdtempSync(join(tmpdir(), "fallow-agent-adapters-"))) => {
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

/**
 * `core.excludesFile` is neutralized so the machine's global ignore rules cannot
 * decide what the test repository tracks, and `-f` stages paths a repository
 * ignore rule would otherwise refuse.
 */
const git = (cwd, args) => {
  const result = spawnSync("git", ["-c", "core.excludesFile=/dev/null", ...args], {
    cwd,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, `git ${args.join(" ")}: ${result.stderr}`);
};

/** A repository whose canonical sources are tracked and whose adapters are not. */
const createTrackedRepo = () => {
  const root = createRepo();
  git(root, ["init", "--quiet"]);
  git(root, ["add", "-f", "--", ".agents"]);
  return root;
};

const removeCanonicalSources = (repoRoot) => {
  rmSync(join(repoRoot, ".agents", "skills", "review"), { recursive: true });
  rmSync(join(repoRoot, ".agents", "agents", "rust-reviewer.md"));
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

test("leaves untracked generated adapters alone and names each one", () => {
  const repoRoot = createTrackedRepo();
  generateAgentAdapters({ repoRoot });
  removeCanonicalSources(repoRoot);

  const skipped = [];
  assert.deepEqual(
    generateAgentAdapters({ check: true, onSkip: (path) => skipped.push(path), repoRoot }),
    [],
  );
  assert.deepEqual(skipped.toSorted(), [
    ".claude/agents/rust-reviewer.md",
    ".claude/skills/review/SKILL.md",
  ]);

  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(join(repoRoot, ".claude", "skills", "review", "SKILL.md")), true);
  assert.equal(existsSync(join(repoRoot, ".claude", "agents", "rust-reviewer.md")), true);
});

test("reports and removes a tracked adapter whose canonical source is gone", () => {
  const repoRoot = createTrackedRepo();
  generateAgentAdapters({ repoRoot });
  git(repoRoot, ["add", "-f", "--", ".claude"]);
  removeCanonicalSources(repoRoot);

  const skipped = [];
  assert.deepEqual(
    generateAgentAdapters({ check: true, onSkip: (path) => skipped.push(path), repoRoot }),
    [".claude/agents/rust-reviewer.md", ".claude/skills/review/SKILL.md"],
  );
  assert.deepEqual(skipped, []);

  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(join(repoRoot, ".claude", "skills", "review")), false);
  assert.equal(existsSync(join(repoRoot, ".claude", "agents", "rust-reviewer.md")), false);
});

test("a checkout nested in another repository judges ownership by its own paths", () => {
  // Git reports paths relative to the top of the repository it answers for, which
  // here is the outer one. Ownership is looked up by a path relative to the
  // checkout, so a checkout that is not itself the repository top would read as
  // entirely untracked: nothing stale, and nothing removed.
  const outer = mkdtempSync(join(tmpdir(), "fallow-agent-adapters-nested-"));
  git(outer, ["init", "--quiet"]);
  const repoRoot = createRepo(join(outer, "inner"));
  generateAgentAdapters({ repoRoot });
  git(outer, ["add", "-f", "--", "."]);
  removeCanonicalSources(repoRoot);

  const skipped = [];
  assert.deepEqual(
    generateAgentAdapters({ check: true, onSkip: (path) => skipped.push(path), repoRoot }),
    [".claude/agents/rust-reviewer.md", ".claude/skills/review/SKILL.md"],
  );
  assert.deepEqual(skipped, []);

  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(join(repoRoot, ".claude", "skills", "review")), false);
  assert.equal(existsSync(join(repoRoot, ".claude", "agents", "rust-reviewer.md")), false);
});

test("companion orphan ownership follows tracked content too", () => {
  const repoRoot = createTrackedRepo();
  generateAgentAdapters({ repoRoot });
  const orphan = join(repoRoot, ".claude", "skills", "review", "references", "local.md");
  mkdirSync(dirname(orphan), { recursive: true });
  writeFileSync(orphan, "local note\n");

  const skipped = [];
  assert.deepEqual(
    generateAgentAdapters({ check: true, onSkip: (path) => skipped.push(path), repoRoot }),
    [],
  );
  assert.deepEqual(skipped, [".claude/skills/review/references/local.md"]);
  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(orphan), true);

  git(repoRoot, ["add", "-f", "--", ".claude"]);
  assert.deepEqual(generateAgentAdapters({ check: true, repoRoot }), [
    ".claude/skills/review/references/local.md",
  ]);
  generateAgentAdapters({ repoRoot });
  assert.equal(existsSync(orphan), false);
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
