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
  schemaVersion: 2,
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
    rootDocuments: [],
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
  hostRules: [{ client: "claude", root: ".claude/rules", mode: "scoped-router" }],
  externalSources: [
    {
      repository: "example/public-contract",
      role: "public-contract",
      direction: "public-to-consumer",
      revisionSource: "lock.json#commit",
      allowedRoot: "public",
      processor: { kind: "validator", entrypoint: "scripts/check.mjs" },
      checkCommand: ["node", "scripts/check.mjs"],
    },
  ],
  trustBoundary: {
    automatedPrivateToPublic: false,
    publicPromotion: "sanitized-reviewed-rewrite",
    forbiddenRoutePrefixes: [".internal/", "internal/", "decisions/"],
    forbiddenReferencePrefixes: [".internal/", "internal/", "decisions/"],
    forbiddenReferencePatterns: [
      { pattern: "/Users/", reason: "machine-local path" },
      { pattern: "~/.claude/", reason: "host-local state" },
    ],
    forbiddenRepositories: ["fallow-rs/fallow-cloud"],
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
  write(root, ".claude/rules/router.md", "# Router\n\nRead `docs/development/guide.md`.\n");
  const visibleFiles = [
    "scripts/knowledge-surfaces.json",
    "scripts/generate-agent-adapters.mjs",
    "AGENTS.md",
    "CLAUDE.md",
    "docs/README.md",
    "docs/development/guide.md",
    ".agents/skills/review/SKILL.md",
    ".claude/skills/review/SKILL.md",
    ".claude/rules/router.md",
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

test("reports unclassified root documents", () => {
  const fixture = setup();
  write(fixture.root, "CONTRIBUTING.md", "# Contributing\n");
  fixture.visibleFiles.push("CONTRIBUTING.md");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("root document is not classified")));
});

test("rejects missing local links and private-boundary routes", () => {
  const fixture = setup();
  write(
    fixture.root,
    "docs/development/guide.md",
    "# Guide\n\n[Missing](missing.md)\n\nSee `decisions/private.md` and `internal/runbook.md`.\n",
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

test("rejects scheme-independent private repository references", () => {
  const fixture = setup();
  write(
    fixture.root,
    "docs/development/guide.md",
    "# Guide\n\nSee git://github.com/fallow-rs/fallow-cloud.git and //github.com/fallow-rs/fallow-cloud.\n",
  );

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("references private repository")));
});

test("rejects local links that escape the repository root", () => {
  const fixture = setup();
  write(fixture.root, "docs/development/guide.md", "# Guide\n\n[Outside](../../../outside.md)\n");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("links outside the repository root")));
});

test("rejects local links to untracked files", () => {
  const fixture = setup();
  write(fixture.root, "docs/untracked.md", "# Untracked\n");
  write(fixture.root, "docs/development/guide.md", "# Guide\n\n[Untracked](../untracked.md)\n");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("links to untracked local path")));
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

test("rejects machine-local knowledge and stale implementation paths", () => {
  const fixture = setup();
  write(
    fixture.root,
    "docs/development/guide.md",
    "# Guide\n\nSee `/Users/example/repo` and `crates/missing/src/lib.rs`.\n",
  );

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("non-portable knowledge")));
  assert.ok(errors.some((error) => error.includes("missing repository path")));
});

test("rejects untracked repository paths in maintainer docs", () => {
  const fixture = setup();
  write(fixture.root, "scripts/untracked.mjs", "export {};\n");
  write(fixture.root, "docs/development/guide.md", "# Guide\n\nRun `scripts/untracked.mjs`.\n");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("untracked repository path")));
});

test("checks canonical skills and generated adapters for private references", () => {
  const fixture = setup();
  write(
    fixture.root,
    ".agents/skills/review/SKILL.md",
    "---\nname: review\n---\n# Review\n\nRead internal/runbook.md.\n",
  );
  write(
    fixture.root,
    ".claude/skills/review/SKILL.md",
    "---\nname: review\n---\n<!-- Generated from .agents/skills. Do not edit. -->\n# Review\n\nRead `/Users/example/private.md`.\n",
  );

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(
    errors.some(
      (error) =>
        error.includes(".agents/skills/review/SKILL.md") && error.includes("private path prefix"),
    ),
  );
  assert.ok(
    errors.some(
      (error) =>
        error.includes(".claude/skills/review/SKILL.md") &&
        error.includes("non-portable knowledge"),
    ),
  );
});

test("checks classified host rules for stale source paths", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.hostRules = [{ client: "claude", root: ".claude/rules", mode: "scoped-router" }];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
  write(fixture.root, ".claude/rules/rust.md", "# Rust\n\nRead `crates/missing/src/lib.rs`.\n");
  fixture.visibleFiles.push(".claude/rules/rust.md");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("missing repository path")));
});

test("requires executable external source contracts", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.externalSources = [
    {
      repository: "example/docs",
      role: "public-docs",
      direction: "public-to-consumer",
    },
  ];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("is missing revisionSource")));
  assert.ok(errors.some((error) => error.includes("invalid processor")));
  assert.ok(errors.some((error) => error.includes("no executable checkCommand")));
});

test("rejects unsafe external roots and shell prose", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.externalSources[0].allowedRoot = "../private";
  changed.externalSources[0].processor.entrypoint = "/tmp/check.mjs";
  changed.externalSources[0].checkCommand = ["node", "scripts/check.mjs", "&&", "echo"];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("unsafe allowedRoot")));
  assert.ok(errors.some((error) => error.includes("unsafe processor entrypoint")));
  assert.ok(errors.some((error) => error.includes("shell control")));
});

test("rejects Windows traversal in external paths", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.externalSources[0].allowedRoot = "..\\private";
  changed.externalSources[0].processor.entrypoint = "scripts\\check.mjs";
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("unsafe allowedRoot")));
  assert.ok(errors.some((error) => error.includes("unsafe processor entrypoint")));
});

test("does not accept entry-point routes mentioned only in comments", () => {
  const fixture = setup();
  write(fixture.root, "AGENTS.md", "# Codex\n\n<!-- docs/README.md -->\n\n`.agents/skills`\n");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("does not route to docs/README.md")));
});

test("requires generated path setup docs to contain the path and command", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.generatedPathContracts = [
    {
      path: "benchmarks/fixtures/real-world",
      setupDocument: "docs/development/guide.md",
      setupCommand: ["npm", "run", "missing-setup"],
    },
  ];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(errors.some((error) => error.includes("does not document generated path command")));
  assert.ok(errors.some((error) => error.includes("does not document generated path: benchmarks")));
});
