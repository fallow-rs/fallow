// Tests for filter-sbom.mjs, the release step that merges the SBOMs of the
// shipped packages and removes the components that the release binaries do
// not link (#5131, #5190).
//
// Run: node --test .github/scripts/filter-sbom.test.mjs

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  danglingRefs,
  dropUnshippedBins,
  filterSbom,
  mergeSboms,
  parseCargoTree,
} from "./filter-sbom.mjs";

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

const TARGET = "x86_64-unknown-linux-gnu";

const pathRef = (name) => `path+file:///work/${name}#${name}@1.0.0`;

const packageSbom = (name, bins, deps, target = TARGET) => ({
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  version: 1,
  metadata: {
    properties: [{ name: "cdx:rustc:sbom:target:triple", value: target }],
    component: {
      type: "application",
      "bom-ref": pathRef(name),
      name,
      version: "1.0.0",
      components: [
        {
          type: "library",
          "bom-ref": `${pathRef(name)} bin-target-0`,
          name: name.replaceAll("-", "_"),
        },
        ...bins.map((bin, index) => ({
          type: "application",
          "bom-ref": `${pathRef(name)} bin-target-${index + 1}`,
          name: bin,
        })),
      ],
    },
  },
  components: deps.map(([dep, version]) => component(dep, version)),
  dependencies: [
    { ref: pathRef(name), dependsOn: deps.map(([dep, version]) => ref(dep, version)) },
    ...deps.map(([dep, version]) => ({ ref: ref(dep, version), dependsOn: [] })),
  ],
});

const cliSbom = () =>
  packageSbom(
    "fallow-cli",
    ["fallow", "stub_sidecar"],
    [
      ["serde", "1.0.0"],
      ["time", "0.3.55"],
    ],
  );
const lspSbom = () =>
  packageSbom(
    "fallow-lsp",
    ["fallow-lsp"],
    [
      ["serde", "1.0.0"],
      ["tower-lsp-server", "0.22.0"],
    ],
  );

test("mergeSboms gives one release root with each package as a component", () => {
  const merged = mergeSboms([cliSbom(), lspSbom()]);

  assert.equal(merged.metadata.component.name, "fallow");
  assert.equal(merged.metadata.component["bom-ref"], "fallow@1.0.0");
  assert.equal(merged.metadata.properties[0].value, TARGET);
  assert.deepEqual(
    merged.components.map((c) => c.name),
    ["fallow-cli", "fallow-lsp", "serde", "time", "tower-lsp-server"],
  );
  assert.deepEqual(merged.dependencies[0], {
    ref: "fallow@1.0.0",
    dependsOn: [pathRef("fallow-cli"), pathRef("fallow-lsp")],
  });
  assert.deepEqual(danglingRefs(merged), []);
});

test("mergeSboms joins the dependency entries of a shared crate", () => {
  const cli = cliSbom();
  const lsp = lspSbom();
  cli.dependencies.find((d) => d.ref === ref("serde", "1.0.0")).dependsOn = [ref("time", "0.3.55")];
  lsp.dependencies.find((d) => d.ref === ref("serde", "1.0.0")).dependsOn = [
    ref("tower-lsp-server", "0.22.0"),
  ];
  const merged = mergeSboms([cli, lsp]);

  const serde = merged.dependencies.filter((d) => d.ref === ref("serde", "1.0.0"));
  assert.equal(serde.length, 1);
  assert.deepEqual(serde[0].dependsOn, [ref("time", "0.3.55"), ref("tower-lsp-server", "0.22.0")]);
});

test("mergeSboms keeps the package root when another SBOM lists it as a library", () => {
  const lsp = lspSbom();
  lsp.components.push({
    type: "library",
    "bom-ref": pathRef("fallow-cli"),
    name: "fallow-cli",
    version: "1.0.0",
  });
  const merged = mergeSboms([cliSbom(), lsp]);

  const cli = merged.components.filter((c) => c["bom-ref"] === pathRef("fallow-cli"));
  assert.equal(cli.length, 1);
  assert.equal(cli[0].type, "application");
  assert.ok(Array.isArray(cli[0].components));
});

test("mergeSboms refuses SBOMs of different targets", () => {
  const lsp = packageSbom("fallow-lsp", ["fallow-lsp"], [], "aarch64-apple-darwin");
  assert.throws(() => mergeSboms([cliSbom(), lsp]), /another target/);
});

