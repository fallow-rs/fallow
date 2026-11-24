// Tests for filter-sbom.mjs, the release step that removes the SBOM components
// that the release binary does not link (#5131).
//
// Run: node --test .github/scripts/filter-sbom.test.mjs

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { filterSbom, parseCargoTree } from "./filter-sbom.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./filter-sbom.mjs", import.meta.url));

const ref = (name, version) =>
  `registry+https://github.com/rust-lang/crates.io-index#${name}@${version}`;
const component = (name, version) => ({
  type: "library",
  "bom-ref": ref(name, version),
  name,
  version,
});

const makeSbom = () => ({
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  metadata: {
    component: { type: "application", "bom-ref": "root", name: "fallow-cli", version: "1.0.0" },
  },
  components: [
    component("serde", "1.0.0"),
    component("time", "0.3.55"),
    component("libc", "0.2.0"),
  ],
  dependencies: [
    { ref: "root", dependsOn: [ref("serde", "1.0.0"), ref("time", "0.3.55")] },
    { ref: ref("serde", "1.0.0"), dependsOn: [] },
    { ref: ref("time", "0.3.55"), dependsOn: [ref("libc", "0.2.0")] },
    { ref: ref("libc", "0.2.0"), dependsOn: [] },
  ],
});

const TREE = [
  "fallow-cli v1.0.0 (/work/crates/cli)",
  "serde v1.0.0",
  "libc v0.2.0",
  "serde_derive v1.0.0 (proc-macro)",
  "libc v0.2.0 (*)",
  "",
].join("\n");

test("parseCargoTree reads name and version and ignores markers", () => {
  assert.deepEqual([...parseCargoTree(TREE)].toSorted(), [
    "fallow-cli@1.0.0",
    "libc@0.2.0",
    "serde@1.0.0",
    "serde_derive@1.0.0",
  ]);
});

test("filterSbom removes a component that the tree does not list, and its dependency entries", () => {
  const tree = parseCargoTree(TREE.replace("serde_derive v1.0.0 (proc-macro)\n", ""));
  const result = filterSbom(makeSbom(), tree);

  assert.deepEqual(result.removed, ["time@0.3.55"]);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(
    result.sbom.components.map((c) => c.name),
    ["serde", "libc"],
  );
  assert.deepEqual(
    result.sbom.dependencies.map((d) => d.ref),
    ["root", ref("serde", "1.0.0"), ref("libc", "0.2.0")],
  );
  assert.deepEqual(result.sbom.dependencies[0].dependsOn, [ref("serde", "1.0.0")]);
});

test("filterSbom keeps a component of the same name with a version in the tree only", () => {
  const sbom = makeSbom();
  sbom.components.push(component("serde", "0.9.0"));
  const result = filterSbom(sbom, parseCargoTree("serde v1.0.0\nlibc v0.2.0\n"));
  assert.deepEqual(result.removed, ["serde@0.9.0", "time@0.3.55"]);
});

test("filterSbom reports a crate of the tree that has no component", () => {
  const result = filterSbom(makeSbom(), parseCargoTree(TREE));
  assert.deepEqual(result.missing, ["serde_derive@1.0.0"]);
});

test("filterSbom refuses an empty tree", () => {
  assert.throws(() => filterSbom(makeSbom(), new Set()), /lists no crates/);
});

const runCli = (sbom, tree) => {
  const dir = mkdtempSync(join(tmpdir(), "filter-sbom-"));
  try {
    const sbomPath = join(dir, "sbom.cdx.json");
    const treePath = join(dir, "tree.txt");
    const original = `${JSON.stringify(sbom)}\n`;
    writeFileSync(sbomPath, original);
    writeFileSync(treePath, tree);
    let status = 0;
    let stderr = "";
    try {
      execFileSync(process.execPath, [SCRIPT_PATH, sbomPath, treePath], { stdio: "pipe" });
    } catch (error) {
      status = error.status;
      stderr = error.stderr.toString();
    }
    return { status, stderr, original, written: readFileSync(sbomPath, "utf8") };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

test("the CLI rewrites the SBOM in place", () => {
  const { status, written } = runCli(makeSbom(), "fallow-cli v1.0.0\nserde v1.0.0\nlibc v0.2.0\n");
  assert.equal(status, 0);
  assert.deepEqual(
    JSON.parse(written).components.map((c) => c.name),
    ["serde", "libc"],
  );
});

test("the CLI fails and leaves the SBOM untouched when a crate has no component", () => {
  const { status, stderr, original, written } = runCli(makeSbom(), TREE);
  assert.equal(status, 1);
  assert.match(stderr, /serde_derive@1\.0\.0/);
  assert.equal(written, original);
});
