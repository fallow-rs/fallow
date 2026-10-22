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
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { assertLocalResolution } from "./assert-local-resolution.mjs";
import { checkGithubActionsFile } from "./check-contract-surfaces.mjs";
import {
  MUTATION_COMPANION_FILES,
  MUTATION_COMPANION_SOURCE_ROOT,
  MUTATION_COMPANION_TARGET_ROOT,
  contractSurfacePaths,
} from "./contract-surfaces.mjs";
import { generateAgentAdapters } from "./generate-agent-adapters.mjs";
import { runGenerationTransaction } from "./generation-transaction.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CAPABILITY_SCHEMA_PATH = "npm/fallow/capabilities.json";
const ISSUE_REGISTRY_PATH = "npm/fallow/issue-registry.json";
const OUTPUT_SCHEMA_PATH = "docs/output-schema.json";
const SIMILAR_CODE_SNAPSHOT_SCHEMA_PATH = "crates/api/similar-code-snapshot-schema.json";
/**
 * The maintainer copy in `.agents/skills/fallow` is not a target here. The
 * adapter generator mirrors it from the released tree, so the released tree
 * has one writer and the copy cannot disagree with it.
 */
const AGENT_DOCS_TARGETS = ["npm/fallow/skills/fallow"];
const AGENT_DOCS_SEED_RECORD_PATH = "scripts/agent-doc-curated-seeds.json";
const TYPE_AWARE_MANIFEST_PATH = "crates/api/type-aware-protocol.json";
const TYPE_AWARE_MODULE_PATH = "tools/type-aware-sidecar/src/generated-protocol.mjs";
const MUTATION_RUNNER_MANIFEST_PATH = "crates/api/mutation-runner-protocol.json";
const MUTATION_RUNNER_PROTOCOL_FILE = "generated-protocol.mjs";
const EXTENSION_CODEGEN_PATH = "editors/vscode/scripts/codegen-contracts.mjs";

/**
 * Node's default stdout ceiling for a synchronous child is 1 MiB, and the
 * emitted output schema passed that, which fails the whole generation with a
 * bare `spawnSync cargo ENOBUFS` rather than anything naming a size. The
 * generated surfaces only grow, so the ceiling is raised well past the current
 * figure instead of tracked against it.
 */
const MAX_CHILD_OUTPUT_BYTES = 64 * 1024 * 1024;

