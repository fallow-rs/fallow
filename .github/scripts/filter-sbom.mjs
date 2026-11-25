// Release step: merge the CycloneDX SBOMs of the packages that one release
// archive ships, and keep only the crates of those builds (#5131, #5190).
//
// The `fallow-<target>` archive ships the binaries of several packages:
// `fallow-cli`, `fallow-lsp`, `fallow-mcp` and the similar-code sidecar, which
// is a separate Cargo workspace. `cargo cyclonedx` writes one SBOM per package.
// This script merges them into one SBOM with one root component for the
// release. Each shipped package is a component of that SBOM.
//
// `cargo cyclonedx --target` resolves features over the whole workspace, so
// its SBOM also lists crates that only other workspace members enable. A
// consumer that scans the SBOM then gets advisories for crates that are not in
// the build. `cargo tree --target <target> -e normal,build` gives the crates of
// the build, including proc-macros and build-dependencies. This script keeps
// only the components that a tree lists, and it fails when a crate of a tree
// has no component.
//
// A package SBOM also lists each bin target of the package as a nested
// component. This script removes the bin components that the archive does not
// ship, for example bins that need a feature that the release does not enable.
//
// Usage:
//   node filter-sbom.mjs --out <merged.cdx.json> --tree <cargo-tree.txt>...
//     --bin <shipped-bin>... <package.cdx.json>...
//
// Each <cargo-tree.txt> is the output of
//   cargo tree <packages> --target <target> -e normal,build --prefix none --format '{p}' --locked
// The script writes <merged.cdx.json>. It exits 1, and writes nothing, when an
// input is not valid, when a crate of a tree has no component, when a shipped
// bin is in no package, or when a dependency entry names an unknown component.

import { randomUUID } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

// A `cargo tree --format '{p}'` line is `<name> v<version>`, optionally
// followed by a source path, `(proc-macro)` or `(*)`.
const TREE_LINE_PATTERN = /^(\S+) v(\S+)/;

/** The name of the root component that describes the release. */
export const RELEASE_NAME = "fallow";

const TARGET_PROPERTY = "cdx:rustc:sbom:target:triple";

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

const targetOf = (sbom) =>
  sbom.metadata?.properties?.find((property) => property.name === TARGET_PROPERTY)?.value;

const unionDependencies = (sboms) => {
  const byRef = new Map();
  for (const sbom of sboms) {
    for (const entry of sbom.dependencies ?? []) {
      const dependsOn = byRef.get(entry.ref) ?? new Set();
      for (const ref of entry.dependsOn ?? []) dependsOn.add(ref);
      byRef.set(entry.ref, dependsOn);
    }
  }
  return [...byRef].map(([ref, dependsOn]) => ({ ref, dependsOn: [...dependsOn] }));
};

const assertMergeable = (sboms) => {
  if (sboms.length === 0) throw new Error("no SBOM to merge");
  const [first] = sboms;
  for (const sbom of sboms) {
    const root = sbom.metadata?.component;
    if (!root?.["bom-ref"]) throw new Error("an SBOM has no root component");
    if (!Array.isArray(sbom.components)) {
      throw new Error(`the SBOM of ${root.name} has no components array`);
    }
    if (sbom.specVersion !== first.specVersion) {
      throw new Error(`the SBOM of ${root.name} has another specVersion`);
    }
    if (targetOf(sbom) !== targetOf(first)) {
      throw new Error(`the SBOM of ${root.name} is for another target`);
    }
  }
};

const unionComponents = (sboms, packages) => {
  const components = new Map(packages.map((root) => [root["bom-ref"], root]));
  for (const sbom of sboms) {
    for (const component of sbom.components) {
      if (!components.has(component["bom-ref"])) components.set(component["bom-ref"], component);
    }
  }
  return [...components.values()];
};

/**
 * Merges the SBOMs of the shipped packages into one SBOM. The root component
 * describes the release, and each package root becomes a top-level component.
 * The components are deduplicated by `bom-ref`, and a package root wins over a
 * library component with the same `bom-ref`. Pure: it returns a new SBOM.
 */