test("dropUnshippedBins removes bins that do not ship and keeps library targets", () => {
  const result = dropUnshippedBins(
    mergeSboms([cliSbom(), lspSbom()]),
    new Set(["fallow", "fallow-lsp"]),
  );

  assert.deepEqual(result.removed, ["stub_sidecar"]);
  assert.deepEqual(
    result.sbom.components[0].components.map((c) => c.name),
    ["fallow_cli", "fallow"],
  );
});

test("dropUnshippedBins fails when a shipped bin is in no package", () => {
  assert.throws(
    () => dropUnshippedBins(mergeSboms([cliSbom()]), new Set(["fallow", "fallow-mcp"])),
    /fallow-mcp/,
  );
});

test("danglingRefs reports a dependency ref that names no component", () => {
  const merged = mergeSboms([cliSbom()]);
  merged.dependencies.push({ ref: ref("libc", "0.2.0"), dependsOn: [ref("gone", "1.0.0")] });
  assert.deepEqual(danglingRefs(merged), [ref("gone", "1.0.0"), ref("libc", "0.2.0")]);
});

const SHIPPED_BINS = ["--bin", "fallow", "--bin", "fallow-lsp"];

const runCli = (sboms, trees, extraArgs = SHIPPED_BINS) => {
  const dir = mkdtempSync(join(tmpdir(), "filter-sbom-"));
  try {
    const outPath = join(dir, "out.cdx.json");
    const args = ["--out", outPath, ...extraArgs];
    trees.forEach((tree, index) => {
      const path = join(dir, `tree-${index}.txt`);
      writeFileSync(path, tree);
      args.push("--tree", path);
    });
    sboms.forEach((sbom, index) => {
      const path = join(dir, `sbom-${index}.cdx.json`);
      writeFileSync(path, `${JSON.stringify(sbom)}\n`);
      args.push(path);
    });
    let status = 0;
    let stderr = "";
    try {
      execFileSync(process.execPath, [SCRIPT_PATH, ...args], { stdio: "pipe" });
    } catch (error) {
      status = error.status;
      stderr = error.stderr.toString();
    }
    const written = existsSync(outPath) ? JSON.parse(readFileSync(outPath, "utf8")) : undefined;
    return { status, stderr, written };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

const CLI_TREE = "fallow-cli v1.0.0 (/work/fallow-cli)\nserde v1.0.0\n";
const LSP_TREE =
  "fallow-lsp v1.0.0 (/work/fallow-lsp)\nserde v1.0.0 (*)\ntower-lsp-server v0.22.0\n";

test("the CLI merges the SBOMs and keeps the crates of every tree", () => {
  const { status, stderr, written } = runCli([cliSbom(), lspSbom()], [CLI_TREE, LSP_TREE]);
  assert.equal(status, 0, stderr);
  assert.deepEqual(
    written.components.map((c) => c.name),
    ["fallow-cli", "fallow-lsp", "serde", "tower-lsp-server"],
  );
  assert.deepEqual(
    written.components.flatMap((c) => (c.components ?? []).map((n) => n.name)),
    ["fallow_cli", "fallow", "fallow_lsp", "fallow-lsp"],
  );
  assert.deepEqual(danglingRefs(written), []);
});

test("the CLI fails and writes nothing when a crate of a tree has no component", () => {
  const { status, stderr, written } = runCli(
    [cliSbom(), lspSbom()],
    [CLI_TREE, `${LSP_TREE}rquickjs v0.9.0\n`],
  );
  assert.equal(status, 1);
  assert.match(stderr, /rquickjs@0\.9\.0/);
  assert.equal(written, undefined);
});

test("the CLI fails and writes nothing when a dependency ref names no component", () => {
  const lsp = lspSbom();
  lsp.dependencies.push({ ref: ref("ghost", "1.0.0"), dependsOn: [] });
  const { status, stderr, written } = runCli([cliSbom(), lsp], [CLI_TREE, LSP_TREE]);
  assert.equal(status, 1);
  assert.match(stderr, /ghost@1\.0\.0/);
  assert.equal(written, undefined);
});

test("the CLI fails and writes nothing when a tree is empty", () => {
  const { status, stderr, written } = runCli([cliSbom(), lspSbom()], [CLI_TREE, ""]);
  assert.equal(status, 1);
  assert.match(stderr, /lists no crates/);
  assert.equal(written, undefined);
});

test("the CLI fails without a --bin argument", () => {
  const { status, stderr } = runCli([cliSbom()], [CLI_TREE], []);
  assert.equal(status, 1);
  assert.match(stderr, /usage/);
});
