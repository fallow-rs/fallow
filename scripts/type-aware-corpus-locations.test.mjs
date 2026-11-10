import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";

import { contractDeclarationPath, resolveEvidencePath } from "./type-aware-corpus/locations.mjs";

const WORKSPACE = resolve("/workspace");
const FIXTURE = resolve(WORKSPACE, "target/type-aware-corpus/fixtures/next.js");

test("a contract declaration in the fixture keeps its fixture-relative path", () => {
  assert.equal(
    contractDeclarationPath(FIXTURE, WORKSPACE, "packages/next/src/server.ts"),
    "packages/next/src/server.ts",
  );
});

test("a contract declaration in a workspace type package is recorded below node_modules", () => {
  assert.equal(
    contractDeclarationPath(FIXTURE, WORKSPACE, "../../../../node_modules/@types/node/http.d.ts"),
    "node_modules/@types/node/http.d.ts",
  );
  assert.equal(
    resolveEvidencePath(FIXTURE, "node_modules/@types/node/http.d.ts", WORKSPACE),
    resolve(WORKSPACE, "node_modules/@types/node/http.d.ts"),
  );
});

test("a contract declaration outside the fixture and the dependency root is rejected", () => {
  assert.equal(contractDeclarationPath(FIXTURE, WORKSPACE, "../../../../scripts/a.d.ts"), null);
  assert.equal(contractDeclarationPath(FIXTURE, WORKSPACE, "/elsewhere/a.d.ts"), null);
  assert.equal(resolveEvidencePath(FIXTURE, "node_modules/../scripts/a.d.ts", WORKSPACE), null);
});

test("without a dependency root an evidence path must stay in the fixture", () => {
  assert.equal(resolveEvidencePath(FIXTURE, "../../../../node_modules/x.d.ts"), null);
  assert.equal(
    resolveEvidencePath(FIXTURE, "node_modules/x.d.ts"),
    resolve(FIXTURE, "node_modules/x.d.ts"),
  );
});
