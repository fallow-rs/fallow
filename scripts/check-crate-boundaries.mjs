#!/usr/bin/env node
import { readdirSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

import { runCliMain } from "./cli-main.mjs";

const PROTOCOL_CRATES = new Set(["fallow-cli", "fallow-lsp", "fallow-mcp", "fallow-node"]);
const FOUNDATION_CRATES = new Set([
  "fallow-types",
  "fallow-config",
  "fallow-extract",
  "fallow-graph",
  "fallow-security",
  "fallow-core",
  "fallow-engine",
  "fallow-output",
  "fallow-process",
  "fallow-api",
]);
const ANALYSIS_STARTERS = new Set(["fallow-core", "fallow-engine", "fallow-api"]);
const PROTOCOL_ADAPTERS = new Set(["fallow-cli", "fallow-lsp", "fallow-mcp", "fallow-node"]);

const boundaryRules = [
  {
    rule: "process-must-remain-independent",
    matches: ({ from }) => from === "fallow-process",
    message: ({ to }) => `fallow-process must not depend on workspace crate ${to}`,
  },
  {
    rule: "foundation-must-not-depend-on-protocol",
    matches: ({ from, to }) => FOUNDATION_CRATES.has(from) && PROTOCOL_CRATES.has(to),
    message: ({ from, to }) => `${from} must not depend on protocol crate ${to}`,
  },
  {
    rule: "output-must-not-start-analysis",
    matches: ({ from, to }) => from === "fallow-output" && ANALYSIS_STARTERS.has(to),
    message: ({ to }) => `fallow-output must not depend on analysis starter ${to}`,
  },
  {
    rule: "protocol-must-use-api-or-engine",
    matches: ({ from, to }) => PROTOCOL_ADAPTERS.has(from) && to === "fallow-core",
    message: ({ from }) => `${from} must use fallow-api or fallow-engine instead of fallow-core`,
  },
];

export const workspaceDependencyEdges = (metadata) => {
  const workspaceIds = new Set(metadata.workspace_members ?? []);
  const packages = (metadata.packages ?? []).filter((pkg) => workspaceIds.has(pkg.id));
  const packageNames = new Set(packages.map((pkg) => pkg.name));
  return packages.flatMap((pkg) =>
    (pkg.dependencies ?? [])
      .filter((dep) => packageNames.has(dep.name))
      .map((dep) => ({
        from: pkg.name,
        to: dep.name,
      })),
  );
};

export const findCrateBoundaryViolations = (metadata) => {
  const edges = workspaceDependencyEdges(metadata);
  const violations = [];

  for (const edge of edges) {
    violations.push(
      ...boundaryRules
        .filter((rule) => rule.matches(edge))
        .map((rule) => ({
          rule: rule.rule,
          ...edge,
          message: rule.message(edge),
        })),
    );
  }

  return violations;
};

const SEMANTIC_OWNER_MARKERS = [
  "struct SemanticRequest",
  "struct SemanticResponse",
  "fn validate_response(",
  "fn apply_api_surface(",
  "fn run_semantic_request",
];
const PROCESS_TREE_OWNER_MARKERS = [
  "process_group(0)",
  "TerminateJobObject",
  "AssignProcessToJobObject",
  "CreateJobObjectW",
  "CREATE_SUSPENDED",
  "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
  "libc::waitid",
];

const PROTOCOL_ADAPTER_SOURCE_ROOTS = [
  ["fallow-cli", "crates/cli/src"],
  ["fallow-lsp", "crates/lsp/src"],
  ["fallow-mcp", "crates/mcp/src"],
  ["fallow-node", "crates/napi/src"],
];
const SOURCE_GATE_TEST_PATH = "crates/cli/src/architecture_boundaries.rs";

const rustFilesBelow = (directory) =>
  readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return rustFilesBelow(entryPath);
    return entry.isFile() && entry.name.endsWith(".rs") ? [entryPath] : [];
  });

export const findSemanticOwnershipViolations = (files) =>
  files.flatMap(({ path, source }) =>
    SEMANTIC_OWNER_MARKERS.filter((marker) => source.includes(marker)).map((marker) => ({
      rule: "semantic-client-owned-by-api",
      from:
        PROTOCOL_ADAPTER_SOURCE_ROOTS.find(([, root]) => path.startsWith(`${root}/`))?.[0] ??
        "protocol-adapter",
      to: "fallow-api",
      message: `${path} contains API-owned semantic client marker ${marker}`,
    })),
  );

export const findProcessTreeOwnershipViolations = (files) =>
  files.flatMap(({ path, source }) =>
    PROCESS_TREE_OWNER_MARKERS.filter((marker) => source.includes(marker)).map((marker) => ({
      rule: "process-tree-owned-by-process-crate",
      from:
        PROTOCOL_ADAPTER_SOURCE_ROOTS.find(([, root]) => path.startsWith(`${root}/`))?.[0] ??
        "protocol-adapter",
      to: "fallow-process",
      message: `${path} contains process-tree owner marker ${marker}`,
    })),
  );

const loadProtocolAdapterRustSources = () =>
  PROTOCOL_ADAPTER_SOURCE_ROOTS.flatMap(([, root]) =>
    rustFilesBelow(root)
      .filter((path) => path !== SOURCE_GATE_TEST_PATH)
      .map((path) => ({
        path,
        source: readFileSync(path, "utf8"),
      })),
  );

const metadataPathFromArgs = (args) => {
  const metadataIndex = args.indexOf("--metadata");
  if (metadataIndex === -1) {
    return null;
  }
  const path = args[metadataIndex + 1];
  if (!path) {
    throw new Error("--metadata requires a path");
  }
  return path;
};

const loadCargoMetadata = () => {
  const result = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || "cargo metadata failed");
  }
  return JSON.parse(result.stdout);
};

const loadMetadata = (args) => {
  const path = metadataPathFromArgs(args);
  return path ? JSON.parse(readFileSync(path, "utf8")) : loadCargoMetadata();
};

export const main = (args = process.argv.slice(2)) => {
  const metadata = loadMetadata(args);
  const adapterSources = loadProtocolAdapterRustSources();
  const violations = [
    ...findCrateBoundaryViolations(metadata),
    ...findSemanticOwnershipViolations(adapterSources),
    ...findProcessTreeOwnershipViolations(adapterSources),
  ];
  if (violations.length === 0) {
    console.log("crate boundary check passed");
    return 0;
  }

  for (const violation of violations) {
    console.error(`${violation.rule}: ${violation.message}`);
  }
  return 1;
};

if (import.meta.url === `file://${process.argv[1]}`) {
  runCliMain(main);
}
