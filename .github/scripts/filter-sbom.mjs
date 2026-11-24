// Release step: remove the components of a CycloneDX SBOM that are not in the
// fallow-cli build of the target (#5131).
//
// `cargo cyclonedx --target` resolves features over the whole workspace, so its
// SBOM also lists crates that only other workspace members enable. A consumer
// that scans the SBOM then gets advisories for crates that are not in the
// build. `cargo tree -p fallow-cli --target <target> -e normal,build` gives
// the crates of the build, including proc-macros and build-dependencies. This
// script keeps only those components, and it fails when a crate of the tree
// has no component.
//
// Usage:
//   node filter-sbom.mjs <sbom.cdx.json> <cargo-tree.txt>
//
// <cargo-tree.txt> is the output of
//   cargo tree -p fallow-cli --target <target> -e normal,build --prefix none --format '{p}' --locked
// The script rewrites the SBOM in place. It exits 1, with the SBOM untouched,
// when the tree is empty or when a crate of the tree has no component.

import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

// A `cargo tree --format '{p}'` line is `<name> v<version>`, optionally
// followed by a source path, `(proc-macro)` or `(*)`.
const TREE_LINE_PATTERN = /^(\S+) v(\S+)/;

const keyOf = (name, version) => `${name}@${version}`;

/** Returns the `name@version` keys of the crates in a `cargo tree` output. */
export const parseCargoTree = (text) => {
  const keys = new Set();
  for (const line of text.split("\n")) {
    const match = TREE_LINE_PATTERN.exec(line.trim());
    if (match) keys.add(keyOf(match[1], match[2]));
  }
  return keys;
};

/**
 * Keeps only the SBOM components whose crate is in `treeKeys`, and removes the
 * dependency entries of the removed components. Pure: it returns a new SBOM.
 */
export const filterSbom = (sbom, treeKeys) => {
  if (treeKeys.size === 0) {
    throw new Error("the cargo tree output lists no crates");
  }
  if (!Array.isArray(sbom.components)) {
    throw new Error("the SBOM has no components array");
  }

  const root = sbom.metadata?.component;
  const rootKey = root ? keyOf(root.name, root.version) : undefined;
  const kept = [];
  const removed = [];
  for (const component of sbom.components) {
    if (treeKeys.has(keyOf(component.name, component.version))) kept.push(component);
    else removed.push(component);
  }

  const componentKeys = new Set(kept.map((c) => keyOf(c.name, c.version)));
  const missing = [...treeKeys]
    .filter((key) => key !== rootKey && !componentKeys.has(key))
    .toSorted();

  const removedRefs = new Set(removed.map((c) => c["bom-ref"]));
  const dependencies = Array.isArray(sbom.dependencies)
    ? sbom.dependencies
        .filter((entry) => !removedRefs.has(entry.ref))
        .map((entry) => ({
          ...entry,
          ...(Array.isArray(entry.dependsOn)
            ? { dependsOn: entry.dependsOn.filter((ref) => !removedRefs.has(ref)) }
            : {}),
        }))
    : sbom.dependencies;

  return {
    sbom: { ...sbom, components: kept, ...(dependencies ? { dependencies } : {}) },
    removed: removed.map((c) => keyOf(c.name, c.version)).toSorted(),
    missing,
  };
};

const main = ([sbomPath, treePath]) => {
  if (!sbomPath || !treePath) {
    console.error("usage: node filter-sbom.mjs <sbom.cdx.json> <cargo-tree.txt>");
    return 1;
  }
  const sbom = JSON.parse(readFileSync(sbomPath, "utf8"));
  const treeKeys = parseCargoTree(readFileSync(treePath, "utf8"));
  const result = filterSbom(sbom, treeKeys);
  if (result.missing.length > 0) {
    console.error(`The SBOM has no component for these crates of the cargo tree:`);
    for (const key of result.missing) console.error(`- ${key}`);
    return 1;
  }
  writeFileSync(sbomPath, `${JSON.stringify(result.sbom, null, 2)}\n`);
  console.log(`Removed ${result.removed.length} components that are not in the fallow-cli build.`);
  for (const key of result.removed) console.log(`- ${key}`);
  return 0;
};

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
