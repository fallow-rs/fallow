import assert from "node:assert/strict";
import { test } from "node:test";

import {
  escapeCell,
  firstSentence,
  hasSection,
  parseExistingTable,
  regenerateSkillMd,
  sectionIsAbsent,
  spliceSection,
} from "./generate-agent-docs.mjs";

const SCHEMA = {
  version: "0.0.0-test",
  manifest_version: "1",
  default_behavior: "Runs all analyses (check + dupes + health). Use --only/--skip to select.",
  commands: [
    {
      name: "dead-code",
      description: "Analyze project for unused code. Second sentence is cut.",
      flags: [{ name: "--unused-files" }, { name: "--unused-exports" }],
    },
    {
      name: "coverage",
      description: "Runtime coverage workflow",
      flags: [],
    },
  ],
  issue_types: [
    {
      id: "unused-file",
      command: "dead-code",
      description: "File is not reachable from any entry point",
      filter_flag: "--unused-files",
      fixable: false,
      suppressible: true,
      suppress_comment: "// fallow-ignore-file unused-file",
      note: null,
      license: "free",
    },
    {
      id: "type-only-dependency",
      command: "dead-code",
      description: "Dependency only used via import type",
      filter_flag: "--unused-deps",
      fixable: false,
      suppressible: false,
      suppress_comment: null,
      note: "Only reported in --production mode",
      license: "free",
    },
    {
      id: "sql-injection",
      command: "security",
      description: "Catalogue security candidate for CWE-89",
      filter_flag: null,
      fixable: false,
      suppressible: true,
      suppress_comment: "// fallow-ignore-next-line security-sink",
      note: null,
      license: "free",
    },
    {
      id: "tainted-sink",
      command: "security",
      description: "Syntactic security sink candidates require verification",
      filter_flag: null,
      fixable: false,
      suppressible: true,
      suppress_comment: "// fallow-ignore-next-line security-sink",
      note: null,
      license: "free",
    },
    {
      id: "runtime-safe-to-delete",
      command: "health",
      description: "Statically unused AND never invoked in production",
      filter_flag: null,
      fixable: false,
      suppressible: false,
      suppress_comment: null,
      note: "Requires --runtime-coverage input",
      license: "freemium",
    },
  ],
  mcp_tools: {
    tools: [
      {
        name: "analyze",
        kind: "analysis",
        license: "free",
        key_params: ["issue_types", "production"],
        description: "Full dead-code analysis",
      },
      {
        name: "list_boundaries",
        kind: "introspection",
        license: "free",
        key_params: [],
        description: "List architecture | boundary zones\nand access rules",
      },
    ],
  },
  task_matrix: [
    {
      task: "delete an unused export or file",
      command: "fallow dead-code --trace <file>:<export>",
      note: null,
    },
    {
      task: "scope a monorepo",
      command: "--workspace <glob> / --changed-workspaces <ref>",
      note: "global flags, prefix any command",
    },
  ],
};

const DOC = `# Skill

Hand-written intro stays.

## Commands

<!-- generated:commands:start -->
| Command | Purpose | Key Flags |
|---|---|---|
| \`fallow\` | Curated combined purpose | \`--only\`, \`--skip\` |
| \`dead-code\` | Curated dead code purpose | \`--changed-since\` |
| \`coverage\` | Coverage helper | \`setup\` |
| \`coverage upload-source-maps\` | Upload source maps from CI | \`--dir dist\` |
| \`removed-command\` | Should disappear | \`--gone\` |
<!-- generated:commands:end -->

## Issue Types

<!-- generated:issue-types:start -->
| Type | Filter flag | Fixable | Suppress comment | Description |
|---|---|---|---|---|
| \`unused-file\` | \`--unused-files\` | - | \`// fallow-ignore-file unused-file\` | Curated teaching prose for unused files |
<!-- generated:issue-types:end -->

## MCP Tools

<!-- generated:mcp-tools:start -->
| Tool | Kind | License | Key params | Description |
|---|---|---|---|---|
| \`analyze\` | analysis | free | \`issue_types\` | Curated long analyze prose with call hints |
<!-- generated:mcp-tools:end -->

## Task Map

<!-- generated:task-matrix:start -->
| When the agent is about to... | Run |
|---|---|
| stale row that should be regenerated | \`fallow gone\` |
<!-- generated:task-matrix:end -->

Hand-written outro stays.
`;

/** A target that has NOT adopted the task-matrix markers. The generator must
 * regenerate the other three sections and leave this file otherwise intact. */
