#!/usr/bin/env node
/**
 * Generate the agent-facing doc tables in the fallow skill tree from the
 * `fallow schema` capability manifest (issues #1188 and #1189).
 *
 * Targets: `<target>/SKILL.md` and, when present,
 * `<target>/references/cli-reference.md` and `<target>/references/mcp.md`.
 * SKILL.md sections:
 *
 *   <!-- generated:commands:start -->    ... <!-- generated:commands:end -->
 *   <!-- generated:issue-types:start --> ... <!-- generated:issue-types:end -->
 *   <!-- generated:task-matrix:start --> ... <!-- generated:task-matrix:end -->
 *
 * references/mcp.md carries the `generated:mcp-tools:*` section (moved out of
 * SKILL.md so the always-loaded entry file stays lean; SKILL.md keeps only a
 * pointer). CLI reference sections use `generated:flags:*` markers for global
 * flags, bare `fallow` combined-mode flags, command-local flags, and the
 * dead-code issue filter table.
 *
 * A target whose text contains NEITHER marker of a section has not adopted
 * that section and is skipped; a half-present marker pair still fails loudly.
 *
 * Merge-splice contract:
 * - IDENTITY columns are always regenerated from the manifest (row set, ids,
 *   filter flags, fixable, suppress comments; kind/license/key-params on
 *   mcp-tools).
 * - CURATED columns (`Purpose` and `Key Flags` on commands, `Description` on
 *   issue-types and mcp-tools) are hand-owned: existing cells are preserved
 *   across regenerations, keyed by the row id in the first column. New rows
 *   seed the curated cell from the manifest; rows whose id left the manifest
 *   are dropped. Note the asymmetry: `Key params` on mcp-tools regenerates
 *   every run, while `Key Flags` on commands is seeded ONCE from the flag
 *   list and never auto-updated afterwards (hand-edit it to change it).
 * - Commands rows whose key contains a space (e.g. `coverage
 *   upload-source-maps`) document nested subcommands the schema does not
 *   enumerate; they are preserved verbatim after their parent row as long as
 *   the parent command still exists.
 * - Everything OUTSIDE the markers is hand-written and never touched. Markers
 *   live on their own lines, outside table rows.
 *
 * Cell escaping contract: `|` becomes `\|`, newline/whitespace runs collapse
 * to one space, backticks and angle brackets pass through untouched (they
 * render fine inside table cells). Curated cells must keep pipes escaped as
 * `\|`; the row parser splits on unescaped pipes only.
 *
 * Usage:
 *   node scripts/generate-agent-docs.mjs --fallow <path-to-fallow-binary> \
 *     --target <skills-tree-dir> [--target <dir> ...] [--check] \
 *     [--expect-version <x.y.z>]
 *   node scripts/generate-agent-docs.mjs --schema <schema.json> --target <dir>
 *
 * `--check` renders in memory and exits 1 listing drifted sections, writing
 * nothing. `--expect-version` guards against a stale binary: the manifest's
 * `version` field must match exactly.
 *
 * Run during /fallow-release (step 5c) against the canonical fallow-skills
 * tree before re-vendoring npm/fallow/skills. Zero dependencies; Node >= 18.
 */

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const SKILL_SECTION_IDS = ["commands", "issue-types", "task-matrix"];
// mcp-tools moved out of SKILL.md into references/mcp.md: the full 20+ tool
// catalogue is reference-grade and (when the server is connected) already
// advertised via live tool schemas, so it does not belong in the always-loaded
// entry file. SKILL.md keeps only a short pointer section.
const MCP_REFERENCE_SECTION_IDS = ["mcp-tools"];
const CLI_REFERENCE_SECTION_IDS = [
  "flags:global",
  "flags:fallow-combined",
  "flags:dead-code",
  "flags:dead-code-filters",
  "flags:dupes",
  "flags:fix",
  "flags:list",
  "flags:init",
  "flags:migrate",
  "flags:health",
  "flags:audit",
  "flags:trace",
  "flags:decision-surface",
  "flags:flags",
  "flags:security",
  "flags:config",
];
export const SECTION_IDS = [
  ...SKILL_SECTION_IDS,
  ...MCP_REFERENCE_SECTION_IDS,
  ...CLI_REFERENCE_SECTION_IDS,
];

