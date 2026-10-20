import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { featureDifferences, parseFeatureTree } from "./check-cargo-features.mjs";

test("parseFeatureTree joins the features of repeated and path packages", () => {
  const tree = parseFeatureTree(
    [
      "serde v1.0.229 [derive,std]",
      "serde v1.0.229 [rc] (*)",
      "fallow-api v3.34.1 (/work/fallow/crates/api) [schema]",
      "syn v3.0.6 (proc-macro) []",
      "",
    ].join("\n"),
  );
  assert.deepEqual([...tree.get("serde v1.0.229")].toSorted(), ["derive", "rc", "std"]);
  assert.deepEqual([...tree.get("fallow-api v3.34.1")], ["schema"]);
  assert.deepEqual([...tree.get("syn v3.0.6")], []);
});

test("featureDifferences reports crates that both builds compile differently", () => {
  const packageTree = parseFeatureTree("serde v1 [derive]\nclap v4 [std]\n");
  const workspaceTree = parseFeatureTree("serde v1 [derive,rc]\nclap v4 [std]\ntokio v1 [rt]\n");
  assert.deepEqual(featureDifferences(packageTree, workspaceTree), [
    { crate: "serde v1", workspaceOnly: ["rc"], packageOnly: [] },
  ]);
});

test("featureDifferences is empty when the feature sets match", () => {
  const tree = parseFeatureTree("serde v1 [derive,rc]\n");
  assert.deepEqual(featureDifferences(tree, tree), []);
});

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "check-cargo-features.mjs");

test("the script reports a feature that only the workspace build enables", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "cargo-features-"));
  try {
    const stub = path.join(dir, "cargo");
    writeFileSync(
      stub,
      [
        "#!/bin/sh",
        'case "$*" in',
        '  *--workspace*) echo "serde v1.0.0 [derive,rc]" ;;',
        '  *) echo "serde v1.0.0 [derive]" ;;',
        "esac",
        "",
      ].join("\n"),
    );
    chmodSync(stub, 0o755);
    const result = spawnSync(process.execPath, [SCRIPT], {
      encoding: "utf8",
      env: { ...process.env, CARGO: stub },
    });
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /serde v1\.0\.0 \(workspace only: rc\)/u);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
