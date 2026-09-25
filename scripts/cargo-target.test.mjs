import assert from "node:assert/strict";
import { resolve } from "node:path";
import { test } from "node:test";

import { cargoFallowBin, cargoTargetDir } from "./cargo-target.mjs";

const root = resolve("/repo");

test("the target directory falls back to <root>/target", () => {
  assert.equal(cargoTargetDir(root, {}), resolve(root, "target"));
  assert.equal(cargoTargetDir(root, { CARGO_TARGET_DIR: "" }), resolve(root, "target"));
});

test("an absolute CARGO_TARGET_DIR is used as it is", () => {
  const dir = resolve("/tmp/private-target");
  assert.equal(cargoTargetDir(root, { CARGO_TARGET_DIR: dir }), dir);
  assert.equal(
    cargoFallowBin(root, "release", { CARGO_TARGET_DIR: dir }),
    resolve(dir, "release/fallow"),
  );
});

test("a relative CARGO_TARGET_DIR resolves against the repository root", () => {
  assert.equal(
    cargoFallowBin(root, "debug", { CARGO_TARGET_DIR: "build/cargo" }),
    resolve(root, "build/cargo/debug/fallow"),
  );
});