const DOC_WITHOUT_TASK_MATRIX = `# Skill

Hand-written intro stays.

## Commands

<!-- generated:commands:start -->
| Command | Purpose | Key Flags |
|---|---|---|
| \`fallow\` | Curated combined purpose | \`--only\`, \`--skip\` |
| \`dead-code\` | Curated dead code purpose | \`--changed-since\` |
<!-- generated:commands:end -->

## Issue Types

<!-- generated:issue-types:start -->
| Type | Filter flag | Fixable | Suppress comment | Description |
|---|---|---|---|---|
| \`unused-file\` | \`--unused-files\` | - | \`// fallow-ignore-file unused-file\` | Curated teaching prose for unused files |
<!-- generated:issue-types:end -->

## MCP Tools

<!-- generated:mcp-tools:start -->
| Tool | Kind | License | Key params | Description |
|---|---|---|---|---|
| \`analyze\` | analysis | free | \`issue_types\` | Curated long analyze prose with call hints |
<!-- generated:mcp-tools:end -->

Hand-written outro stays.
`;

test("escapeCell escapes pipes and collapses whitespace, leaves backticks and angle brackets", () => {
  assert.equal(escapeCell("a | b\nc  d `e` <f>"), "a \\| b c d `e` <f>");
  assert.equal(escapeCell("pre\\|escaped"), "pre\\|escaped");
});

test("firstSentence cuts at sentence boundary and survives dotted filenames", () => {
  assert.equal(firstSentence("First part. Second part."), "First part.");
  assert.equal(
    firstSentence("Initialize a .fallowrc.json configuration file"),
    "Initialize a .fallowrc.json configuration file",
  );
});

test("regeneration is idempotent and preserves content outside markers", () => {
  const once = regenerateSkillMd(DOC, SCHEMA);
  const twice = regenerateSkillMd(once, SCHEMA);
  assert.equal(once, twice);
  assert.ok(once.startsWith("# Skill\n\nHand-written intro stays."));
  assert.ok(once.trimEnd().endsWith("Hand-written outro stays."));
});

test("curated cells are preserved; identity columns are regenerated", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  assert.ok(out.includes("Curated dead code purpose"));
  assert.ok(out.includes("`--changed-since`"));
  assert.ok(out.includes("Curated teaching prose for unused files"));
  assert.ok(out.includes("Curated long analyze prose with call hints"));
  // Identity regenerated: analyze gains its second key param from the manifest.
  assert.ok(out.includes("`issue_types`, `production`"));
});

test("new rows are seeded from the manifest", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  // New issue type seeded with description + note.
  assert.ok(
    out.includes(
      "| `type-only-dependency` | `--unused-deps` | - | - | Dependency only used via import type; Only reported in --production mode |",
    ),
  );
  // New MCP tool seeded; empty key params render as a dash.
  assert.ok(out.includes("| `list_boundaries` | introspection | free | - |"));
});

test("removed rows drop; nested-subcommand rows survive while their parent exists", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  assert.ok(!out.includes("removed-command"));
  assert.ok(out.includes("`coverage upload-source-maps`"));
  const coverageIdx = out.indexOf("| `coverage` |");
  const uploadIdx = out.indexOf("| `coverage upload-source-maps` |");
  assert.ok(coverageIdx !== -1 && uploadIdx > coverageIdx);
});

test("security catalogue and freemium rows stay out of the issue-types table", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  assert.ok(!out.includes("sql-injection"));
  assert.ok(!out.includes("runtime-safe-to-delete"));
  assert.ok(out.includes("`tainted-sink`"));
});

test("seeded cells escape pipes and newlines from manifest text", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  assert.ok(out.includes("List architecture \\| boundary zones and access rules"));
});

test("missing, duplicated, and inverted markers fail loudly", () => {
  assert.throws(
    () => spliceSection("no markers here", "commands", SCHEMA, "f.md"),
    /missing marker.*commands/s,
  );
  const dup = `${DOC}\n<!-- generated:commands:start -->\n<!-- generated:commands:end -->\n`;
  assert.throws(() => spliceSection(dup, "commands", SCHEMA, "f.md"), /duplicated marker/);
  const inverted = "<!-- generated:commands:end -->\n<!-- generated:commands:start -->\n";
  assert.throws(
    () => spliceSection(inverted, "commands", SCHEMA, "f.md"),
    /end marker before start/,
  );
});

