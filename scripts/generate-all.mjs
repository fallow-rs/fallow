#!/usr/bin/env node
/**
 * Regenerate every committed generated contract surface from the Rust and
 * manifest sources of truth.
 *
 * Every phase writes to one temporary output root. The complete staged tree is
 * validated before changed files are promoted as a transaction. `--check`
 * performs the same generation and validation but never writes destinations.
 */

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  checkSummaryRowFiles,
  formatSummaryRowProblems,
  hasSummaryRowProblems,
} from "./check-ci-summary-rows.mjs";
import { checkGithubActionsFile } from "./check-contract-surfaces.mjs";
import { contractSurfacePaths } from "./contract-surfaces.mjs";
import { runGenerationTransaction } from "./generation-transaction.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CAPABILITY_SCHEMA_PATH = "npm/fallow/capabilities.json";
const ISSUE_REGISTRY_PATH = "npm/fallow/issue-registry.json";
const OUTPUT_SCHEMA_PATH = "docs/output-schema.json";
const AGENT_DOCS_TARGET = "npm/fallow/skills/fallow";

const run = (cmd, args, options = {}) =>
  execFileSync(cmd, args, {
    cwd: REPO_ROOT,
    encoding: options.encoding ?? "utf8",
    env: options.env ? { ...process.env, ...options.env } : process.env,
    stdio: options.stdio ?? ["ignore", "pipe", "inherit"],
  });

const cargoFallow = (subcommand, ...args) =>
  run("cargo", [
    "run",
    "--quiet",
    "-p",
    "fallow-cli",
    "--bin",
    "fallow",
    "--",
    subcommand,
    ...args,
  ]);

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

const writeStaged = (stagingRoot, path, content) => {
  const destination = join(stagingRoot, path);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, content);
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
    JSON.stringify(
      {
        schema_version: 1,
        source: "fallow schema issue_types",
        issue_types: issueTypes,
      },
      null,
      2,
    ),
  );
};

const generateSchemaFiles = (stagingRoot) => {
  const configSchema = ensureTrailingNewline(cargoFallow("config-schema", "--pretty"));
  const pluginSchema = ensureTrailingNewline(cargoFallow("plugin-schema", "--pretty"));
  const rulePackSchema = ensureTrailingNewline(cargoFallow("rule-pack-schema", "--pretty"));
  const capabilitySchema = ensureTrailingNewline(cargoFallow("schema", "--pretty"));
  const registry = issueRegistry(capabilitySchema);
  const output = ensureTrailingNewline(outputSchema());

  writeStaged(stagingRoot, "schema.json", configSchema);
  writeStaged(stagingRoot, "plugin-schema.json", pluginSchema);
  writeStaged(stagingRoot, "rule-pack-schema.json", rulePackSchema);
  writeStaged(stagingRoot, CAPABILITY_SCHEMA_PATH, capabilitySchema);
  writeStaged(stagingRoot, ISSUE_REGISTRY_PATH, registry);
  writeStaged(stagingRoot, OUTPUT_SCHEMA_PATH, output);
};

const generateExtensionContracts = (stagingRoot) => {
  run("node", ["editors/vscode/scripts/codegen-contracts.mjs"], {
    env: {
      FALLOW_CODEGEN_CAPABILITY_SCHEMA: join(stagingRoot, CAPABILITY_SCHEMA_PATH),
      FALLOW_CODEGEN_OUTPUT_SCHEMA: join(stagingRoot, OUTPUT_SCHEMA_PATH),
      FALLOW_GENERATION_OUTPUT_ROOT: stagingRoot,
    },
    stdio: "inherit",
  });
};

const generateAgentDocs = (stagingRoot) => {
  run(
    "node",
    [
      "scripts/generate-agent-docs.mjs",
      "--schema",
      join(stagingRoot, CAPABILITY_SCHEMA_PATH),
      "--target",
      AGENT_DOCS_TARGET,
      "--output-target",
      join(stagingRoot, AGENT_DOCS_TARGET),
    ],
    { stdio: "inherit" },
  );
};

const generateNapiTypes = (stagingRoot) => {
  run("node", ["crates/napi/scripts/write-dts.mjs"], {
    env: { FALLOW_GENERATION_OUTPUT_ROOT: stagingRoot },
    stdio: "inherit",
  });
};

const generateAllPhases = (stagingRoot) => {
  generateSchemaFiles(stagingRoot);
  generateExtensionContracts(stagingRoot);
  generateAgentDocs(stagingRoot);
  generateNapiTypes(stagingRoot);
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

const checkCiSummaryRows = (stagingRoot) => {
  const result = checkSummaryRowFiles({
    githubPath: join(REPO_ROOT, "action/jq/summary-check.jq"),
    gitlabPath: join(REPO_ROOT, "ci/jq/summary-check.jq"),
    registryPath: join(stagingRoot, ISSUE_REGISTRY_PATH),
  });
  if (hasSummaryRowProblems(result)) {
    throw new Error(`CI summary rows are stale:\n${formatSummaryRowProblems(result)}`);
  }
};

const validateStagedContracts = (stagingRoot) => {
  checkContractSurfaceCoverage();
  checkCiSummaryRows(stagingRoot);
};

const parseArgs = (argv) => {
  const unknown = argv.filter((arg) => arg !== "--check" && arg !== "--help" && arg !== "-h");
  if (unknown.length > 0) {
    throw new Error(`unknown argument: ${unknown[0]}`);
  }
  return {
    check: argv.includes("--check"),
    help: argv.includes("--help") || argv.includes("-h"),
  };
};

export const main = (argv = process.argv.slice(2)) => {
  const { check, help } = parseArgs(argv);
  if (help) {
    console.log("Usage: node scripts/generate-all.mjs [--check]");
    return 0;
  }

  const { driftedPaths } = runGenerationTransaction({
    check,
    generate: generateAllPhases,
    repoRoot: REPO_ROOT,
    surfacePaths: contractSurfacePaths(),
    validate: validateStagedContracts,
  });

  if (check && driftedPaths.length > 0) {
    throw new Error(
      `generated contract surfaces are stale:\n${driftedPaths
        .map((path) => `  - ${path}`)
        .join("\n")}`,
    );
  }
  for (const path of driftedPaths) {
    console.log(`${check ? "stale" : "regenerated"}: ${path}`);
  }
  return 0;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`generate-all: ${error.message}`);
    process.exitCode = 1;
  }
}