/** Security family rows kept in the SKILL.md issue-types table; the ~47
 * per-CWE catalogue categories collapse under tainted-sink there. */
const SECURITY_FAMILY_IDS = new Set(["tainted-sink", "client-server-leak", "hardcoded-secret"]);

const MAX_SEEDED_KEY_FLAGS = 8;

const COMBINED_MODE_FLAGS = [
  "--only",
  "--skip",
  "--production",
  "--no-production",
  "--production-dead-code",
  "--production-health",
  "--production-dupes",
  "--dupes-mode",
  "--dupes-threshold",
  "--dupes-min-tokens",
  "--dupes-min-lines",
  "--dupes-min-occurrences",
  "--dupes-skip-local",
  "--dupes-cross-language",
  "--dupes-ignore-imports",
  "--score",
  "--trend",
  "--save-snapshot",
  "--coverage",
  "--coverage-root",
];

const CLI_REFERENCE_FLAG_SECTIONS = {
  "flags:dead-code": {
    command: "dead-code",
    excludeIssueFilters: true,
    globalRefs: [
      "--format",
      "--quiet",
      "--output-file",
      "--changed-since",
      "--max-file-size",
      "--production",
      "--no-production",
      "--production-dead-code",
      "--baseline",
      "--save-baseline",
      "--workspace",
      "--changed-workspaces",
      "--include-entry-exports",
    ],
  },
  "flags:dupes": {
    command: "dupes",
    globalRefs: [
      "--format",
      "--quiet",
      "--changed-since",
      "--baseline",
      "--save-baseline",
      "--workspace",
      "--changed-workspaces",
      "--group-by",
      "--explain-skipped",
    ],
  },
  "flags:fix": { command: "fix", globalRefs: ["--format", "--quiet"] },
  "flags:list": { command: "list", globalRefs: ["--format", "--quiet"] },
  "flags:init": { command: "init", globalRefs: ["--root", "--config"] },
  "flags:migrate": { command: "migrate", globalRefs: ["--root", "--config"] },
  "flags:health": {
    command: "health",
    globalRefs: [
      "--format",
      "--quiet",
      "--changed-since",
      "--churn-file",
      "--workspace",
      "--group-by",
      "--baseline",
      "--save-baseline",
      "--production",
      "--no-production",
      "--explain",
    ],
  },
  "flags:audit": {
    command: "audit",
    globalRefs: [
      "--format",
      "--quiet",
      "--changed-since",
      "--diff-file",
      "--diff-stdin",
      "--workspace",
      "--changed-workspaces",
      "--group-by",
      "--output-file",
    ],
  },
  "flags:trace": {
    command: "trace",
    globalRefs: ["--format", "--quiet", "--root", "--config"],
  },
  "flags:decision-surface": {
    command: "decision-surface",
    globalRefs: ["--changed-since", "--format", "--quiet", "--workspace", "--root", "--config"],
  },
  "flags:flags": {
    command: "flags",
    globalRefs: ["--format", "--quiet", "--changed-since", "--workspace"],
  },
  "flags:security": {
    command: "security",
    globalRefs: [
      "--format",
      "--quiet",
      "--changed-since",
      "--diff-file",
      "--diff-stdin",
      "--workspace",
      "--changed-workspaces",
    ],
  },
  "flags:config": { command: "config", globalRefs: ["--format", "--quiet", "--config", "--root"] },
};

/** Collapse whitespace and escape unescaped pipes so a cell cannot break the table. */
export const escapeCell = (text) =>
  String(text ?? "")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/\\\|/g, "|")
    .replace(/\|/g, "\\|");

