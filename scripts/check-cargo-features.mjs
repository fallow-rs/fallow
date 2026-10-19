#!/usr/bin/env node
// Cargo resolves features per build. When another workspace member enables a
// feature on a crate that fallow-cli also uses, `cargo build -p fallow-cli`
// and `cargo build --workspace` compile two variants of that crate and of all
// its dependents. After each edit, a switch between the two commands then
// compiles the fallow crates again. This check fails when the feature sets
// differ, so the feature-only pins in Cargo.toml and crates/cli/Cargo.toml
// stay complete.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const PACKAGE = "fallow-cli";
const PACKAGE_LINE = /^(\S+) (v\S+)/;

/**
 * Parses `cargo tree --prefix none -f "{p} [{f}]"` output into a map from
 * `name version` to the union of its enabled features.
 */
export const parseFeatureTree = (text) => {
  const features = new Map();
  for (const line of text.split("\n")) {
    const match = PACKAGE_LINE.exec(line);
    const open = line.lastIndexOf("[");
    const close = line.lastIndexOf("]");
    if (match === null || open === -1 || close < open) continue;
    const key = `${match[1]} ${match[2]}`;
    const set = features.get(key) ?? new Set();
    for (const feature of line.slice(open + 1, close).split(",")) {
      if (feature !== "") set.add(feature);
    }
    features.set(key, set);
  }
  return features;
};

/**
 * Returns one entry for each crate that both builds compile with different
 * features. Crates that only the workspace build compiles are not a problem.
 */
export const featureDifferences = (packageTree, workspaceTree) => {
  const differences = [];
  for (const [key, packageFeatures] of packageTree) {
    const workspaceFeatures = workspaceTree.get(key);
    if (workspaceFeatures === undefined) continue;
    const workspaceOnly = [...workspaceFeatures].filter((f) => !packageFeatures.has(f)).toSorted();
    const packageOnly = [...packageFeatures].filter((f) => !workspaceFeatures.has(f)).toSorted();
    if (workspaceOnly.length > 0 || packageOnly.length > 0) {
      differences.push({ crate: key, workspaceOnly, packageOnly });
    }
  }
  return differences.toSorted((a, b) => a.crate.localeCompare(b.crate));
};

const cargoTree = (scope) =>
  execFileSync(
    process.env.CARGO ?? "cargo",
    ["tree", ...scope, "--edges", "normal,build", "--prefix", "none", "--format", "{p} [{f}]"],
    {
      cwd: ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
      maxBuffer: 64 * 1024 * 1024,
    },
  );

const main = () => {
  const differences = featureDifferences(
    parseFeatureTree(cargoTree(["--package", PACKAGE])),
    parseFeatureTree(cargoTree(["--workspace"])),
  );
  if (differences.length === 0) {
    console.log(`${PACKAGE} and the workspace build resolve the same crate features.`);
    return 0;
  }
  console.error(
    `\`cargo build -p ${PACKAGE}\` and \`cargo build --workspace\` resolve different features:`,
  );
  for (const { crate, workspaceOnly, packageOnly } of differences) {
    const parts = [];
    if (workspaceOnly.length > 0) parts.push(`workspace only: ${workspaceOnly.join(", ")}`);
    if (packageOnly.length > 0) parts.push(`${PACKAGE} only: ${packageOnly.join(", ")}`);
    console.error(`  - ${crate} (${parts.join("; ")})`);
  }
  console.error(
    "Enable the workspace-only features in [workspace.dependencies] or as feature-only " +
      "dependencies in crates/cli/Cargo.toml.",
  );
  return 1;
};

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
