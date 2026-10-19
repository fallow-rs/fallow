import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
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

test("rejects unclassified shared review rules", () => {
  const fixture = setup();
  write(fixture.root, ".agents/rules/rust.md", "# Rust\n\nRead `crates/missing/src/lib.rs`.\n");
  fixture.visibleFiles.push(".agents/rules/rust.md");

  const errors = validateKnowledgeArchitecture(fixture);
  assert.ok(
    errors.some((error) => error.includes("host rule is not classified: .agents/rules/rust.md")),
  );
});

for (const [client, root] of [
  ["claude", ".claude/rules"],
  ["shared", ".agents/rules"],
]) {
  test(`checks ${client} rules for stale source paths`, () => {
    const fixture = setup();
    const changed = structuredClone(manifest);
    changed.hostRules.push({ client, root, mode: "scoped-router" });
    write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
    write(fixture.root, `${root}/rust.md`, "# Rust\n\nRead `crates/missing/src/lib.rs`.\n");
    fixture.visibleFiles.push(`${root}/rust.md`);

    const errors = validateKnowledgeArchitecture(fixture);
    assert.ok(
      errors.some(
        (error) => error.includes(`${root}/rust.md`) && error.includes("missing repository path"),
      ),
    );
  });
}

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

test("the script starts as CI runs it and checks the repository", () => {
  const repoRoot = resolve(import.meta.dirname, "..");
  const result = spawnSync(process.execPath, ["scripts/check-knowledge-architecture.mjs"], {
    cwd: repoRoot,
    encoding: "utf8",
    env: { PATH: process.env.PATH ?? "" },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "Knowledge architecture is consistent.\n");
});

test("reports every manifest contract error in a stable order", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.schemaVersion = 1;
  changed.trustBoundary.automatedPrivateToPublic = true;
  changed.entryPoints[0].routes.push("/absolute/route", "../outside");
  changed.maintainerDocs.documents.push(
    { path: "docs/README.md", section: "entry-point" },
    { path: "docs/phantom.md", section: "development" },
    { path: "docs/development/unindexed.md", section: "development" },
  );
  changed.maintainerDocs.rootDocuments = [
    { path: "CHANGELOG.md", section: "release-history" },
    { path: "CHANGELOG.md", section: "release-history" },
    { path: "MISSING.md", section: "project" },
  ];
  changed.hostRules.push(
    { client: "broken", root: "", mode: "scoped-router" },
    { root: ".agents/rules", mode: "flat" },
    { client: "empty", root: ".empty/rules", mode: "scoped-router" },
  );
  changed.generatedPathContracts = [
    { path: "../generated", setupDocument: "/absolute/setup.md" },
    { path: "generated", setupDocument: "docs/development/guide.md", setupCommand: [] },
  ];
  changed.externalSources = [
    {
      repository: "example/broken",
      role: "public-contract",
      direction: "private-to-public",
      revisionSource: "lock.json#commit",
      allowedRoot: "public",
      processor: { kind: "generator" },
      checkCommand: ["node", "scripts/check.mjs"],
    },
    {
      role: "",
      direction: "public-to-consumer",
      revisionSource: "lock.json#commit",
      allowedRoot: "public",
      processor: { kind: "none" },
      checkCommand: [""],
    },
  ];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
  write(fixture.root, "CHANGELOG.md", "# Changelog\n\nSee `/Users/example/history`.\n");
  write(fixture.root, "docs/development/unindexed.md", "# Unindexed\n");
  write(fixture.root, "docs/superpowers/plan.md", "# Plan\n");
  write(fixture.root, "editors/AGENTS.md", "# Editors\n\nSee `/Users/example/editor`.\n");
  write(fixture.root, ".agents/skills/extra/SKILL.md", "---\nname: extra\n---\n# Extra\n");
  write(fixture.root, ".claude/skills/review/SKILL.md", "---\nname: review\n---\n# Review\n");
  fixture.visibleFiles.push(
    "CHANGELOG.md",
    "docs/development/unindexed.md",
    "docs/superpowers/plan.md",
    "editors/AGENTS.md",
    ".agents/skills/extra/SKILL.md",
  );

  assert.deepEqual(validateKnowledgeArchitecture(fixture), [
    "unsupported knowledge manifest schema: 1",
    "trust boundary must forbid automated private-to-public publication",
    "AGENTS.md has a non-repository route: /absolute/route",
    "AGENTS.md has a non-repository route: ../outside",
    "knowledge manifest contains duplicate maintainer document paths",
    "classified maintainer document is not tracked: docs/phantom.md",
    "knowledge manifest contains duplicate root document paths",
    "classified root document is not tracked: MISSING.md",
    "root document is missing: MISSING.md",
    "maintainer document is missing: docs/phantom.md",
    "docs/README.md does not index docs/phantom.md",
    "docs/README.md does not index docs/development/unindexed.md",
    "docs/superpowers/ must remain ignored, but Git can see docs/superpowers/plan.md",
    "editors/AGENTS.md references non-portable knowledge: /Users/",
    "invalid broken host rule contract",
    "invalid <unknown> host rule contract",
    "empty host rule tree is empty: .empty/rules",
    "claude generated adapters do not match canonical skills: canonical=[extra, review], adapters=[review]",
    ".claude/skills/review/SKILL.md is missing the generated adapter marker",
    "generated path contract is unsafe: ../generated",
    "generated path setup document is unsafe: /absolute/setup.md",
    "generated path contract has no executable setupCommand: generated",
    "external source example/broken has invalid direction: private-to-public",
    "external source example/broken is missing processor entrypoint",
    "external source <unknown> is missing repository",
    "external source <unknown> is missing role",
    "external source <unknown> has no executable checkCommand",
  ]);
});

test("reports an unclassified docs index", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.maintainerDocs.index = "docs/INDEX.md";
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);

  assert.deepEqual(validateKnowledgeArchitecture(fixture), [
    "maintainer docs index is not classified: docs/INDEX.md",
  ]);
});

test("requires host rules, canonical skills, and external sources", () => {
  const fixture = setup();
  const changed = structuredClone(manifest);
  changed.hostRules = [];
  changed.externalSources = [];
  write(fixture.root, "scripts/knowledge-surfaces.json", `${JSON.stringify(changed, null, 2)}\n`);
  const visibleFiles = fixture.visibleFiles.filter(
    (path) => !path.endsWith("/SKILL.md") && !path.startsWith(".claude/rules/"),
  );

  assert.deepEqual(validateKnowledgeArchitecture({ root: fixture.root, visibleFiles }), [
    "knowledge manifest must classify host rule trees",
    "canonical skill tree is empty: .agents/skills",
    "knowledge manifest must declare external source contracts",
  ]);
});
