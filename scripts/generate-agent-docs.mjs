#!/usr/bin/env node
/**
 * Generate the agent-facing doc tables in the fallow skill tree from the
 * `fallow schema` capability manifest (issue #1188).
 *
 * Targets (v1): `<target>/SKILL.md` only. Three marker-wrapped sections:
 *
 *   <!-- generated:commands:start -->    ... <!-- generated:commands:end -->
 *   <!-- generated:issue-types:start --> ... <!-- generated:issue-types:end -->
 *   <!-- generated:mcp-tools:start -->   ... <!-- generated:mcp-tools:end -->
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
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const SECTION_IDS = ["commands", "issue-types", "mcp-tools"];

/** Security family rows kept in the SKILL.md issue-types table; the ~47
 * per-CWE catalogue categories collapse under tainted-sink there. */
const SECURITY_FAMILY_IDS = new Set(["tainted-sink", "client-server-leak", "hardcoded-secret"]);

const MAX_SEEDED_KEY_FLAGS = 8;

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
    const key = cells[0].replace(/`/g, "").trim();
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
  const headers = ["Tool", "Kind", "License", "Key params", "Description"];
  const rows = schema.mcp_tools.tools.map((tool) => [
    code(tool.name),
    tool.kind,
    tool.license,
    tool.key_params.length > 0 ? tool.key_params.map(code).join(", ") : "-",
    curatedCell(existing, tool.name, "Description", tool.description),
  ]);
  return renderTable(headers, rows);
};

const RENDERERS = {
  commands: renderCommandsSection,
  "issue-types": renderIssueTypesSection,
  "mcp-tools": renderMcpToolsSection,
};

/** Splice one generated section between its markers. Throws on marker misuse. */
export const spliceSection = (text, sectionId, schema, fileLabel) => {
  const start = `<!-- generated:${sectionId}:start -->`;
  const end = `<!-- generated:${sectionId}:end -->`;
  const fail = (reason) => {
    throw new Error(
      `${fileLabel}: ${reason} for section '${sectionId}'. Expected exactly one ` +
        `'${start}' ... '${end}' pair (markers on their own lines). ` +
        `Known sections: ${SECTION_IDS.join(", ")}.`,
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
  const rendered = RENDERERS[sectionId](schema, existing);
  return `${text.slice(0, startIdx + start.length)}\n${rendered}\n${text.slice(endIdx)}`;
};

/** Regenerate every section in a SKILL.md text; returns the new text. */
export const regenerateSkillMd = (text, schema, fileLabel = "SKILL.md") => {
  let out = text;
  for (const sectionId of SECTION_IDS) {
    out = spliceSection(out, sectionId, schema, fileLabel);
  }
  return out;
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
    const file = join(target, "SKILL.md");
    const before = readFileSync(file, "utf8");
    const after = regenerateSkillMd(before, schema, file);
    if (after === before) {
      console.log(`up to date: ${file}`);
    } else if (opts.check) {
      drifted += 1;
      const sections = SECTION_IDS.filter(
        (id) => spliceSection(before, id, schema, file) !== before,
      );
      console.error(`DRIFT: ${file} (sections: ${sections.join(", ")})`);
    } else {
      writeFileSync(file, after);
      console.log(`regenerated: ${file}`);
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
