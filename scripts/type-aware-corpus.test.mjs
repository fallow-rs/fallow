import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  candidateFeatureBucketFields,
  candidateKey,
  indexLedgerForRefresh,
  requireCompletePublicationCorpus,
  runOrderForIteration,
  summarizeAdjudicatedFeatureBuckets,
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

test("publication gate rejects a subset missing the zero-control projects", () => {
  const corpus = manifest.projects.map(({ id, repo, ref, role, candidate_expectation }) => ({
    id,
    repo,
    ref,
    role,
    candidate_expectation,
  }));
  assert.doesNotThrow(() => requireCompletePublicationCorpus(manifest, { corpus }));

  const accuracyOnly = corpus.filter(({ role }) => role === "accuracy-core");
  assert.throws(
    () => requireCompletePublicationCorpus(manifest, { corpus: accuracyOnly }),
    /complete manifest corpus.*without `--project`.*run evidence before summarize/,
  );
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

test("ledger migration keeps legacy project buckets as non-gating suggestions", () => {
  const legacy = {
    feature_buckets: ["generics", "project-references"],
    truth: "used",
  };
  assert.deepEqual(candidateFeatureBucketFields(["framework-runtime"], legacy, 1), {
    suggested_feature_buckets: ["generics", "project-references"],
    adjudicated_feature_buckets: [],
  });

  const adjudicated = {
    suggested_feature_buckets: ["generics", "project-references"],
    adjudicated_feature_buckets: ["generics"],
  };
  assert.deepEqual(
    candidateFeatureBucketFields(["framework-runtime"], adjudicated, 2),
    adjudicated,
  );
});

test("ledger refresh accepts supported schemas and preserves candidate entries", () => {
  const previousEntry = {
    key: "candidate-one",
    truth: "used",
    source_evidence: { notes: "reviewed" },
  };

  for (const schemaVersion of [1, 2]) {
    const indexed = indexLedgerForRefresh(
      { schema_version: schemaVersion, candidates: [previousEntry] },
      new Set([previousEntry.key]),
    );
    assert.equal(indexed.get(previousEntry.key), previousEntry);
  }
});

test("ledger refresh rejects an unknown schema before writing", () => {
  assert.throws(
    () => indexLedgerForRefresh({ schema_version: 3, candidates: [] }, new Set()),
    /must use schema_version 1 or 2.*Archive the old ledger or restore the discovery/s,
  );
});

test("ledger refresh rejects duplicate existing candidate keys before writing", () => {
  const duplicate = { key: "candidate-one" };
  assert.throws(
    () =>
      indexLedgerForRefresh(
        { schema_version: 2, candidates: [duplicate, structuredClone(duplicate)] },
        new Set([duplicate.key]),
      ),
    /duplicate candidate key "candidate-one".*Archive the old ledger or restore the discovery/s,
  );
});

test("ledger refresh rejects existing candidate keys absent from discovery before writing", () => {
  assert.throws(
    () =>
      indexLedgerForRefresh(
        { schema_version: 2, candidates: [{ key: "stale-candidate" }] },
        new Set(["current-candidate"]),
      ),
    /"stale-candidate" is missing from the current discovery.*Archive the old ledger or restore the discovery/s,
  );
});

test("feature bucket gate needs separate correct confirmations for distinct buckets", () => {
  const confirmed = (key, buckets) => ({
    key,
    semantic_status: "confirmed-used",
    truth: "used",
    adjudicated_feature_buckets: buckets,
  });

  assert.deepEqual(summarizeAdjudicatedFeatureBuckets([confirmed("one", ["a", "b"])]), {
    confirmed_feature_buckets: ["a", "b"],
    multiple_feature_buckets: false,
  });
  assert.deepEqual(
    summarizeAdjudicatedFeatureBuckets([confirmed("one", ["a"]), confirmed("two", ["b"])]),
    {
      confirmed_feature_buckets: ["a", "b"],
      multiple_feature_buckets: true,
    },
  );
  assert.deepEqual(
    summarizeAdjudicatedFeatureBuckets([
      confirmed("one", ["a"]),
      { ...confirmed("two", ["b"]), semantic_status: "retained" },
    ]),
    {
      confirmed_feature_buckets: ["a"],
      multiple_feature_buckets: false,
    },
  );
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
    schema_version: 2,
    candidates: [
      {
        key,
        project_id: "svelte",
        candidate,
        semantic_status: "confirmed-used",
        truth: "pending",
        suggested_feature_buckets: ["generics"],
        adjudicated_feature_buckets: [],
        source_evidence: {
          declaration: { path: candidate.path, line: candidate.line, excerpt: "execute() {}" },
          uses: [],
          notes: null,
        },
      },
    ],
  };

  assert.deepEqual(verifyLedgerData(discovery, incomplete), [
    `${key}: at least one explicitly adjudicated feature bucket is required`,
    `${key}: truth must be used, unused, or indeterminate`,
    `${key}: used or confirmed candidates require concrete use evidence`,
  ]);

  const complete = structuredClone(incomplete);
  complete.candidates[0].truth = "used";
  complete.candidates[0].adjudicated_feature_buckets = ["generics"];
  complete.candidates[0].source_evidence.uses.push({
    path: "src/consumer.ts",
    line: 8,
    excerpt: "service.execute();",
  });
  assert.deepEqual(verifyLedgerData(discovery, complete), []);

  delete complete.candidates[0].suggested_feature_buckets;
  assert.deepEqual(verifyLedgerData(discovery, complete), []);

  assert.deepEqual(verifyLedgerData(discovery, { ...complete, schema_version: 1 }), [
    "ledger schema_version 1 is outdated; run `npm run type-aware:corpus -- evidence` to migrate it",
  ]);
});
