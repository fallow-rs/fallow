import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  assertDependencyFreeFixture,
  candidateFeatureBucketFields,
  candidateKey,
  independentReviewDigest,
  independentReviewErrors,
  indexLedgerForRefresh,
  main,
  normalizeSupplementalOutput,
  parseArgs,
  requireCompletePublicationCorpus,
  requirePublicationGo,
  runOrderForIteration,
  summarizeAdjudicatedFeatureBuckets,
  validateManifest,
  validateMeasurements,
  validatePartialOutput,
  validateSupplementalArtifactData,
  verifyLedgerData,
} from "./type-aware-corpus.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const manifest = JSON.parse(
  readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-corpus.json"), "utf8"),
);
const adjudication = JSON.parse(
  readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-adjudication.json"), "utf8"),
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

test("tracked supplemental smoke is internally reproducible and review-bound", () => {
  const artifact = JSON.parse(
    readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-supplemental-smoke.json"), "utf8"),
  );
  const sourceRoot = mkdtempSync(resolve(tmpdir(), "fallow-supplemental-validation-"));
  try {
    artifact.schema_version = 3;
    for (const run of artifact.artifacts.source_runs) {
      delete run.raw_sha256;
      run.path = `${run.mode}-${run.iteration}.json`;
      const output = {
        kind: "dead-code",
        elapsed_ms: run.iteration + 1,
        mode: run.mode,
        _meta: {
          telemetry: { analysis_run_id: `run-${run.iteration}` },
          type_aware: { elapsed_ms: run.iteration + 2, phase_timings_ms: { scan: 1 } },
        },
      };
      const sourcePath = resolve(sourceRoot, run.path);
      writeFileSync(sourcePath, `${JSON.stringify(output)}\n`);
      run.normalized_sha256 = createHash("sha256")
        .update(JSON.stringify(normalizeSupplementalOutput(output)))
        .digest("hex");
    }
    const context = {
      fallowSha256: artifact.artifacts.fallow_sha256,
      sidecarSha256: artifact.artifacts.sidecar_sha256,
      sourceRoot,
    };
    assert.deepEqual(validateSupplementalArtifactData(artifact, adjudication, context), artifact);

    const stale = structuredClone(artifact);
    stale.result.confirmed_candidate_keys.pop();
    assert.throws(
      () => validateSupplementalArtifactData(stale, adjudication, context),
      /counts, hashes, or review binding/,
    );

    const wrongRuntime = structuredClone(artifact);
    wrongRuntime.artifacts.fallow_sha256 = "0".repeat(64);
    assert.throws(
      () => validateSupplementalArtifactData(wrongRuntime, adjudication, context),
      /runtime hashes/,
    );

    const wrongNormalizedHash = structuredClone(artifact);
    for (const run of wrongNormalizedHash.artifacts.source_runs.filter(
      ({ mode }) => mode === "baseline",
    )) {
      run.normalized_sha256 = "0".repeat(64);
    }
    assert.throws(
      () => validateSupplementalArtifactData(wrongNormalizedHash, adjudication, context),
      /source hash/,
    );

    writeFileSync(resolve(sourceRoot, "baseline-0.json"), '{"tampered":true}\n');
    assert.throws(
      () => validateSupplementalArtifactData(artifact, adjudication, context),
      /source hash/,
    );
  } finally {
    rmSync(sourceRoot, { recursive: true, force: true });
  }
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

test("publication verifier rejects a focused Fallow binary hash mismatch", () => {
  const focusedReport = { fallow_sha256: "a".repeat(64) };
  const discovery = { provenance: { fallow: { sha256: "b".repeat(64) } } };
  const focusedCases = focusedReport.fallow_sha256 === discovery.provenance.fallow.sha256;
  const summary = { gate: { go: focusedCases, checks: { focused_cases: focusedCases } } };

  assert.throws(() => requirePublicationGo(summary, "verify"), /NO-GO.*focused_cases/);
  assert.doesNotThrow(() => requirePublicationGo(summary, "write"));
});

test("partial runs require an isolated non-canonical output directory", () => {
  const canonical = parseArgs([
    "discover",
    "--project",
    "astro",
    "--out-dir",
    resolve(REPO_ROOT, "target/type-aware-corpus"),
  ]);
  assert.throws(() => validatePartialOutput(canonical), /non-canonical --out-dir/);

  const isolated = parseArgs([
    "discover",
    "--project",
    "astro",
    "--out-dir",
    resolve(REPO_ROOT, "target/type-aware-corpus-astro"),
  ]);
  assert.doesNotThrow(() => validatePartialOutput(isolated));
});

test(
  "focused verification executes the exact sidecar artifact it hashes",
  { skip: process.platform === "win32" },
  async () => {
    const root = mkdtempSync(resolve(tmpdir(), "fallow-type-aware-sentinel-"));
    try {
      const sidecar = resolve(root, "sentinel-sidecar.mjs");
      writeFileSync(
        sidecar,
        '#!/usr/bin/env node\nprocess.stderr.write("sentinel sidecar executed\\n");\nprocess.exit(42);\n',
      );
      chmodSync(sidecar, 0o755);
      await assert.rejects(
        () => main(["focused", "--sidecar-bin", sidecar, "--out-dir", resolve(root, "artifacts")]),
        /candidate-bearing semantic edge-case suite failed/,
      );
      assert.match(
        `${readFileSync(resolve(root, "artifacts/focused/stdout.txt"), "utf8")}\n${readFileSync(
          resolve(root, "artifacts/focused/stderr.txt"),
          "utf8",
        )}`,
        /sentinel sidecar executed/,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);

test("prepared fixtures reject direct and nested dependency directories", () => {
  const root = mkdtempSync(resolve(tmpdir(), "fallow-type-aware-fixture-"));
  try {
    mkdirSync(resolve(root, "node_modules"));
    assert.throws(
      () => assertDependencyFreeFixture(root, "fixture", []),
      /contains dependency directories: node_modules/,
    );
    rmSync(resolve(root, "node_modules"), { recursive: true });
    assert.throws(
      () =>
        assertDependencyFreeFixture(root, "fixture", [
          "packages/app/node_modules/",
          "packages/app/.pnpm-store/",
        ]),
      /\.pnpm-store.*node_modules|node_modules.*\.pnpm-store/,
    );
    assert.doesNotThrow(() => assertDependencyFreeFixture(root, "fixture", []));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("manifest requires every release threshold", () => {
  const invalid = structuredClone(manifest);
  delete invalid.gates.maximum_abstention;
  assert.throws(
    () => validateManifest(invalid),
    /maximum_abstention must be a non-negative number/,
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

test("measurement validation requires complete paired runs", () => {
  const discovery = {
    corpus: [{ id: "one" }],
    provenance: { marker: "same" },
    projects: [{ id: "one", baseline_candidate_count: 2, refined_candidate_count: 1 }],
  };
  const runs = [];
  for (const warmup of [true, false]) {
    const iterations = warmup ? [0] : [0, 1, 2, 3];
    for (const iteration of iterations) {
      runs.push(
        {
          warmup,
          iteration,
          mode: "baseline",
          wall_ms: 1,
          peak_process_tree_rss_kb: 1,
          candidate_count: 2,
        },
        {
          warmup,
          iteration,
          mode: "refined",
          wall_ms: 2,
          peak_process_tree_rss_kb: 2,
          candidate_count: 1,
        },
      );
    }
  }
  const measurements = {
    schema_version: 1,
    corpus: discovery.corpus,
    provenance: discovery.provenance,
    warmups: 1,
    measured_pairs: 4,
    projects: [{ id: "one", runs }],
  };
  assert.doesNotThrow(() => validateMeasurements(discovery, measurements));

  const invalidMode = structuredClone(measurements);
  invalidMode.projects[0].runs.find(({ mode }) => mode === "refined").mode = "bogus";
  assert.throws(
    () => validateMeasurements(discovery, invalidMode),
    /mode must be baseline or refined/,
  );

  const invalidWarmup = structuredClone(measurements);
  invalidWarmup.projects[0].runs[0].warmup = "false";
  assert.throws(() => validateMeasurements(discovery, invalidWarmup), /warmup must be a boolean/);

  const invalidIteration = structuredClone(measurements);
  invalidIteration.projects[0].runs.find(
    ({ warmup, mode }) => !warmup && mode === "refined",
  ).iteration = 4;
  assert.throws(
    () => validateMeasurements(discovery, invalidIteration),
    /iteration is outside its expected range/,
  );

  measurements.projects[0].runs.pop();
  assert.throws(() => validateMeasurements(discovery, measurements), /incomplete run matrix/);
});

test("independent review digest binds declaration and use evidence", () => {
  const entry = {
    key: "candidate-one",
    project_id: "project-one",
    semantic_status: "confirmed-used",
    candidate: {
      path: "src/client.ts",
      parent_name: "Client",
      member_name: "execute",
      kind: "class-method",
      line: 2,
      col: 2,
    },
    source_evidence: {
      declaration: { path: "src/client.ts", line: 2, col: 2, excerpt: "execute(): void {}" },
      uses: [{ path: "src/app.ts", line: 4, col: 7, excerpt: "client.execute();" }],
    },
  };
  const digest = independentReviewDigest([entry]);
  const review = {
    project_id: "project-one",
    candidate_count: 1,
    candidate_set_sha256: createHash("sha256").update("candidate-one\n").digest("hex"),
    evidence_sha256: digest,
    verdict: "approved",
  };
  assert.deepEqual(independentReviewErrors([entry], [review]), []);
  const changed = structuredClone(entry);
  changed.source_evidence.uses[0].line = 5;
  assert.notEqual(independentReviewDigest([changed]), digest);
  assert.match(
    independentReviewErrors([changed], [review])[0],
    /does not match confirmed evidence/,
  );
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
          declaration: {
            path: candidate.path,
            line: candidate.line,
            col: candidate.col,
            excerpt: "execute() {}",
          },
          uses: [],
          notes: null,
        },
      },
    ],
  };

  assert.deepEqual(verifyLedgerData(discovery, incomplete), [
    `${key}: at least one explicitly adjudicated feature bucket is required`,
    `${key}: every confirmed removal must be adjudicated used`,
    `${key}: truth must be used, unused, or indeterminate`,
    `${key}: used or confirmed candidates require concrete use evidence`,
  ]);

  const complete = structuredClone(incomplete);
  complete.candidates[0].truth = "used";
  complete.candidates[0].adjudicated_feature_buckets = ["generics"];
  complete.candidates[0].source_evidence.uses.push({
    path: "src/consumer.ts",
    line: 8,
    col: 4,
    excerpt: "service.execute();",
  });
  assert.deepEqual(verifyLedgerData(discovery, complete), []);

  delete complete.candidates[0].suggested_feature_buckets;
  assert.deepEqual(verifyLedgerData(discovery, complete), []);

  assert.deepEqual(verifyLedgerData(discovery, { ...complete, schema_version: 1 }), [
    "ledger schema_version 1 is outdated; run `npm run type-aware:corpus -- evidence` to migrate it",
  ]);
});