test("task-matrix section regenerates from the manifest and is idempotent", () => {
  const once = regenerateSkillMd(DOC, SCHEMA);
  const twice = regenerateSkillMd(once, SCHEMA);
  assert.equal(once, twice);
  // Stale row replaced by the manifest rows.
  assert.ok(!once.includes("stale row that should be regenerated"));
  assert.ok(once.includes("| When the agent is about to... | Run |"));
  assert.ok(once.includes("| delete an unused export or file | `fallow dead-code --trace <file>:<export>` |"));
  // The flag-fragment row appends its note after a semicolon.
  assert.ok(
    once.includes(
      "| scope a monorepo | `--workspace <glob> / --changed-workspaces <ref>`; global flags, prefix any command |",
    ),
  );
});

test("a target without the task-matrix markers regenerates the other sections and is left intact", () => {
  assert.ok(sectionIsAbsent(DOC_WITHOUT_TASK_MATRIX, "task-matrix"));
  assert.ok(!hasSection(DOC_WITHOUT_TASK_MATRIX, "task-matrix"));
  // Tolerance: regeneration must NOT throw on the absent section.
  const out = regenerateSkillMd(DOC_WITHOUT_TASK_MATRIX, SCHEMA);
  // The three adopted sections still regenerate (analyze gains its 2nd param).
  assert.ok(out.includes("`issue_types`, `production`"));
  assert.ok(out.includes("`type-only-dependency`"));
  // No task-matrix markers were injected, and the surrounding prose is intact.
  assert.ok(!out.includes("generated:task-matrix"));
  assert.ok(out.startsWith("# Skill\n\nHand-written intro stays."));
  assert.ok(out.trimEnd().endsWith("Hand-written outro stays."));
  // Idempotent on the tolerant path too.
  assert.equal(regenerateSkillMd(out, SCHEMA), out);
});

test("a half-present task-matrix marker still throws", () => {
  const halfStart = DOC_WITHOUT_TASK_MATRIX.replace(
    "## MCP Tools",
    "<!-- generated:task-matrix:start -->\n\n## MCP Tools",
  );
  assert.ok(!sectionIsAbsent(halfStart, "task-matrix"));
  assert.throws(() => regenerateSkillMd(halfStart, SCHEMA), /missing marker.*task-matrix/s);

  const halfEnd = DOC_WITHOUT_TASK_MATRIX.replace(
    "## MCP Tools",
    "<!-- generated:task-matrix:end -->\n\n## MCP Tools",
  );
  assert.ok(!sectionIsAbsent(halfEnd, "task-matrix"));
  assert.throws(() => regenerateSkillMd(halfEnd, SCHEMA), /missing marker.*task-matrix/s);
});

test("parseExistingTable honors escaped pipes inside cells", () => {
  const { rows } = parseExistingTable(
    "| Tool | Description |\n|---|---|\n| `x` | uses a \\| pipe |\n",
  );
  assert.equal(rows.get("x").get("Description"), "uses a \\| pipe");
});

test("manifest_version and expect-version guards", async () => {
  const { loadSchema } = await import("./generate-agent-docs.mjs");
  const tmp = `${process.env.TMPDIR ?? "/tmp"}/agent-docs-schema-${process.pid}.json`;
  const { writeFileSync, rmSync } = await import("node:fs");
  writeFileSync(tmp, JSON.stringify({ ...SCHEMA, manifest_version: "2" }));
  assert.throws(() => loadSchema({ schemaPath: tmp }), /unsupported manifest_version/);
  writeFileSync(tmp, JSON.stringify(SCHEMA));
  assert.throws(() => loadSchema({ schemaPath: tmp, expectVersion: "9.9.9" }), /expected 9\.9\.9/);
  rmSync(tmp);
});

test("--check exits 1 on drift, writes nothing, and exits 0 when in sync", async () => {
  const { mkdtempSync, writeFileSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { main } = await import("./generate-agent-docs.mjs");

  const dir = mkdtempSync(join(tmpdir(), "agent-docs-check-"));
  const schemaPath = join(dir, "schema.json");
  writeFileSync(schemaPath, JSON.stringify(SCHEMA));
  writeFileSync(join(dir, "SKILL.md"), DOC);

  // DOC is stale relative to SCHEMA: --check must report drift without writing.
  const before = readFileSync(join(dir, "SKILL.md"), "utf8");
  assert.equal(main(["--schema", schemaPath, "--target", dir, "--check"]), 1);
  assert.equal(readFileSync(join(dir, "SKILL.md"), "utf8"), before);

  // Regenerate for real, then --check must pass.
  assert.equal(main(["--schema", schemaPath, "--target", dir]), 0);
  assert.equal(main(["--schema", schemaPath, "--target", dir, "--check"]), 0);
  rmSync(dir, { recursive: true });
});
