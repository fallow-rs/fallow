import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  classifyFinding,
  evaluatePairCoverage,
  loadManifest,
  parseArgs,
  runConformance,
  validateFallowReport,
} from "./semantic-clone-conformance.mjs";
import {
  ABSOLUTE_TOLERANCE,
  buildRequest as buildCandleRequest,
  compareEvidence as compareCandleEvidence,
  parseArgs as parseCandleArgs,
  validateBaselineShape as validateCandleBaselineShape,
  validateResponse as validateCandleResponse,
} from "./semantic-clone-candle-conformance.mjs";
import { parseArgs as parseModelArgs } from "./semantic-clone-model-evidence.mjs";

const REPO_ROOT = resolve(import.meta.dirname, "..");
const MANIFEST = resolve(REPO_ROOT, "tests/semantic-clone-corpus/manifest.json");
const PROTOCOL_MANIFEST = resolve(REPO_ROOT, "crates/api/similar-code-protocol.json");
const CANDLE_BASELINE = resolve(
  REPO_ROOT,
  "tests/semantic-clone-corpus/evidence/jina-v2-code-f32-candle.json",
);

const countLines = (path) => {
  const contents = readFileSync(path, "utf8");
  return contents.length === 0 ? 0 : contents.replace(/\n$/, "").split("\n").length;
};

const mockReport = (loaded, testCase, cloneGroups = []) => ({
  kind: "dupes",
  schema_version: loaded.manifest.deterministic_baseline.schema_version,
  clone_groups: cloneGroups,
  stats: {
    total_files: testCase.files.length,
    total_lines: testCase.files.reduce(
      (total, file) => total + countLines(resolve(loaded.root, file.fixture)),
      0,
    ),
  },
});

test("manifest locks public fixture provenance and relationship labels", () => {
  const { manifest } = loadManifest(MANIFEST);

  assert.equal(manifest.source.public_fixture, true);
  assert.match(manifest.source.revision, /^[0-9a-f]{40}$/);
  assert.equal(manifest.vector_fixture.dimensions, 256);
  assert.equal(manifest.vector_fixture.quality_evidence, false);
  assert.ok(manifest.cases.some((entry) => entry.truth.candidate_worthy));
  assert.ok(manifest.cases.some((entry) => !entry.truth.candidate_worthy));
});

test("candidate evidence pins local execution and complete case coverage", () => {
  const loaded = loadManifest(MANIFEST);

  assert.equal(loaded.candidateEvidence.length, 1);
  assert.equal(loaded.candidateEvidence[0].provider.source_left_machine, false);
  assert.match(loaded.candidateEvidence[0].provider.runtime_lock_sha256, /^[0-9a-f]{64}$/);
  assert.match(loaded.candidateEvidence[0].model.revision, /^[0-9a-f]{40}$/);
  assert.deepEqual(
    loaded.candidateEvidence[0].profiles.map((profile) => [
      profile.dimensions,
      profile.model_supports_dimensions,
    ]),
    [
      [768, true],
      [256, false],
    ],
  );
});

test("pair coverage requires both files in one clone group", () => {
  const result = evaluatePairCoverage(
    [
      {
        instances: [
          { file: "a.ts", start_line: 1, end_line: 8 },
          { file: "b.ts", start_line: 3, end_line: 10 },
        ],
      },
      {
        instances: [{ file: "a.ts", start_line: 1, end_line: 10 }],
      },
    ],
    ["a.ts", "b.ts"],
    new Map([
      ["a.ts", 10],
      ["b.ts", 20],
    ]),
  );

  assert.deepEqual(result, {
    best_pair_coverage: 0.4,
    matching_groups: 1,
  });
});

test("finding classification uses refactor safety rather than candidate value", () => {
  assert.equal(classifyFinding(true, true), "true_positive");
  assert.equal(classifyFinding(true, false), "false_positive");
  assert.equal(classifyFinding(false, true), "false_negative");
  assert.equal(classifyFinding(false, false), "true_negative");
});

