import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { validateKnowledgeArchitecture } from "./check-knowledge-architecture.mjs";

const write = (root, path, content) => {
  mkdirSync(join(root, path, ".."), { recursive: true });
  writeFileSync(join(root, path), content);
};

const manifest = {
  schemaVersion: 1,
  entryPoints: [
    {
      path: "AGENTS.md",
      client: "codex",
      routes: ["docs/README.md", ".agents/skills"],
    },
    {
      path: "CLAUDE.md",
      client: "claude",
      routes: ["docs/README.md", ".claude/skills"],
    },
  ],
  maintainerDocs: {
    index: "docs/README.md",
    documents: [
      { path: "docs/README.md", section: "entry-point" },
      { path: "docs/development/guide.md", section: "development" },
    ],
    ignoredWorktreePrefixes: [{ path: "docs/superpowers/", reason: "local workflow artifacts" }],
  },
  skills: {
    canonicalRoot: ".agents/skills",
    adapterRoots: [{ client: "claude", path: ".claude/skills", mode: "generated" }],
    generator: "scripts/generate-agent-adapters.mjs",
    checkCommand: "npm run check:agent-adapters",
  },
  trustBoundary: {
    automatedPrivateToPublic: false,
    publicPromotion: "sanitized-reviewed-rewrite",
    forbiddenRoutePrefixes: [".internal/", "internal/", "decisions/"],
    forbiddenReferencePrefixes: [".internal/", "decisions/"],
  },
};

const setup = () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-knowledge-"));
  write(root, "scripts/knowledge-surfaces.json", `${JSON.stringify(manifest, null, 2)}\n`);
  write(root, "scripts/generate-agent-adapters.mjs", "export {};\n");
  write(root, "AGENTS.md", "# Codex\n\n[Docs](docs/README.md)\n\n`.agents/skills`\n");
  write(root, "CLAUDE.md", "# Claude\n\n[Docs](docs/README.md)\n\n`.claude/skills`\n");
  write(root, "docs/README.md", "# Docs\n\n[Guide](development/guide.md)\n");
  write(root, "docs/development/guide.md", "# Guide\n");
  write(root, ".agents/skills/review/SKILL.md", "---\nname: review\n---\n# Review\n");
  write(
    root,
    ".claude/skills/review/SKILL.md",
    "---\nname: review\n---\n<!-- Generated from .agents/skills. Do not edit. -->\n# Review\n",
  );
  const visibleFiles = [
    "scripts/knowledge-surfaces.json",
    "scripts/generate-agent-adapters.mjs",
    "AGENTS.md",
    "CLAUDE.md",
    "docs/README.md",
    "docs/development/guide.md",
    ".agents/skills/review/SKILL.md",
    ".claude/skills/review/SKILL.md",
  ];
  return { root, visibleFiles };
};

test("accepts indexed durable docs and matching generated adapters", () => {
  const fixture = setup();
  assert.deepEqual(validateKnowledgeArchitecture(fixture), []);
});

test("does not let an untracked document satisfy the manifest", () => {
  const fixture = setup();
  execFileSync("git", ["-C", fixture.root, "init", "--quiet"]);
  execFileSync("git", ["-C", fixture.root, "add", "."]);
  const changed = structuredClone(manifest);
  changed.maintainerDocs.documents.push({
    path: "docs/untracked.md",
    section: "development",
  });
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
  write(
    fixture.root,
    "docs/README.md",
    "# Docs\n\n[Guide](development/guide.md)\n\n[Untracked](untracked.md)\n",
  );
  write(fixture.root, "docs/untracked.md", "# Untracked\n");

  const errors = validateKnowledgeArchitecture({ root: fixture.root });
  assert.ok(errors.some((error) => error.includes("not tracked: docs/untracked.md")));
});

test("reports unclassified docs and missing entry-point routes", () => {
  const fixture = setup();
  write(fixture.root, "docs/orphan.md", "# Orphan\n");
  write(fixture.root, "AGENTS.md", "# Codex\n");
  fixture.visibleFiles.push("docs/orphan.md");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("not classified: docs/orphan.md")));
  assert.ok(errors.some((error) => error.includes("does not route to docs/README.md")));
  assert.ok(errors.some((error) => error.includes("does not route to .agents/skills")));
});

test("rejects missing local links and private-boundary routes", () => {
  const fixture = setup();
  write(
    fixture.root,
    "docs/development/guide.md",
    "# Guide\n\n[Missing](missing.md)\n\nSee `decisions/private.md`.\n",
  );
  const changed = structuredClone(manifest);
  changed.entryPoints[0].routes.push("internal/runbook.md");
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
  write(
    fixture.root,
    "AGENTS.md",
    "# Codex\n\n[Docs](docs/README.md)\n\n`.agents/skills`\n\n`internal/runbook.md`\n",
  );

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("missing local path")));
  assert.ok(errors.some((error) => error.includes("routes across the private boundary")));
  assert.ok(errors.some((error) => error.includes("references private path prefix")));
});

test("rejects symlinked canonical knowledge", () => {
  const fixture = setup();
  const external = join(fixture.root, "external.md");
  writeFileSync(external, "# External\n");
  const changedVisible = fixture.visibleFiles.map((path) =>
    path === ".agents/skills/review/SKILL.md" ? ".agents/skills/review-link/SKILL.md" : path,
  );
  mkdirSync(join(fixture.root, ".agents/skills/review-link"), { recursive: true });
  symlinkSync(external, join(fixture.root, ".agents/skills/review-link/SKILL.md"));
  mkdirSync(join(fixture.root, ".claude/skills/review-link"), { recursive: true });
  write(
    fixture.root,
    ".claude/skills/review-link/SKILL.md",
    "<!-- Generated from .agents/skills. Do not edit. -->\n",
  );
  const errors = validateKnowledgeArchitecture({
    root: fixture.root,
    visibleFiles: changedVisible,
  });
  assert.ok(errors.some((error) => error.includes("must not be a symlink")));
});
