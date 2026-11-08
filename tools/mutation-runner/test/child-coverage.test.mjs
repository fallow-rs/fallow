import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { childHitsPath } from "../src/runtime/child.mjs";
import { createChildCoverage } from "../src/runtime/child-coverage.mjs";

const writeChildReport = (overlayPath, token, name, hits) => {
  const dir = childHitsPath(overlayPath);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${token}.${name}.json`), JSON.stringify({ hits, loaded: [] }));
};

test("each concurrent test gets the child hits of the shared token", (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-child-coverage-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const overlayPath = join(root, "overlay");
  const coverage = createChildCoverage(overlayPath);

  const token = coverage.startTest();
  assert.equal(coverage.startTest(), token);
  writeChildReport(overlayPath, token, "a", ["m1"]);

  assert.deepEqual(coverage.finishTest().hits, ["m1"]);
  writeChildReport(overlayPath, token, "b", ["m2"]);
  assert.deepEqual(coverage.finishTest().hits.toSorted(), ["m1", "m2"]);
});

test("a later test does not get the child hits of an earlier test", (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-child-coverage-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const overlayPath = join(root, "overlay");
  const coverage = createChildCoverage(overlayPath);

  const first = coverage.startTest();
  writeChildReport(overlayPath, first, "a", ["m1"]);
  assert.deepEqual(coverage.finishTest().hits, ["m1"]);

  const second = coverage.startTest();
  assert.notEqual(second, first);
  assert.deepEqual(coverage.finishTest().hits, []);
});
