#!/usr/bin/env node
/**
 * Regenerate every committed generated surface from the Rust and manifest
 * sources of truth.
 *
 * Default mode writes files. `--check` renders into a temp dir where possible
 * and exits non-zero on drift without touching committed files.
 */

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CHECK = process.argv.includes("--check");
const HELP = process.argv.includes("--help") || process.argv.includes("-h");

const run = (cmd, args, options = {}) =>
  execFileSync(cmd, args, {
    cwd: REPO_ROOT,
    encoding: options.encoding ?? "utf8",
    stdio: options.stdio ?? ["ignore", "pipe", "inherit"],
  });

const cargoFallow = (subcommand) =>
  run("cargo", ["run", "--quiet", "-p", "fallow-cli", "--bin", "fallow", "--", subcommand]);

const outputSchema = () =>
  run("cargo", [
    "run",
    "--quiet",
    "-p",
    "fallow-cli",
    "--features",
    "schema-emit",
    "--bin",
    "fallow-schema-emit",
  ]);

const read = (path) => readFileSync(join(REPO_ROOT, path), "utf8");
const write = (path, content) => writeFileSync(join(REPO_ROOT, path), content);

const assertSame = (path, actual) => {
  const expected = read(path);
  if (expected !== actual) {
    throw new Error(`${path} is stale`);
  }
};

const ensureTrailingNewline = (text) => (text.endsWith("\n") ? text : `${text}\n`);

const generateSchemaFiles = () => {
  const configSchema = ensureTrailingNewline(cargoFallow("config-schema"));
  const pluginSchema = ensureTrailingNewline(cargoFallow("plugin-schema"));
  const rulePackSchema = ensureTrailingNewline(cargoFallow("rule-pack-schema"));
  const output = ensureTrailingNewline(outputSchema());

  if (CHECK) {
    assertSame("schema.json", configSchema);
    if (existsSync(join(REPO_ROOT, "npm/fallow/schema.json"))) {
      assertSame("npm/fallow/schema.json", configSchema);
    }
    assertSame("plugin-schema.json", pluginSchema);
    assertSame("rule-pack-schema.json", rulePackSchema);
    assertSame("docs/output-schema.json", output);
    return;
  }

  write("schema.json", configSchema);
  write("plugin-schema.json", pluginSchema);
  write("rule-pack-schema.json", rulePackSchema);
  write("docs/output-schema.json", output);
  if (existsSync(join(REPO_ROOT, "npm/fallow/schema.json"))) {
    copyFileSync(join(REPO_ROOT, "schema.json"), join(REPO_ROOT, "npm/fallow/schema.json"));
  }
};

const generateOutputTypes = () => {
  run("pnpm", ["--dir", "editors/vscode", "run", CHECK ? "check:codegen" : "codegen:types"], {
    stdio: "inherit",
  });
};

const generateAgentDocs = () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-generate-all-"));
  const capabilityPath = join(dir, "schema.json");
  try {
    writeFileSync(capabilityPath, ensureTrailingNewline(cargoFallow("schema")));
    const args = [
      "scripts/generate-agent-docs.mjs",
      "--schema",
      capabilityPath,
      "--target",
      "npm/fallow/skills/fallow",
    ];
    if (CHECK) {
      args.push("--check");
    }
    run("node", args, { stdio: "inherit" });
  } finally {
    rmSync(dir, { force: true, recursive: true });
  }
};

const main = () => {
  if (HELP) {
    console.log("Usage: node scripts/generate-all.mjs [--check]");
    return;
  }
  generateSchemaFiles();
  generateOutputTypes();
  generateAgentDocs();
};

try {
  main();
} catch (error) {
  console.error(`generate-all: ${error.message}`);
  process.exitCode = 1;
}
