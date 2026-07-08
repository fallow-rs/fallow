import assert from "node:assert/strict";
import { test } from "node:test";

import {
  escapeCell,
  firstSentence,
  hasSection,
  parseExistingTable,
  regenerateCliReferenceMd,
  regenerateMcpReferenceMd,
  regenerateSkillMd,
  sectionIsAbsent,
  spliceSection,
} from "./generate-agent-docs.mjs";

const boolFlag = (name, description = `${name} description`) => ({
  name,
  type: "bool",
  required: false,
  description,
  possible_values: ["true", "false"],
});

const stringFlag = (name, description = `${name} description`, extra = {}) => ({
  name,
  type: "string",
  required: false,
  description,
  ...extra,
});

const SCHEMA = {
  version: "0.0.0-test",
  manifest_version: "1",
  default_behavior: "Runs all analyses (check + dupes + health). Use --only/--skip to select.",
  global_flags: [
    stringFlag("--format", "Output format", {
      short: "-f",
      default: "human",
      possible_values: ["human", "json"],
    }),
    boolFlag("--quiet", "Suppress progress output"),
    stringFlag("--output-file", "Write the report to a file", { short: "-o" }),
    boolFlag("--legacy-envelope"),
    stringFlag("--changed-since"),
    stringFlag("--max-file-size"),
    boolFlag("--production"),
    boolFlag("--no-production"),
    boolFlag("--production-dead-code"),
    boolFlag("--production-health"),
    boolFlag("--production-dupes"),
    stringFlag("--baseline"),
    stringFlag("--save-baseline"),
    stringFlag("--workspace"),
    stringFlag("--changed-workspaces"),
    boolFlag("--include-entry-exports"),
    stringFlag("--only"),
    stringFlag("--skip"),
    stringFlag("--dupes-mode"),
    stringFlag("--dupes-threshold"),
    stringFlag("--dupes-min-tokens"),
    stringFlag("--dupes-min-lines"),
    stringFlag("--dupes-min-occurrences"),
    boolFlag("--dupes-skip-local"),
    boolFlag("--dupes-cross-language"),
    boolFlag("--dupes-ignore-imports"),
    boolFlag("--score"),
    boolFlag("--trend"),
    stringFlag("--save-snapshot"),
    stringFlag("--coverage"),
    stringFlag("--coverage-root"),
    stringFlag("--dupes-random"),
    stringFlag("--root", "Project root directory", { short: "-r" }),
    stringFlag("--config", "Config file path", { short: "-c" }),
    stringFlag("--churn-file"),
    stringFlag("--group-by"),
    boolFlag("--explain"),
    boolFlag("--explain-skipped"),
    stringFlag("--diff-file"),
    boolFlag("--diff-stdin"),
  ],
  commands: [
    {
      name: "dead-code",
      description: "Analyze project for unused code. Second sentence is cut.",
      flags: [
        boolFlag("--unused-files", "Only report unused files"),
        boolFlag("--unused-exports", "Only report unused exports"),
        boolFlag("--unused-deps", "Only report unused dependencies"),
        boolFlag("--trace", "Trace export usage"),
        stringFlag("--file", "Scope output to files"),
      ],
    },
    {
      name: "coverage",
      description: "Runtime coverage workflow",
      flags: [],
    },
    {
      name: "dupes",
      description: "Find clones",
      flags: [
        stringFlag("--mode", "Detection mode", {
          default: "mild",
          possible_values: ["strict", "mild"],
        }),
        stringFlag("--trace", "Trace clones"),
      ],
    },
    {
      name: "fix",
      description: "Apply fixes",
      flags: [boolFlag("--dry-run"), boolFlag("--yes"), boolFlag("--no-create-config")],
    },
    {
      name: "list",
      description: "List project data",
      flags: [boolFlag("--entry-points")],
    },
    {
      name: "init",
      description: "Initialize config",
      flags: [boolFlag("--toml")],
    },
    {
      name: "migrate",
      description: "Migrate config",
      flags: [boolFlag("--dry-run")],
    },
    {
      name: "health",
      description: "Analyze health",
      flags: [stringFlag("--top")],
    },
    {
      name: "audit",
      description: "Audit changed files",
      flags: [boolFlag("--strict")],
    },
    {
      name: "flags",
      description: "Analyze feature flags",
      flags: [stringFlag("--top")],
    },
    {
      name: "security",
      description: "Analyze security candidates",
      flags: [stringFlag("--gate")],
    },
    {
      name: "config",
      description: "Show config",
      flags: [stringFlag("--path")],
    },
    {
      name: "explain",
      description: "Explain issue types",
      flags: [{ name: "issue_type", type: "string", description: "Issue type to explain" }],
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

Hand-written outro stays.
`;

/** A dedicated references/mcp.md target: the mcp-tools section moved here out of
 * SKILL.md. Stale relative to SCHEMA (analyze is missing its 2nd key param and
 * list_boundaries is absent) so regeneration has work to do. */
const DOC_MCP_REFERENCE = `# Fallow MCP Server Reference

Hand-written intro stays.

## Tool catalogue

<!-- generated:mcp-tools:start -->
| Tool | Kind | License | Key params | Description |
|---|---|---|---|---|
| \`analyze\` | analysis | free | \`issue_types\` | Curated long analyze prose with call hints |
<!-- generated:mcp-tools:end -->

Hand-written outro stays.
`;

const DOC_CLI_REFERENCE = `# Fallow CLI Reference

## \`dead-code\`: Dead Code Analysis

### Flags

<!-- generated:flags:dead-code:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| \`--format\` | \`human\\|json\` | \`human\` | Old global row should move out |
| \`--trace\` | \`bool\` | \`false\` | Curated trace description |
| \`--removed\` | \`bool\` | \`false\` | Should disappear |
<!-- generated:flags:dead-code:end -->

### Issue Type Filters

<!-- generated:flags:dead-code-filters:start -->
| Flag | Issue Type |
|---|---|
| \`--unused-files\` | Curated unused files prose |
<!-- generated:flags:dead-code-filters:end -->

## \`dupes\`: Duplication Detection

### Flags

<!-- generated:flags:dupes:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| \`--mode\` | \`strict\\|mild\` | \`mild\` | Curated mode prose |
<!-- generated:flags:dupes:end -->

## \`explain\`: Rule Explanation

Arguments are hand-written here.

## Global Flags

<!-- generated:flags:global:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| \`-o, --output-file\` | \`string\` | - | Curated output file prose |
| \`--removed-global\` | \`string\` | - | Should disappear |
<!-- generated:flags:global:end -->

### Combined Mode Flags

<!-- generated:flags:fallow-combined:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| \`--only\` | \`string\` | - | Curated only prose |
<!-- generated:flags:fallow-combined:end -->
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
});

test("new rows are seeded from the manifest", () => {
  const out = regenerateSkillMd(DOC, SCHEMA);
  // New issue type seeded with description + note.
  assert.ok(
    out.includes(
      "| `type-only-dependency` | `--unused-deps` | - | - | Dependency only used via import type; Only reported in --production mode |",
    ),
  );
});

test("mcp-tools regenerates in references/mcp.md: curated preserved, identity + new rows", () => {
  const out = regenerateMcpReferenceMd(DOC_MCP_REFERENCE, SCHEMA);
  // Idempotent.
  assert.equal(regenerateMcpReferenceMd(out, SCHEMA), out);
  // Curated Description preserved across regeneration.
  assert.ok(out.includes("Curated long analyze prose with call hints"));
  // Identity regenerated: analyze gains its second key param from the manifest.
  assert.ok(out.includes("`issue_types`, `production`"));
  // New tool seeded; empty key params render as a dash.
  assert.ok(out.includes("| `list_boundaries` | introspection | free | - |"));
  // Hand-written prose outside the markers is untouched.
  assert.ok(out.startsWith("# Fallow MCP Server Reference"));
  assert.ok(out.trimEnd().endsWith("Hand-written outro stays."));
});

test("SKILL.md no longer carries or regenerates an mcp-tools section", () => {
  assert.ok(sectionIsAbsent(DOC, "mcp-tools"));
  const out = regenerateSkillMd(DOC, SCHEMA);
  assert.ok(!out.includes("generated:mcp-tools"));
  assert.ok(!out.includes("list_boundaries"));
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
  const out = regenerateMcpReferenceMd(DOC_MCP_REFERENCE, SCHEMA);
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
  assert.ok(
    once.includes(
      "| delete an unused export or file | `fallow dead-code --trace <file>:<export>` |",
    ),
  );
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
  // The adopted sections still regenerate: new command + issue-type rows appear.
  assert.ok(out.includes("| `dupes` |"));
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
    "Hand-written outro stays.",
    "<!-- generated:task-matrix:start -->\n\nHand-written outro stays.",
  );
  assert.ok(!sectionIsAbsent(halfStart, "task-matrix"));
  assert.throws(() => regenerateSkillMd(halfStart, SCHEMA), /missing marker.*task-matrix/s);

  const halfEnd = DOC_WITHOUT_TASK_MATRIX.replace(
    "Hand-written outro stays.",
    "<!-- generated:task-matrix:end -->\n\nHand-written outro stays.",
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

test("CLI reference flag sections regenerate from the manifest and preserve curated cells", () => {
  const out = regenerateCliReferenceMd(DOC_CLI_REFERENCE, SCHEMA);
  assert.equal(regenerateCliReferenceMd(out, SCHEMA), out);
  assert.match(out, /\| `--trace` \| `bool` \| `false` \| Curated trace description \|/);
  assert.match(out, /\| `--mode` \| `strict\\\|mild` \| `mild` \| Curated mode prose \|/);
  assert.match(out, /\| `-o, --output-file` \| `string` \| - \| Curated output file prose \|/);
  assert.doesNotMatch(out, /Old global row should move out/);
  assert.doesNotMatch(out, /--removed/);
  assert.doesNotMatch(out, /--removed-global/);
});

test("CLI reference keeps issue filters separate from command flags", () => {
  const out = regenerateCliReferenceMd(DOC_CLI_REFERENCE, SCHEMA);
  const deadCodeBlock = out.slice(
    out.indexOf("<!-- generated:flags:dead-code:start -->"),
    out.indexOf("<!-- generated:flags:dead-code:end -->"),
  );
  const filterBlock = out.slice(
    out.indexOf("<!-- generated:flags:dead-code-filters:start -->"),
    out.indexOf("<!-- generated:flags:dead-code-filters:end -->"),
  );
  assert.doesNotMatch(deadCodeBlock, /--unused-files/);
  assert.match(deadCodeBlock, /--trace/);
  assert.match(filterBlock, /\| `--unused-files` \| Curated unused files prose \|/);
  assert.match(filterBlock, /\| `--unused-deps` \|/);
});

test("CLI reference combined mode uses the explicit allow-list", () => {
  const out = regenerateCliReferenceMd(DOC_CLI_REFERENCE, SCHEMA);
  const block = out.slice(
    out.indexOf("<!-- generated:flags:fallow-combined:start -->"),
    out.indexOf("<!-- generated:flags:fallow-combined:end -->"),
  );
  assert.match(block, /\| `--dupes-mode` \|/);
  assert.match(block, /\| `--coverage` \|/);
  assert.match(block, /\| `--coverage-root` \|/);
  assert.match(block, /\| `--only` \| `string` \| - \| Curated only prose \|/);
  assert.doesNotMatch(block, /--dupes-random/);
});

test("CLI reference fails loudly when an explicit global reference mapping is stale", () => {
  const schema = {
    ...SCHEMA,
    global_flags: SCHEMA.global_flags.filter((flag) => flag.name !== "--format"),
  };
  assert.throws(
    () => regenerateCliReferenceMd(DOC_CLI_REFERENCE, schema),
    /schema is missing flag '--format'/,
  );
});

test("CLI reference ignores command arguments that are not flags", () => {
  const out = regenerateCliReferenceMd(DOC_CLI_REFERENCE, SCHEMA);
  assert.match(out, /Arguments are hand-written here\./);
  assert.doesNotMatch(out, /issue_type/);
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
  const { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { main } = await import("./generate-agent-docs.mjs");

  const dir = mkdtempSync(join(tmpdir(), "agent-docs-check-"));
  const referencesDir = join(dir, "references");
  const schemaPath = join(dir, "schema.json");
  writeFileSync(schemaPath, JSON.stringify(SCHEMA));
  writeFileSync(join(dir, "SKILL.md"), DOC);
  mkdirSync(referencesDir);
  writeFileSync(join(referencesDir, "cli-reference.md"), DOC_CLI_REFERENCE);
  writeFileSync(join(referencesDir, "mcp.md"), DOC_MCP_REFERENCE);

  // DOC is stale relative to SCHEMA: --check must report drift without writing.
  const before = readFileSync(join(dir, "SKILL.md"), "utf8");
  const cliBefore = readFileSync(join(referencesDir, "cli-reference.md"), "utf8");
  const mcpBefore = readFileSync(join(referencesDir, "mcp.md"), "utf8");
  assert.equal(main(["--schema", schemaPath, "--target", dir, "--check"]), 1);
  assert.equal(readFileSync(join(dir, "SKILL.md"), "utf8"), before);
  assert.equal(readFileSync(join(referencesDir, "cli-reference.md"), "utf8"), cliBefore);
  assert.equal(readFileSync(join(referencesDir, "mcp.md"), "utf8"), mcpBefore);

  // Regenerate for real, then --check must pass.
  assert.equal(main(["--schema", schemaPath, "--target", dir]), 0);
  assert.equal(main(["--schema", schemaPath, "--target", dir, "--check"]), 0);
  // The mcp-tools table now lives in references/mcp.md, not SKILL.md.
  assert.ok(readFileSync(join(referencesDir, "mcp.md"), "utf8").includes("list_boundaries"));
  assert.ok(!readFileSync(join(dir, "SKILL.md"), "utf8").includes("generated:mcp-tools"));
  rmSync(dir, { recursive: true });
});