export const mergeSboms = (sboms) => {
  assertMergeable(sboms);
  const [first] = sboms;
  const packages = sboms.map((sbom) => sbom.metadata.component);
  const version = packages[0].version;
  const rootRef = keyOf(RELEASE_NAME, version);
  return {
    bomFormat: first.bomFormat,
    specVersion: first.specVersion,
    serialNumber: `urn:uuid:${randomUUID()}`,
    version: 1,
    metadata: {
      ...first.metadata,
      component: {
        type: "application",
        "bom-ref": rootRef,
        name: RELEASE_NAME,
        version,
        description: `The fallow ${version} release archive: ${packages.map((p) => p.name).join(", ")}`,
      },
    },
    components: unionComponents(sboms, packages),
    dependencies: [
      { ref: rootRef, dependsOn: packages.map((root) => root["bom-ref"]) },
      ...unionDependencies(sboms),
    ],
  };
};

/**
 * Removes the nested bin components that the archive does not ship. A nested
 * component of type `application` is a bin target; a library target stays.
 * Throws when a shipped bin is in no component. Pure: it returns a new SBOM.
 */
export const dropUnshippedBins = (sbom, shippedBins) => {
  const found = new Set();
  const removed = [];
  const components = sbom.components.map((component) => {
    if (!Array.isArray(component.components)) return component;
    const nested = component.components.filter((child) => {
      if (child.type !== "application") return true;
      if (shippedBins.has(child.name)) {
        found.add(child.name);
        return true;
      }
      removed.push(child.name);
      return false;
    });
    return { ...component, components: nested };
  });
  const absent = [...shippedBins].filter((name) => !found.has(name)).toSorted();
  if (absent.length > 0) {
    throw new Error(`no package of the SBOM has these shipped bins: ${absent.join(", ")}`);
  }
  return { sbom: { ...sbom, components }, removed: removed.toSorted() };
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

/** Returns the dependency refs that name no component of the SBOM. */
export const danglingRefs = (sbom) => {
  const known = new Set([sbom.metadata?.component?.["bom-ref"]]);
  const visit = (component) => {
    known.add(component["bom-ref"]);
    for (const child of component.components ?? []) visit(child);
  };
  for (const component of sbom.components) visit(component);
  const dangling = new Set();
  for (const entry of sbom.dependencies ?? []) {
    for (const ref of [entry.ref, ...(entry.dependsOn ?? [])]) {
      if (!known.has(ref)) dangling.add(ref);
    }
  }
  return [...dangling].toSorted();
};

const USAGE =
  "usage: node filter-sbom.mjs --out <merged.cdx.json> --tree <cargo-tree.txt>... --bin <name>... <package.cdx.json>...";

const parseOptions = (args) => {
  const { values, positionals } = parseArgs({
    args,
    allowPositionals: true,
    options: {
      out: { type: "string" },
      tree: { type: "string", multiple: true, default: [] },
      bin: { type: "string", multiple: true, default: [] },
    },
  });
  const complete =
    values.out && values.tree.length > 0 && values.bin.length > 0 && positionals.length > 0;
  return complete ? { ...values, sbomPaths: positionals } : undefined;
};

const readTreeKeys = (paths) => {
  const treeKeys = new Set();
  for (const path of paths) {
    const keys = parseCargoTree(readFileSync(path, "utf8"));
    if (keys.size === 0) throw new Error(`the cargo tree output ${path} lists no crates`);
    for (const key of keys) treeKeys.add(key);
  }
  return treeKeys;
};

const printList = (print, heading, items) => {
  print(heading);
  for (const item of items) print(`- ${item}`);
};

const main = (args) => {
  const options = parseOptions(args);
  if (!options) {
    console.error(USAGE);
    return 1;
  }

  const sboms = options.sbomPaths.map((path) => JSON.parse(readFileSync(path, "utf8")));
  const bins = dropUnshippedBins(mergeSboms(sboms), new Set(options.bin));
  const result = filterSbom(bins.sbom, readTreeKeys(options.tree));
  if (result.missing.length > 0) {
    const heading = "The SBOM has no component for these crates of the cargo tree:";
    printList(console.error, heading, result.missing);
    return 1;
  }
  const dangling = danglingRefs(result.sbom);
  if (dangling.length > 0) {
    printList(console.error, "These dependency refs name no component of the SBOM:", dangling);
    return 1;
  }

  writeFileSync(options.out, `${JSON.stringify(result.sbom, null, 2)}\n`);
  console.log(`Merged ${sboms.length} SBOMs into ${result.sbom.components.length} components.`);
  const binHeading = `Removed ${bins.removed.length} bins that the archive does not ship.`;
  printList(console.log, binHeading, bins.removed);
  const crateHeading = `Removed ${result.removed.length} components that are not in the build.`;
  printList(console.log, crateHeading, result.removed);
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
