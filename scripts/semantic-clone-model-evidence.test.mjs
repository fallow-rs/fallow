import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";

import {
  compareEvidence,
  formatComparison,
  parseArgs,
  readComparisonBaseline,
} from "./semantic-clone-model-evidence.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const LOCKED_EVIDENCE = resolve(
  REPO_ROOT,
  "tests/semantic-clone-corpus/evidence/jina-v2-code-q8.json",
);

const REQUIRED_ARGS = [
  "--runtime-lock",
  "/tmp/package-lock.json",
  "--transformers-module",
  "/tmp/transformers.node.mjs",
];

const evidence = (runtimeVersion, profiles) => ({
  provider: { runtime_version: runtimeVersion },
  profiles: profiles.map(([id, threshold, cases]) => ({
    id,
    threshold,
    cases: Object.entries(cases).map(([caseId, similarity]) => ({ id: caseId, similarity })),
  })),
});

test("model evidence accepts an optional comparison baseline", () => {
  assert.equal(parseArgs(REQUIRED_ARGS).compare, null);
  assert.equal(
    parseArgs([...REQUIRED_ARGS, "--compare", "/tmp/old.json"]).compare,
    "/tmp/old.json",
  );
  assert.throws(() => parseArgs([...REQUIRED_ARGS, "--compare"]), /--compare requires/);
  for (const inherited of ["constructor", "toString", "__proto__"]) {
    assert.throws(() => parseArgs([...REQUIRED_ARGS, inherited, "value"]), /unknown argument/);
  }
});

test("comparison of the locked evidence with itself reports no change", () => {
  const locked = JSON.parse(readFileSync(LOCKED_EVIDENCE, "utf8"));
  const comparison = compareEvidence(locked, locked);

  assert.deepEqual(
    comparison.profiles.map((entry) => entry.id),
    locked.profiles.map((entry) => entry.id),
  );
  for (const entry of comparison.profiles) {
    assert.equal(entry.max_abs_delta, 0);
    assert.deepEqual(entry.threshold_crossings, []);
    assert.equal(entry.order_changed, false);
    assert.ok(entry.cases.every((testCase) => testCase.delta === 0));
  }
  const summary = formatComparison(comparison);
  assert.match(summary, /case order: unchanged/);
  assert.match(summary, /threshold crossings: none/);
  assert.doesNotMatch(summary, /\+0\.0*[1-9]|-0\.0*[1-9]/);
});

test("comparison reports deltas, threshold crossings, and order changes", () => {
  const previous = evidence("4.2.0", [
    ["native-768", 0.8, { alpha: 0.95, beta: 0.81, gamma: 0.79 }],
    ["naive-truncation-256", 0.8, { alpha: 0.9, beta: 0.7, gamma: 0.6 }],
    ["removed-profile", 0.8, { alpha: 0.5 }],
  ]);
  const current = evidence("4.3.1", [
    ["native-768", 0.8, { alpha: 0.95, beta: 0.799999, gamma: 0.8, delta: 0.4 }],
    ["naive-truncation-256", 0.8, { alpha: 0.900001, beta: 0.7 }],
  ]);

  const comparison = compareEvidence(previous, current);
  const [native, truncated] = comparison.profiles;

  assert.deepEqual(
    native.cases.map((testCase) => [testCase.id, testCase.delta]),
    [
      ["alpha", 0],
      ["beta", -0.010001],
      ["gamma", 0.01],
    ],
  );
  assert.equal(native.max_abs_delta, 0.010001);
  assert.deepEqual(
    native.threshold_crossings.map((testCase) => [
      testCase.id,
      testCase.previous_selected,
      testCase.current_selected,
    ]),
    [
      ["beta", true, false],
      ["gamma", false, true],
    ],
  );
  assert.equal(native.order_changed, true);
  assert.deepEqual(native.previous_order, ["alpha", "beta", "gamma"]);
  assert.deepEqual(native.current_order, ["alpha", "gamma", "beta"]);
  assert.deepEqual(native.added_cases, ["delta"]);

  assert.equal(truncated.max_abs_delta, 0.000001);
  assert.deepEqual(truncated.threshold_crossings, []);
  assert.equal(truncated.order_changed, false);
  assert.deepEqual(truncated.removed_cases, ["gamma"]);
  assert.deepEqual(comparison.removed_profiles, ["removed-profile"]);

  const summary = formatComparison(comparison);
  assert.match(summary, /runtime 4\.2\.0 -> 4\.3\.1/);
  assert.match(summary, /beta\s+0\.810000\s+0\.799999\s+-0\.010001/);
  assert.match(summary, /max absolute delta: 0\.010001/);
  assert.match(summary, /beta: selected -> not selected/);
  assert.match(summary, /gamma: not selected -> selected/);
  assert.match(summary, /current: {2}alpha, gamma, beta/);
  assert.match(summary, /added cases: delta/);
  assert.match(summary, /removed cases: gamma/);
  assert.match(summary, /Removed profiles: removed-profile/);
});

test("comparison baseline must be model evidence with profiles", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-model-evidence-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, "evidence.json");

  writeFileSync(path, JSON.stringify({ profiles: "none" }));
  assert.throws(() => readComparisonBaseline(path), /must contain a profiles array/);
  writeFileSync(path, JSON.stringify({ profiles: [{ id: "native-768", threshold: 0.8 }] }));
  assert.throws(() => readComparisonBaseline(path), /native-768 must contain a cases array/);
  assert.equal(readComparisonBaseline(LOCKED_EVIDENCE).profiles.length, 2);
});