const run = (cmd, args, options = {}) =>
  execFileSync(cmd, args, {
    cwd: REPO_ROOT,
    encoding: options.encoding ?? "utf8",
    env: options.env ? { ...process.env, ...options.env } : process.env,
    stdio: options.stdio ?? ["ignore", "pipe", "inherit"],
    maxBuffer: MAX_CHILD_OUTPUT_BYTES,
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

const outputSchema = (...args) =>
  run("cargo", [
    "run",
    "--quiet",
    "-p",
    "fallow-cli",
    "--features",
    "schema-emit",
    "--bin",
    "fallow-schema-emit",
    "--",
    ...args,
  ]);

const writeStaged = (stagingRoot, path, content) => {
  const destination = join(stagingRoot, path);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, content);
};

export const ensureTrailingNewline = (text) => (text.endsWith("\n") ? text : `${text}\n`);

export const issueRegistry = (capabilitySchema) => {
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
  const similarCodeSnapshotSchema = ensureTrailingNewline(outputSchema("--similar-code-snapshot"));

  writeStaged(stagingRoot, "schema.json", configSchema);
  writeStaged(stagingRoot, "plugin-schema.json", pluginSchema);
  writeStaged(stagingRoot, "rule-pack-schema.json", rulePackSchema);
  writeStaged(stagingRoot, CAPABILITY_SCHEMA_PATH, capabilitySchema);
  writeStaged(stagingRoot, ISSUE_REGISTRY_PATH, registry);
  writeStaged(stagingRoot, OUTPUT_SCHEMA_PATH, output);
  writeStaged(stagingRoot, SIMILAR_CODE_SNAPSHOT_SCHEMA_PATH, similarCodeSnapshotSchema);
};

const generateExtensionContracts = (stagingRoot) => {
  run("node", [EXTENSION_CODEGEN_PATH], {
    env: {
      FALLOW_CODEGEN_CAPABILITY_SCHEMA: join(stagingRoot, CAPABILITY_SCHEMA_PATH),
      FALLOW_CODEGEN_OUTPUT_SCHEMA: join(stagingRoot, OUTPUT_SCHEMA_PATH),
      FALLOW_GENERATION_OUTPUT_ROOT: stagingRoot,
    },
    stdio: "inherit",
  });
};

const generateAgentDocs = (stagingRoot) => {
  const targetArgs = AGENT_DOCS_TARGETS.flatMap((target) => [
    "--target",
    target,
    "--output-target",
    join(stagingRoot, target),
  ]);
  run(
    "node",
    [
      "scripts/generate-agent-docs.mjs",
      "--schema",
      join(stagingRoot, CAPABILITY_SCHEMA_PATH),
      ...targetArgs,
      "--seed-record-output",
      join(stagingRoot, AGENT_DOCS_SEED_RECORD_PATH),
    ],
    { stdio: "inherit" },
  );
};

const generateNapiTypes = (stagingRoot) => {
  run("node", ["crates/napi/scripts/write-dts-entry.mjs"], {
    env: { FALLOW_GENERATION_OUTPUT_ROOT: stagingRoot },
    stdio: "inherit",
  });
};

export const generateTypeAwareProtocol = (stagingRoot) => {
  const manifest = JSON.parse(readFileSync(join(REPO_ROOT, TYPE_AWARE_MANIFEST_PATH), "utf8"));
  const requiredKeys = [
    "schema_version",
    "wire_protocol_version",
    "semantic_schema_version",
    "analysis_operation",
    "status_operation",
    "mutant_typecheck_operation",
    "query_operations",
    "session_envelope_types",
    "backend",
    "experimental_backends",
    "sidecar",
  ];
  if (
    manifest.schema_version !== 1 ||
    requiredKeys.some((key) => !(key in manifest)) ||
    !Number.isSafeInteger(manifest.wire_protocol_version) ||
    !Number.isSafeInteger(manifest.semantic_schema_version) ||
    !Array.isArray(manifest.query_operations) ||
    manifest.query_operations.length === 0 ||
    !Array.isArray(manifest.session_envelope_types) ||
    manifest.session_envelope_types.length === 0 ||
    !Array.isArray(manifest.experimental_backends) ||
    [manifest.backend, ...manifest.experimental_backends].some(
      (backend) =>
        typeof backend?.family !== "string" ||
        typeof backend.package !== "string" ||
        typeof backend.version !== "string",
    ) ||
    new Set([manifest.backend, ...manifest.experimental_backends].map((backend) => backend.family))
      .size !==
      manifest.experimental_backends.length + 1 ||
    manifest.sidecar?.version_source !== "workspace-package"
  ) {
    throw new Error("invalid type-aware protocol manifest");
  }
  const string = (value) => JSON.stringify(value);
  const stringArray = (values) => values.map(string).join(", ");
  writeStaged(
    stagingRoot,
    TYPE_AWARE_MODULE_PATH,
    `// Generated from crates/api/type-aware-protocol.json. Do not edit.\n\
export const TYPE_AWARE_PROTOCOL = Object.freeze({\n\
  schema_version: ${manifest.schema_version},\n\
  wire_protocol_version: ${manifest.wire_protocol_version},\n\
  semantic_schema_version: ${manifest.semantic_schema_version},\n\
  analysis_operation: ${string(manifest.analysis_operation)},\n\
  status_operation: ${string(manifest.status_operation)},\n\
  mutant_typecheck_operation: ${string(manifest.mutant_typecheck_operation)},\n\
  query_operations: [${stringArray(manifest.query_operations)}],\n\
  session_envelope_types: [${stringArray(manifest.session_envelope_types)}],\n\
  backend: {\n\
    family: ${string(manifest.backend.family)},\n\
    version: ${string(manifest.backend.version)},\n\
    package: ${string(manifest.backend.package)},\n\
  },\n\
  experimental_backends: [\n\
${manifest.experimental_backends
  .map(
    (backend) =>
      `    {\n      family: ${string(backend.family)},\n      package: ${string(backend.package)},\n      version: ${string(backend.version)},\n    },\n`,
  )
  .join("")}\
  ],\n\
  sidecar: {\n\
    package: ${string(manifest.sidecar.package)},\n\
    version_source: ${string(manifest.sidecar.version_source)},\n\
  },\n\
});\n\
export const WIRE_PROTOCOL_VERSION = TYPE_AWARE_PROTOCOL.wire_protocol_version;\n\
export const SEMANTIC_SCHEMA_VERSION = TYPE_AWARE_PROTOCOL.semantic_schema_version;\n\
export const ANALYSIS_OPERATION = TYPE_AWARE_PROTOCOL.analysis_operation;\n\
export const STATUS_OPERATION = TYPE_AWARE_PROTOCOL.status_operation;\n\
export const MUTANT_TYPECHECK_OPERATION = TYPE_AWARE_PROTOCOL.mutant_typecheck_operation;\n\
export const QUERY_OPERATIONS = Object.freeze(TYPE_AWARE_PROTOCOL.query_operations);\n\
export const SESSION_ENVELOPE_TYPES = Object.freeze(TYPE_AWARE_PROTOCOL.session_envelope_types);\n\
export const BACKEND_FAMILY = TYPE_AWARE_PROTOCOL.backend.family;\n\
export const BACKEND_VERSION = TYPE_AWARE_PROTOCOL.backend.version;\n\
export const BACKENDS = Object.freeze([\n\
  TYPE_AWARE_PROTOCOL.backend,\n\
  ...TYPE_AWARE_PROTOCOL.experimental_backends,\n\
]);\n\
export const SIDECAR_PACKAGE = TYPE_AWARE_PROTOCOL.sidecar.package;\n`,
  );
};

const MUTATION_RUNNER_LIST_KEYS = [
  "request_types",
  "response_types",
  "test_statuses",
  "run_statuses",
  "failure_kinds",
  "reload_methods",
  "error_codes",
  "capabilities",
  "isolation_modes",
  "in_band_markers",
];
const MUTATION_RUNNER_RUNTIME_KEYS = ["global_name", "active_function", "hit_function"];
const SEMVER_CORE = /^\d+\.\d+\.\d+$/u;
const ENV_NAME = /^[A-Z][A-Z0-9_]*$/u;

const isNonEmptyString = (value) => typeof value === "string" && value.length > 0;

const isNonEmptyStringList = (value) =>
  Array.isArray(value) && value.length > 0 && value.every(isNonEmptyString);

const hasStringFields = (record, keys) => keys.every((key) => typeof record?.[key] === "string");

const isValidMutationRunner = (runner) =>
  hasStringFields(runner, ["name", "package"]) &&
  [runner.min_version, runner.max_version_exclusive].every((version) => SEMVER_CORE.test(version));

/** Each check takes the manifest and is true when its part is valid. */
const MUTATION_RUNNER_MANIFEST_CHECKS = [
  (manifest) => manifest.schema_version === 1,
  (manifest) => Number.isSafeInteger(manifest.wire_protocol_version),
  (manifest) => Number.isSafeInteger(manifest.runtime_semantics_version),
  (manifest) => ENV_NAME.test(manifest.session_token_env ?? ""),
  (manifest) => MUTATION_RUNNER_LIST_KEYS.every((key) => isNonEmptyStringList(manifest[key])),
  (manifest) => hasStringFields(manifest.runtime, MUTATION_RUNNER_RUNTIME_KEYS),
  (manifest) =>
    Array.isArray(manifest.runners) &&
    manifest.runners.length > 0 &&
    manifest.runners.every(isValidMutationRunner),
];

export const assertMutationRunnerManifest = (manifest) => {
  if (!MUTATION_RUNNER_MANIFEST_CHECKS.every((check) => check(manifest))) {
    throw new Error("invalid mutation runner protocol manifest");
  }
};

const FORMAT_LINE_WIDTH = 100;

const isScalar = (value) => value === null || typeof value !== "object";

/** Gives a list of scalars on one line, or null when it is too long or nested. */
const inlineList = (list, linePrefix) => {
  const inline = `[${list.map((entry) => JSON.stringify(entry)).join(", ")}]`;
  const fits = linePrefix.length + inline.length + 1 <= FORMAT_LINE_WIDTH;
  return list.every(isScalar) && fits ? inline : null;
};

const blockLiteral = (open, lines, close, indent) =>
  `${open}\n${lines.join("\n")}\n${" ".repeat(indent)}${close}`;

const listLiteral = (list, indent) => {
  const inner = " ".repeat(indent + 2);
  const lines = list.map((entry) => `${inner}${jsLiteral(entry, indent + 2, inner)},`);
  return blockLiteral("[", lines, "]", indent);
};

const objectLiteral = (record, indent) => {
  const inner = " ".repeat(indent + 2);
  const lines = Object.entries(record).map(([key, entry]) => {
    const prefix = `${inner}${key}: `;
    return `${prefix}${jsLiteral(entry, indent + 2, prefix)},`;
  });
  return blockLiteral("{", lines, "}", indent);
};

/**
 * Prints a JSON value as a JavaScript literal in the layout that `oxfmt`
 * keeps: objects on several lines, and a list of scalars on one line when the
 * line fits.
 */
export const jsLiteral = (value, indent, linePrefix) => {
  if (isScalar(value)) return JSON.stringify(value);
  if (!Array.isArray(value)) return objectLiteral(value, indent);
  return inlineList(value, linePrefix) ?? listLiteral(value, indent);
};

/**
 * Writes the companion constants module. The module holds the same data as
 * the manifest.
 */
export const generateMutationRunnerProtocol = (stagingRoot) => {
  const manifest = JSON.parse(readFileSync(join(REPO_ROOT, MUTATION_RUNNER_MANIFEST_PATH), "utf8"));
  assertMutationRunnerManifest(manifest);
  writeStaged(
    stagingRoot,
    `${MUTATION_COMPANION_SOURCE_ROOT}/${MUTATION_RUNNER_PROTOCOL_FILE}`,
    `// Generated from ${MUTATION_RUNNER_MANIFEST_PATH}. Do not edit.\n\
const deepFreeze = (value) => {\n\
  if (value !== null && typeof value === "object") {\n\
    for (const entry of Object.values(value)) deepFreeze(entry);\n\
    Object.freeze(value);\n\
  }\n\
  return value;\n\
};\n\
\n\
export const MUTATION_RUNNER_PROTOCOL = deepFreeze(${jsLiteral(manifest, 0, "")});\n\
export const WIRE_PROTOCOL_VERSION = MUTATION_RUNNER_PROTOCOL.wire_protocol_version;\n\
export const SESSION_TOKEN_ENV = MUTATION_RUNNER_PROTOCOL.session_token_env;\n\
export const REQUEST_TYPES = MUTATION_RUNNER_PROTOCOL.request_types;\n\
export const ERROR_CODES = MUTATION_RUNNER_PROTOCOL.error_codes;\n\
export const GLOBAL_NAME = MUTATION_RUNNER_PROTOCOL.runtime.global_name;\n\
export const RUNNERS = MUTATION_RUNNER_PROTOCOL.runners;\n`,
  );
};

/**
 * Copies the companion runtime files into `fallow-api`. The copy of the
 * constants module comes from the staging root, so one generation run gives
 * the same text in both places.
 */
export const generateMutationCompanion = (stagingRoot) => {
  for (const file of MUTATION_COMPANION_FILES) {
    const sourcePath = `${MUTATION_COMPANION_SOURCE_ROOT}/${file}`;
    const sourceRoot = file === MUTATION_RUNNER_PROTOCOL_FILE ? stagingRoot : REPO_ROOT;
    const text = readFileSync(join(sourceRoot, sourcePath), "utf8");
    writeStaged(
      stagingRoot,
      `${MUTATION_COMPANION_TARGET_ROOT}/${file}`,
      `// Generated from ${sourcePath}. Do not edit.\n${text}`,
    );
  }
};

const generateAllPhases = (stagingRoot) => {
  generateSchemaFiles(stagingRoot);
  generateExtensionContracts(stagingRoot);
  generateAgentDocs(stagingRoot);
  generateNapiTypes(stagingRoot);
  generateTypeAwareProtocol(stagingRoot);
  generateMutationRunnerProtocol(stagingRoot);
  generateMutationCompanion(stagingRoot);
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

const validateStagedContracts = () => {
  checkContractSurfaceCoverage();
};

export const parseArgs = (argv) => {
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

  assertLocalResolution({
    dependency: "json-schema-to-typescript",
    resolveFrom: join(REPO_ROOT, EXTENSION_CODEGEN_PATH),
    installCommand: "pnpm --dir editors/vscode install",
  });

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
  if (!check) {
    for (const path of generateAgentAdapters()) {
      console.log(`regenerated: ${path}`);
    }
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