test("conformance output keeps candidate gaps separate from findings", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runConformance(
    loaded,
    (_binary, testCase) =>
      mockReport(
        loaded,
        testCase,
        testCase.id === "renamed-identifiers-ts-02"
          ? [
              {
                instances: [
                  { file: "a.ts", start_line: 1, end_line: 32 },
                  { file: "b.ts", start_line: 1, end_line: 32 },
                ],
              },
            ]
          : [],
      ),
    "/fixture/fallow",
  );

  assert.equal(result.cases[0].candidate_gap, true);
  assert.equal(result.cases[0].deterministic_finding.classification, "true_negative");
  assert.equal(
    result.cases.find((entry) => entry.id === "renamed-identifiers-ts-02").deterministic_finding
      .classification,
    "true_positive",
  );
  assert.equal(result.candidate_evidence[0].profiles[0].added_vs_deterministic, 1);
  assert.deepEqual(result.candidate_evidence[0].profiles[0].classifications, {
    true_positive: 2,
    false_positive: 0,
    false_negative: 1,
    true_negative: 4,
  });
  assert.equal(result.summary.baseline_drift, 0);
});

test("conformance rejects incomplete or out-of-scope fallow reports", () => {
  const loaded = loadManifest(MANIFEST);
  const testCase = loaded.manifest.cases[0];
  const fileNames = testCase.files.map((file) => file.fixture.split("/").at(-1));
  const fileLineCounts = new Map(
    testCase.files.map((file, index) => [
      fileNames[index],
      countLines(resolve(loaded.root, file.fixture)),
    ]),
  );

  assert.throws(
    () =>
      validateFallowReport(
        { clone_groups: [], stats: {} },
        testCase,
        fileNames,
        fileLineCounts,
        loaded.manifest.deterministic_baseline,
      ),
    /report kind must be dupes/,
  );
  assert.throws(
    () =>
      validateFallowReport(
        mockReport(loaded, testCase, [
          {
            instances: [
              { file: fileNames[0], start_line: 1, end_line: 2 },
              { file: "outside.ts", start_line: 1, end_line: 2 },
            ],
          },
        ]),
        testCase,
        fileNames,
        fileLineCounts,
        loaded.manifest.deterministic_baseline,
      ),
    /outside the locked pair/,
  );
});

test("conformance reports a locked baseline mismatch", () => {
  const loaded = loadManifest(MANIFEST);
  const result = runConformance(
    loaded,
    (_binary, testCase) => mockReport(loaded, testCase),
    "/fixture/fallow",
  );

  assert.equal(result.summary.baseline_drift, 1);
  assert.equal(
    result.cases.find((entry) => entry.id === "renamed-identifiers-ts-02").deterministic_finding
      .matches_expected,
    false,
  );
});

test("argument parsing is explicit", () => {
  assert.deepEqual(parseArgs(["--fallow-bin", "/tmp/fallow", "--pretty"]), {
    check: false,
    fallowBin: "/tmp/fallow",
    manifest: MANIFEST,
    pretty: true,
  });
  assert.equal(parseArgs(["--check"]).check, true);
  assert.throws(() => parseArgs(["--unknown"]), /unknown argument/);
});

test("model evidence requires locked runtime provenance", () => {
  assert.deepEqual(
    parseModelArgs([
      "--runtime-lock",
      "/tmp/package-lock.json",
      "--transformers-module",
      "/tmp/transformers.node.mjs",
      "--model-cache-state",
      "cold",
    ]),
    {
      manifest: MANIFEST,
      modelCacheState: "cold",
      runtimeLock: "/tmp/package-lock.json",
      transformersModule: "/tmp/transformers.node.mjs",
    },
  );
  assert.throws(
    () => parseModelArgs(["--transformers-module", "/tmp/transformers.node.mjs"]),
    /--runtime-lock is required/,
  );
  assert.throws(
    () =>
      parseModelArgs([
        "--runtime-lock",
        "/tmp/package-lock.json",
        "--transformers-module",
        "/tmp/transformers.node.mjs",
        "--model-cache-state",
        "maybe",
      ]),
    /requires cold, warm, or unknown/,
  );
});

