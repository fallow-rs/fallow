#!/usr/bin/env node
/**
 * Regenerate every committed generated contract surface from the Rust and
 * manifest sources of truth.
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

import {
  checkSummaryRowFiles,
  formatSummaryRowProblems,
  hasSummaryRowProblems,
} from "./check-ci-summary-rows.mjs";
import { checkGithubActionsFile } from "./check-contract-surfaces.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CHECK = process.argv.includes("--check");
const HELP = process.argv.includes("--help") || process.argv.includes("-h");
const CAPABILITY_SCHEMA_PATH = "npm/fallow/capabilities.json";
const ISSUE_REGISTRY_PATH = "npm/fallow/issue-registry.json";

const run = (cmd, args, options = {}) =>
  execFileSync(cmd, args, {
    cwd: REPO_ROOT,
    encoding: options.encoding ?? "utf8",
    env: options.env ? { ...process.env, ...options.env } : process.env,
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

const issueRegistry = (capabilitySchema) => {
  const parsed = JSON.parse(capabilitySchema);
  const issueTypes = (parsed.issue_types ?? []).toSorted((a, b) => {
    const leftIndex = Number.isInteger(a.registry_index)
      ? a.registry_index
      : Number.MAX_SAFE_INTEGER;
    const rightIndex = Number.isInteger(b.registry_index)
      ? b.registry_index
      : Number.MAX_SAFE_INTEGER;
    return leftIndex - rightIndex || String(a.id).localeCompare(String(b.id));
  });
  return ensureTrailingNewline(
    `${JSON.stringify(
      {
        schema_version: 1,
        source: "fallow schema issue_types",
        issue_types: issueTypes,
      },
      null,
      2,
    )}`,
  );
};

const generateSchemaFiles = () => {
  const configSchema = ensureTrailingNewline(cargoFallow("config-schema"));
  const pluginSchema = ensureTrailingNewline(cargoFallow("plugin-schema"));
  const rulePackSchema = ensureTrailingNewline(cargoFallow("rule-pack-schema"));
  const capabilitySchema = ensureTrailingNewline(cargoFallow("schema"));
  const registry = issueRegistry(capabilitySchema);
  const output = ensureTrailingNewline(outputSchema());

  if (CHECK) {
    assertSame("schema.json", configSchema);
    if (existsSync(join(REPO_ROOT, "npm/fallow/schema.json"))) {
      assertSame("npm/fallow/schema.json", configSchema);
    }
    assertSame("plugin-schema.json", pluginSchema);
    assertSame("rule-pack-schema.json", rulePackSchema);
    assertSame(CAPABILITY_SCHEMA_PATH, capabilitySchema);
    assertSame(ISSUE_REGISTRY_PATH, registry);
    assertSame("docs/output-schema.json", output);
    return capabilitySchema;
  }

  write("schema.json", configSchema);
  write("plugin-schema.json", pluginSchema);
  write("rule-pack-schema.json", rulePackSchema);
  write(CAPABILITY_SCHEMA_PATH, capabilitySchema);
  write(ISSUE_REGISTRY_PATH, registry);
  write("docs/output-schema.json", output);
  if (existsSync(join(REPO_ROOT, "npm/fallow/schema.json"))) {
    copyFileSync(join(REPO_ROOT, "schema.json"), join(REPO_ROOT, "npm/fallow/schema.json"));
  }
  return capabilitySchema;
};

const generateExtensionContracts = (capabilityPath) => {
  run("pnpm", ["--dir", "editors/vscode", "run", CHECK ? "check:contracts" : "codegen:contracts"], {
    env: { FALLOW_CODEGEN_CAPABILITY_SCHEMA: capabilityPath },
    stdio: "inherit",
  });
};

const generateAgentDocs = (capabilityPath) => {
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
};

const generateNapiTypes = () => {
  const args = ["crates/napi/scripts/write-dts.mjs"];
  if (CHECK) {
    args.push("--check");
  }
  run("node", args, { stdio: "inherit" });
};

const checkContractSurfaceCoverage = () => {
  const result = checkGithubActionsFile(join(REPO_ROOT, ".github/workflows/ci.yml"), undefined, {
    filterName: "rust",
  });
  if (result.missing.length > 0) {
    throw new Error(
      `generated contract surfaces are missing from CI path filters: ${result.missing
        .map(({ path }) => path)
        .join(", ")}`,
    );
  }
};

const checkCiSummaryRows = () => {
  const result = checkSummaryRowFiles({
    githubPath: join(REPO_ROOT, "action/jq/summary-check.jq"),
    gitlabPath: join(REPO_ROOT, "ci/jq/summary-check.jq"),
    registryPath: join(REPO_ROOT, ISSUE_REGISTRY_PATH),
  });
  if (hasSummaryRowProblems(result)) {
    throw new Error(`CI summary rows are stale:\n${formatSummaryRowProblems(result)}`);
  }
};

const withCapabilitySchemaFile = (capabilitySchema, callback) => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-generate-all-"));
  const capabilityPath = join(dir, "schema.json");
  try {
    writeFileSync(capabilityPath, capabilitySchema);
    callback(capabilityPath);
  } finally {
    rmSync(dir, { force: true, recursive: true });
  }
};

const main = () => {
  if (HELP) {
    console.log("Usage: node scripts/generate-all.mjs [--check]");
    return;
  }
  const capabilitySchema = generateSchemaFiles();
  withCapabilitySchemaFile(capabilitySchema, (capabilityPath) => {
    generateExtensionContracts(capabilityPath);
    generateAgentDocs(capabilityPath);
  });
  generateNapiTypes();
  if (CHECK) {
    checkContractSurfaceCoverage();
    checkCiSummaryRows();
  }
};

try {
  main();
} catch (error) {
  console.error(`generate-all: ${error.message}`);
  process.exitCode = 1;
}
