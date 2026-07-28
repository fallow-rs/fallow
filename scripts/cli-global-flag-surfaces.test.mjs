/**
 * Guards the generated agent-facing contract surfaces against a new global clap
 * flag that lands in `crates/cli/src/lib.rs` without a contract regeneration
 * (issue #2010). `fallow schema` reflects the clap command, so the committed
 * capability manifest and the generated CLI reference both go stale silently.
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CLI_SOURCE = join(REPO_ROOT, "crates/cli/src/lib.rs");
const CAPABILITIES = join(REPO_ROOT, "npm/fallow/capabilities.json");
const CLI_REFERENCE = join(REPO_ROOT, "npm/fallow/skills/fallow/references/cli-reference.md");

/** `--help` and `--version` are clap builtins that `build_cli_schema` drops. */
const SCHEMA_EXCLUDED_FLAGS = new Set(["--help", "--version"]);

const ARG_FIELD = /#\[arg\(([\s\S]*?)\)\]\s*(?:#\[[\s\S]*?\]\s*)*([a-z0-9_]+)\s*:/g;

/** Long flag names declared on the root `Cli` struct, in declaration order. */
const declaredGlobalFlags = () => {
  const source = readFileSync(CLI_SOURCE, "utf8");
  const start = source.indexOf("\nstruct Cli {");
  assert.notEqual(start, -1, "crates/cli/src/lib.rs should declare `struct Cli`");
  const body = source.slice(start, source.indexOf("\n}\n", start));
  const names = [];
  for (const [, attributes, field] of body.matchAll(ARG_FIELD)) {
    const explicit = attributes.match(/\blong\s*=\s*"([^"]+)"/);
    const name = explicit ? `--${explicit[1]}` : `--${field.replaceAll("_", "-")}`;
    if (!explicit && !/\blong\b/.test(attributes)) {
      continue;
    }
    if (!SCHEMA_EXCLUDED_FLAGS.has(name)) {
      names.push(name);
    }
  }
  assert.ok(names.length > 10, "root flag extraction should find the global flag block");
  return names;
};

const capabilityGlobalFlags = () =>
  JSON.parse(readFileSync(CAPABILITIES, "utf8")).global_flags ?? [];

const referenceSection = (id) => {
  const text = readFileSync(CLI_REFERENCE, "utf8");
  const start = text.indexOf(`<!-- generated:${id}:start -->`);
  const end = text.indexOf(`<!-- generated:${id}:end -->`);
  assert.ok(start !== -1 && end > start, `cli-reference should carry the ${id} section`);
  return text.slice(start, end);
};

test("every global CLI flag reaches the committed capability manifest", () => {
  const declared = declaredGlobalFlags();
  const published = capabilityGlobalFlags().map((flag) => flag.name);
  const missing = declared.filter((name) => !published.includes(name));
  assert.deepEqual(
    missing,
    [],
    "run `npm run generate:contracts` after adding a global flag to `struct Cli`",
  );
  const extra = published.filter((name) => !declared.includes(name));
  assert.deepEqual(extra, [], "capability manifest lists flags the CLI no longer declares");
});

test("every published global flag has a CLI reference row with a description", () => {
  const rows = referenceSection("flags:global")
    .split("\n")
    .filter((line) => line.startsWith("| "))
    .map((line) =>
      line
        .split(/(?<!\\)\|/)
        .slice(1, -1)
        .map((cell) => cell.trim()),
    );
  for (const flag of capabilityGlobalFlags()) {
    // Curated cells may merge sibling flags into one `Flag` cell.
    const row = rows.find(([name]) =>
      name
        .replaceAll("`", "")
        .split(/[\s,/]+/)
        .includes(flag.name),
    );
    assert.ok(row, `cli-reference global flags table is missing a row for ${flag.name}`);
    assert.ok(row.at(-1), `cli-reference row for ${flag.name} has an empty description`);
  }
});

test("the health flags section points at the baseline global flags", () => {
  const section = referenceSection("flags:health");
  for (const flag of ["--baseline", "--baseline-mode", "--save-baseline"]) {
    assert.ok(
      section.includes(`[\`${flag}\`](#global-flags)`),
      `health section should reference ${flag}`,
    );
  }
});
