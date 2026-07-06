#!/usr/bin/env node
import { readFileSync } from "node:fs";
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
  "fallow-api",
]);
const ANALYSIS_STARTERS = new Set(["fallow-core", "fallow-engine", "fallow-api"]);
const PROTOCOL_ADAPTERS = new Set(["fallow-cli", "fallow-lsp", "fallow-mcp", "fallow-node"]);

const boundaryRules = [
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
  const violations = findCrateBoundaryViolations(metadata);
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