test("Candle conformance sends the versioned production embedding contract", () => {
  const loaded = loadManifest(MANIFEST, { includeCandidateEvidence: false });
  const protocol = JSON.parse(readFileSync(PROTOCOL_MANIFEST, "utf8"));
  const { caseKeys, request } = buildCandleRequest(loaded, protocol);

  assert.equal(request.operation, protocol.analysis_operation);
  assert.equal(request.protocol_version, protocol.wire_protocol_version);
  assert.equal(request.embedding_semantics_version, protocol.embedding_semantics_version);
  assert.equal(request.model_revision, protocol.model.revision);
  assert.equal(request.functions.length, loaded.manifest.cases.length * 2);
  assert.equal(caseKeys.size, loaded.manifest.cases.length);
  assert.ok(request.functions.every((entry) => !Object.hasOwn(entry, "fixture")));
});

test("Candle conformance rejects incomplete or stale sidecar responses", () => {
  const protocol = JSON.parse(readFileSync(PROTOCOL_MANIFEST, "utf8"));
  const request = { functions: [{ key: 0, source: "function a() {}" }] };
  const response = {
    protocol_version: protocol.wire_protocol_version,
    embedding_semantics_version: protocol.embedding_semantics_version,
    model_revision: protocol.model.revision,
    dimensions: protocol.model.dimensions,
    status: "complete",
    completion: { embedded_functions: 1 },
    vectors: [{ key: 0, values: Array(protocol.model.dimensions).fill(0.1) }],
  };

  assert.equal(validateCandleResponse(response, request, protocol).size, 1);
  assert.throws(
    () =>
      validateCandleResponse(
        { ...response, embedding_semantics_version: protocol.embedding_semantics_version + 1 },
        request,
        protocol,
      ),
    /embedding semantics differ/,
  );
  assert.throws(
    () => validateCandleResponse({ ...response, status: "partial" }, request, protocol),
    /was not complete/,
  );
});

test("committed Candle baseline is compact, complete, and tolerance bounded", () => {
  const loaded = loadManifest(MANIFEST, { includeCandidateEvidence: false });
  const protocol = JSON.parse(readFileSync(PROTOCOL_MANIFEST, "utf8"));
  const baseline = JSON.parse(readFileSync(CANDLE_BASELINE, "utf8"));

  validateCandleBaselineShape(baseline, loaded, protocol);
  assert.equal(baseline.comparison.absolute_tolerance, ABSOLUTE_TOLERANCE);
  assert.ok(Buffer.byteLength(JSON.stringify(baseline)) < 4096);
  assert.ok(
    baseline.cases.every(
      (entry) => Object.keys(entry).toSorted().join(",") === "id,selected,similarity",
    ),
  );

  const withinTolerance = structuredClone(baseline);
  withinTolerance.cases[0].similarity += ABSOLUTE_TOLERANCE / 2;
  compareCandleEvidence(withinTolerance, baseline, loaded, protocol);

  const drifted = structuredClone(baseline);
  drifted.cases[0].similarity += ABSOLUTE_TOLERANCE * 2;
  assert.throws(
    () => compareCandleEvidence(drifted, baseline, loaded, protocol),
    /similarity drift/,
  );
});

test("Candle conformance arguments require explicit values", () => {
  const parsed = parseCandleArgs([
    "--sidecar-bin",
    "/tmp/fallow-similar-code",
    "--baseline",
    "/tmp/baseline.json",
    "--pretty",
  ]);
  assert.equal(parsed.sidecarBin, "/tmp/fallow-similar-code");
  assert.equal(parsed.baseline, "/tmp/baseline.json");
  assert.equal(parsed.pretty, true);
  assert.throws(() => parseCandleArgs(["--write-baseline"]), /requires a path/);
  assert.throws(() => parseCandleArgs(["--unknown"]), /unknown argument/);
});
