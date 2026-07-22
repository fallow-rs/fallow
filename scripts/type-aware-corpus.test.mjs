import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  candidateKey,
  runOrderForIteration,
  validateManifest,
  verifyLedgerData,
} from "./type-aware-corpus.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const manifest = JSON.parse(
  readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-corpus.json"), "utf8"),
);

const candidate = {
  path: "src/service.ts",
  parent_name: "Service",
  member_name: "execute",
  kind: "class-method",
  line: 12,
  col: 3,
};

test("manifest contains the complete pinned accuracy and control corpus", () => {
  assert.deepEqual(validateManifest(structuredClone(manifest)), manifest);

  const invalid = structuredClone(manifest);
  invalid.projects.find(({ id }) => id === "vite").candidate_expectation = "nonzero";
  assert.throws(() => validateManifest(invalid), /vite\.candidate_expectation must be zero/);
});

test("candidate keys are stable across object property order and path separators", () => {
  const reordered = {
    col: 3,
    line: 12,
    kind: "class-method",
    member_name: "execute",
    parent_name: "Service",
    path: "src\\service.ts",
  };

  assert.equal(candidateKey("svelte", candidate), candidateKey("svelte", reordered));
  assert.notEqual(candidateKey("svelte", candidate), candidateKey("astro", candidate));
});

test("measured run order alternates baseline and refined", () => {
  assert.deepEqual(runOrderForIteration(0), ["baseline", "refined"]);
  assert.deepEqual(runOrderForIteration(1), ["refined", "baseline"]);
  assert.deepEqual(runOrderForIteration(2), ["baseline", "refined"]);
});

test("ledger verifier requires truth and complete source evidence for every candidate", () => {
  const key = candidateKey("svelte", candidate);
  const discovery = {
    schema_version: 1,
    projects: [
      {
        id: "svelte",
        candidates: [{ key, ...candidate, semantic_status: "confirmed-used" }],
      },
    ],
  };
  const incomplete = {
    schema_version: 1,
    candidates: [
      {
        key,
        project_id: "svelte",
        candidate,
        semantic_status: "confirmed-used",
        truth: "pending",
        feature_buckets: ["generics"],
        source_evidence: {
          declaration: { path: candidate.path, line: candidate.line, excerpt: "execute() {}" },
          uses: [],
          notes: null,
        },
      },
    ],
  };

  assert.deepEqual(verifyLedgerData(discovery, incomplete), [
    `${key}: truth must be used, unused, or indeterminate`,
    `${key}: used or confirmed candidates require concrete use evidence`,
  ]);

  const complete = structuredClone(incomplete);
  complete.candidates[0].truth = "used";
  complete.candidates[0].source_evidence.uses.push({
    path: "src/consumer.ts",
    line: 8,
    excerpt: "service.execute();",
  });
  assert.deepEqual(verifyLedgerData(discovery, complete), []);
});