/** First sentence of a description (cut at ". " followed by more text). */
export const firstSentence = (text) => {
  const collapsed = String(text ?? "")
    .replace(/\s+/g, " ")
    .trim();
  const cut = collapsed.indexOf(". ");
  return cut === -1 ? collapsed : collapsed.slice(0, cut + 1);
};

const code = (text) => `\`${text}\``;
const codeOrDash = (text) => (text ? code(text) : "-");
const yesOrDash = (flag) => (flag ? "yes" : "-");

const LONG_FLAG_PATTERN = /--[a-z][a-z0-9-]*/g;

const canonicalRowKey = (cell) => {
  const stripped = cell.replace(/`/g, "").trim();
  const flags = stripped.match(LONG_FLAG_PATTERN);
  return flags?.at(-1) ?? stripped;
};

/** Split a markdown table row into raw cells, honoring `\|` escapes. */
const splitRow = (line) => {
  const cells = [];
  let current = "";
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (ch === "\\" && line[i + 1] === "|") {
      current += "\\|";
      i += 1;
    } else if (ch === "|") {
      cells.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  cells.push(current.trim());
  // A well-formed row starts and ends with a pipe: drop the empty edges.
  return cells.slice(1, -1);
};

/** Parse the existing generated block into { headers, rows: Map<key, Map<header, cell>> }. */
export const parseExistingTable = (block) => {
  const lines = block.split("\n").filter((l) => l.trim().startsWith("|"));
  if (lines.length < 2) {
    return { headers: [], rows: new Map() };
  }
  const headers = splitRow(lines[0]);
  const rows = new Map();
  for (const line of lines.slice(2)) {
    const cells = splitRow(line);
    if (cells.length === 0) {
      continue;
    }
    const key = canonicalRowKey(cells[0]);
    const byHeader = new Map();
    headers.forEach((h, i) => byHeader.set(h, cells[i] ?? ""));
    rows.set(key, byHeader);
  }
  return { headers, rows };
};

const renderTable = (headers, rows) => {
  const lines = [
    `| ${headers.join(" | ")} |`,
    `|${headers.map(() => "---").join("|")}|`,
    ...rows.map((cells) => `| ${cells.join(" | ")} |`),
  ];
  return lines.join("\n");
};

/** Existing curated cell if present and non-empty, else the seed. */
const curatedCell = (existing, key, header, seed) => {
  const cell = existing.rows.get(key)?.get(header);
  return cell !== undefined && cell !== "" ? cell : escapeCell(seed);
};

const renderCommandsSection = (schema, existing) => {
  const headers = ["Command", "Purpose", "Key Flags"];
  const commandNames = new Set(schema.commands.map((c) => c.name));

  // Hand-added nested-subcommand rows (key contains a space), grouped by parent.
  const extrasByParent = new Map();
  for (const [key, cells] of existing.rows) {
    const parent = key.split(" ")[0];
    if (key.includes(" ") && (commandNames.has(parent) || parent === "fallow")) {
      const list = extrasByParent.get(parent) ?? [];
      list.push(headers.map((h) => cells.get(h) ?? ""));
      extrasByParent.set(parent, list);
    }
  }

  const rows = [];
  const pushCommand = (key, purposeSeed, keyFlagsSeed) => {
    rows.push([
      code(key),
      curatedCell(existing, key, "Purpose", purposeSeed),
      curatedCell(existing, key, "Key Flags", keyFlagsSeed),
    ]);
    for (const extra of extrasByParent.get(key) ?? []) {
      rows.push(extra);
    }
  };

  // Bare `fallow` (combined mode) is not in schema.commands[]; synthesize it.
  pushCommand("fallow", firstSentence(schema.default_behavior), "");
  for (const command of schema.commands) {
    const flagSeed = command.flags
      .slice(0, MAX_SEEDED_KEY_FLAGS)
      .map((f) => code(f.name))
      .join(", ");
    pushCommand(command.name, firstSentence(command.description ?? ""), flagSeed);
  }

  return [
    renderTable(headers, rows),
    "",
    "Run `fallow <command> --help` for the full flag list per command (see also references/cli-reference.md).",
  ].join("\n");
};

const issueTypeInTable = (issue) => {
  if (issue.license === "freemium") {
    return false;
  }
  if (issue.command === "security" && !SECURITY_FAMILY_IDS.has(issue.id)) {
    return false;
  }
  return true;
};

const renderIssueTypesSection = (schema, existing) => {
  const headers = ["Type", "Filter flag", "Fixable", "Suppress comment", "Description"];
  const rows = schema.issue_types.filter(issueTypeInTable).map((issue) => {
    const seed = issue.note ? `${issue.description}; ${issue.note}` : issue.description;
    return [
      code(issue.id),
      codeOrDash(issue.filter_flag),
      yesOrDash(issue.fixable),
      codeOrDash(issue.suppress_comment),
      curatedCell(existing, issue.id, "Description", seed),
    ];
  });

  return [
    renderTable(headers, rows),
    "",
    "Runtime-coverage verdicts and the full security sink catalogue are listed by `fallow schema` (`issue_types`).",
  ].join("\n");
};

const renderMcpToolsSection = (schema, existing) => {
  const headers = ["Tool", "Kind", "License", "CLI fallback", "Key params", "Description"];
  const rows = schema.mcp_tools.tools.map((tool) => [
    code(tool.name),
    tool.kind,
    tool.license,
    codeOrDash(tool.cli_command),
    tool.key_params.length > 0 ? tool.key_params.map(code).join(", ") : "-",
    curatedCell(existing, tool.name, "Description", tool.description),
  ]);
  return renderTable(headers, rows);
};

/** Agent task-to-command matrix (R2). Fully regenerated from the manifest;
 * no curated columns. The Run cell backticks the command and appends the note
 * after a semicolon when present. Same two-column table as the Rust
 * `render_task_matrix_markdown`, which renders the note as a parenthesized
 * suffix instead; only the note separator differs between the two surfaces. */
const renderTaskMatrixSection = (schema) => {
  const headers = ["When the agent is about to...", "Run"];
  const rows = (schema.task_matrix ?? []).map((row) => {
    const run = row.note ? `${code(row.command)}; ${row.note}` : code(row.command);
    return [escapeCell(row.task), run];
  });
  return renderTable(headers, rows);
};

const flagDisplaySeed = (flag) => (flag.short ? `${flag.short}, ${flag.name}` : flag.name);

const flagDisplayCell = (existing, flag) => {
  const key = flag.name;
  const cell = existing.rows.get(key)?.get("Flag");
  return cell !== undefined && cell !== "" ? cell : code(flagDisplaySeed(flag));
};

const flagTypeCell = (flag) => {
  const values = (flag.possible_values ?? []).filter(
    (value) => value !== "true" && value !== "false",
  );
  if (values.length > 0) {
    return code(escapeCell(values.join("|")));
  }
  return code(flag.type ?? "string");
};

const flagDefaultCell = (flag) => {
  if (flag.default !== undefined && flag.default !== null) {
    return code(flag.default);
  }
  if (flag.type === "bool") {
    return code("false");
  }
  return "-";
};

const flagRows = (flags, existing) =>
  flags.map((flag) => [
    flagDisplayCell(existing, flag),
    flagTypeCell(flag),
    flagDefaultCell(flag),
    curatedCell(existing, flag.name, "Description", flag.description ?? ""),
  ]);

const commandByName = (schema, name) => {
  const command = schema.commands.find((candidate) => candidate.name === name);
  if (!command) {
    throw new Error(`schema is missing command '${name}'`);
  }
  return command;
};

const flagByName = (schema, name) => {
  const flags = [
    ...(schema.global_flags ?? []),
    ...schema.commands.flatMap((command) => command.flags ?? []),
  ];
  const flag = flags.find((candidate) => candidate.name === name);
  if (!flag) {
    throw new Error(`schema is missing flag '${name}'`);
  }
  return flag;
};

const flagRefs = (schema, names) =>
  names
    .map((name) => flagByName(schema, name).name)
    .map((name) => `[${code(name)}](#global-flags)`)
    .join(", ");

const nonArgumentFlags = (flags) => flags.filter((flag) => flag.name.startsWith("--"));

const deadCodeFilterFlags = (schema) =>
  new Set(
    schema.issue_types
      .filter((issue) => issue.command === "dead-code" && issue.filter_flag)
      .map((issue) => issue.filter_flag),
  );

const renderGlobalFlagsSection = (schema, existing) =>
  renderTable(
    ["Flag", "Type", "Default", "Description"],
    flagRows(schema.global_flags ?? [], existing),
  );

const renderCombinedFlagsSection = (schema, existing) => {
  const flags = COMBINED_MODE_FLAGS.map((name) => flagByName(schema, name));
  return [
    renderTable(["Flag", "Type", "Default", "Description"], flagRows(flags, existing)),
    "",
    "These are global flags with behavior specific to bare `fallow` combined mode.",
  ].join("\n");
};

const issueFilterSet = (schema, config) => {
  if (!config.excludeIssueFilters) {
    return new Set();
  }
  return deadCodeFilterFlags(schema);
};

const globalFlagNote = (schema, names) => {
  if (!names?.length) {
    return "";
  }
  const refs = flagRefs(schema, names);
  if (!refs) {
    return "";
  }
  return `\n\nCommon global flags for this command: ${refs}.`;
};

const renderCommandFlags = (schema, existing, sectionId) => {
  const config = CLI_REFERENCE_FLAG_SECTIONS[sectionId];
  const command = commandByName(schema, config.command);
  const issueFilters = issueFilterSet(schema, config);
  const flags = nonArgumentFlags(command.flags ?? []).filter(
    (flag) => !issueFilters.has(flag.name),
  );
  const table = renderTable(["Flag", "Type", "Default", "Description"], flagRows(flags, existing));
  return `${table}${globalFlagNote(schema, config.globalRefs)}`;
};

const renderCommandFlagsSection = (sectionId) => (schema, existing) =>
  renderCommandFlags(schema, existing, sectionId);

const renderDeadCodeFiltersSection = (schema, existing) => {
  const flagsByName = new Map(
    nonArgumentFlags(commandByName(schema, "dead-code").flags ?? []).map((flag) => [
      flag.name,
      flag,
    ]),
  );
  const grouped = new Map();
  for (const issue of schema.issue_types.filter(
    (candidate) => candidate.command === "dead-code" && candidate.filter_flag,
  )) {
    const list = grouped.get(issue.filter_flag) ?? [];
    list.push(issue);
    grouped.set(issue.filter_flag, list);
  }
  const flagOrder = [...flagsByName.keys()];
  const rows = [...grouped]
    .toSorted(([a], [b]) => flagOrder.indexOf(a) - flagOrder.indexOf(b))
    .map(([filterFlag, issues]) => {
      const flag = flagsByName.get(filterFlag) ?? { name: filterFlag };
      const seed = [...new Set(issues.map((issue) => issue.description))].join("; ");
      return [
        flagDisplayCell(existing, flag),
        curatedCell(existing, filterFlag, "Issue Type", seed),
      ];
    });
  return renderTable(["Flag", "Issue Type"], rows);
};

const RENDERERS = {
  commands: renderCommandsSection,
  "issue-types": renderIssueTypesSection,
  "mcp-tools": renderMcpToolsSection,
  "task-matrix": renderTaskMatrixSection,
  "flags:global": renderGlobalFlagsSection,
  "flags:fallow-combined": renderCombinedFlagsSection,
  "flags:dead-code": renderCommandFlagsSection("flags:dead-code"),
  "flags:dead-code-filters": renderDeadCodeFiltersSection,
  "flags:dupes": renderCommandFlagsSection("flags:dupes"),
  "flags:fix": renderCommandFlagsSection("flags:fix"),
  "flags:list": renderCommandFlagsSection("flags:list"),
  "flags:init": renderCommandFlagsSection("flags:init"),
  "flags:migrate": renderCommandFlagsSection("flags:migrate"),
  "flags:health": renderCommandFlagsSection("flags:health"),
  "flags:audit": renderCommandFlagsSection("flags:audit"),
  "flags:trace": renderCommandFlagsSection("flags:trace"),
  "flags:decision-surface": renderCommandFlagsSection("flags:decision-surface"),
  "flags:flags": renderCommandFlagsSection("flags:flags"),
  "flags:security": renderCommandFlagsSection("flags:security"),
  "flags:config": renderCommandFlagsSection("flags:config"),
};

/** True only when BOTH the start and end markers for a section are present.
 * A fully-absent pair means the target has not adopted this section, so the
 * orchestrator skips it; a half-present pair is malformed and still throws via
 * `spliceSection`. */
export const hasSection = (text, sectionId) => {
  const start = `<!-- generated:${sectionId}:start -->`;
  const end = `<!-- generated:${sectionId}:end -->`;
  return text.includes(start) && text.includes(end);
};

/** True when NEITHER marker is present: the target has not adopted the section
 * at all, so the orchestrator skips it gracefully. A half-present pair (exactly
 * one marker) returns false here and is left to `spliceSection`, which throws on
 * the missing partner. */
export const sectionIsAbsent = (text, sectionId) => {
  const start = `<!-- generated:${sectionId}:start -->`;
  const end = `<!-- generated:${sectionId}:end -->`;
  return !text.includes(start) && !text.includes(end);
};

/** Splice one generated section between its markers. Throws on marker misuse. */
export const spliceSection = (text, sectionId, schema, fileLabel, knownSections = SECTION_IDS) => {
  const start = `<!-- generated:${sectionId}:start -->`;
  const end = `<!-- generated:${sectionId}:end -->`;
  const fail = (reason) => {
    throw new Error(
      `${fileLabel}: ${reason} for section '${sectionId}'. Expected exactly one ` +
        `'${start}' ... '${end}' pair (markers on their own lines). ` +
        `Known sections: ${knownSections.join(", ")}.`,
    );
  };

  const startIdx = text.indexOf(start);
  const endIdx = text.indexOf(end);
  if (startIdx === -1 || endIdx === -1) {
    fail("missing marker");
  }
  if (text.indexOf(start, startIdx + 1) !== -1 || text.indexOf(end, endIdx + 1) !== -1) {
    fail("duplicated marker");
  }
  if (endIdx < startIdx) {
    fail("end marker before start marker");
  }

  const existingBlock = text.slice(startIdx + start.length, endIdx);
  const existing = parseExistingTable(existingBlock);
  const renderer = RENDERERS[sectionId];
  if (!renderer) {
    fail("unknown generated section");
  }
  const rendered = renderer(schema, existing);
  return `${text.slice(0, startIdx + start.length)}\n${rendered}\n${text.slice(endIdx)}`;
};

const regenerateSections = (text, schema, fileLabel, sectionIds) => {
  let out = text;
  for (const sectionId of sectionIds) {
    if (sectionIsAbsent(out, sectionId)) {
      continue;
    }
    out = spliceSection(out, sectionId, schema, fileLabel, sectionIds);
  }
  return out;
};

/** Regenerate every adopted section in a SKILL.md text; returns the new text.
 * A section whose markers are BOTH absent is skipped (target has not adopted
 * it); a half-present / duplicated / inverted marker pair still throws via
 * `spliceSection`. */
export const regenerateSkillMd = (text, schema, fileLabel = "SKILL.md") =>
  regenerateSections(text, schema, fileLabel, SKILL_SECTION_IDS);

export const regenerateCliReferenceMd = (text, schema, fileLabel = "references/cli-reference.md") =>
  regenerateSections(text, schema, fileLabel, CLI_REFERENCE_SECTION_IDS);

export const regenerateMcpReferenceMd = (text, schema, fileLabel = "references/mcp.md") =>
  regenerateSections(text, schema, fileLabel, MCP_REFERENCE_SECTION_IDS);

const changedSections = (before, schema, fileLabel, sectionIds) =>
  sectionIds.filter(
    (id) =>
      !sectionIsAbsent(before, id) &&
      spliceSection(before, id, schema, fileLabel, sectionIds) !== before,
  );

const processFile = ({ file, before, after, schema, sectionIds, check }) => {
  if (after === before) {
    console.log(`up to date: ${file}`);
    return false;
  }
  if (check) {
    const sections = changedSections(before, schema, file, sectionIds);
    console.error(`DRIFT: ${file} (sections: ${sections.join(", ")})`);
    return true;
  }
  writeFileSync(file, after);
  console.log(`regenerated: ${file}`);
  return false;
};

export const loadSchema = ({ fallowBin, schemaPath, expectVersion }) => {
  let raw;
  if (schemaPath) {
    raw = readFileSync(schemaPath, "utf8");
  } else if (fallowBin) {
    raw = execFileSync(fallowBin, ["schema"], {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      env: { ...process.env, FALLOW_QUIET: "1" },
    });
  } else {
    throw new Error("pass --fallow <binary> or --schema <json file>");
  }
  const schema = JSON.parse(raw);
  if (schema.manifest_version !== "1") {
    throw new Error(`unsupported manifest_version: ${schema.manifest_version ?? "(absent)"}`);
  }
  if (expectVersion && schema.version !== expectVersion) {
    throw new Error(
      `schema came from fallow ${schema.version}, expected ${expectVersion}; ` +
        "rebuild the binary before generating docs",
    );
  }
  return schema;
};

const parseArgs = (argv) => {
  const opts = { targets: [], check: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) {
        throw new Error(`${arg} requires a value`);
      }
      return argv[i];
    };
    if (arg === "--fallow") {
      opts.fallowBin = next();
    } else if (arg === "--schema") {
      opts.schemaPath = next();
    } else if (arg === "--target") {
      opts.targets.push(next());
    } else if (arg === "--expect-version") {
      opts.expectVersion = next();
    } else if (arg === "--check") {
      opts.check = true;
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (opts.targets.length === 0) {
    throw new Error("pass at least one --target <skills-tree-dir>");
  }
  return opts;
};

export const main = (argv = process.argv.slice(2)) => {
  const opts = parseArgs(argv);
  const schema = loadSchema(opts);

  let drifted = 0;
  for (const target of opts.targets) {
    const skillFile = join(target, "SKILL.md");
    const skillBefore = readFileSync(skillFile, "utf8");
    const skillAfter = regenerateSkillMd(skillBefore, schema, skillFile);
    if (
      processFile({
        file: skillFile,
        before: skillBefore,
        after: skillAfter,
        schema,
        sectionIds: SKILL_SECTION_IDS,
        check: opts.check,
      })
    ) {
      drifted += 1;
    }

    const cliReferenceFile = join(target, "references", "cli-reference.md");
    if (existsSync(cliReferenceFile)) {
      const cliBefore = readFileSync(cliReferenceFile, "utf8");
      const cliAfter = regenerateCliReferenceMd(cliBefore, schema, cliReferenceFile);
      if (
        processFile({
          file: cliReferenceFile,
          before: cliBefore,
          after: cliAfter,
          schema,
          sectionIds: CLI_REFERENCE_SECTION_IDS,
          check: opts.check,
        })
      ) {
        drifted += 1;
      }
    }

    const mcpReferenceFile = join(target, "references", "mcp.md");
    if (existsSync(mcpReferenceFile)) {
      const mcpBefore = readFileSync(mcpReferenceFile, "utf8");
      const mcpAfter = regenerateMcpReferenceMd(mcpBefore, schema, mcpReferenceFile);
      if (
        processFile({
          file: mcpReferenceFile,
          before: mcpBefore,
          after: mcpAfter,
          schema,
          sectionIds: MCP_REFERENCE_SECTION_IDS,
          check: opts.check,
        })
      ) {
        drifted += 1;
      }
    }
  }
  return drifted === 0 ? 0 : 1;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`generate-agent-docs: ${error.message}`);
    process.exitCode = 2;
  }
}
