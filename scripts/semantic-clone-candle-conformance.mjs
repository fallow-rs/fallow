#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadManifest } from "./semantic-clone-conformance.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "tests/semantic-clone-corpus/manifest.json");
const DEFAULT_PROTOCOL_MANIFEST = resolve(REPO_ROOT, "crates/api/similar-code-protocol.json");
const DEFAULT_BASELINE = resolve(
  REPO_ROOT,
  "tests/semantic-clone-corpus/evidence/jina-v2-code-f32-candle.json",
);
const DEFAULT_SIDECAR_BIN = resolve(
  REPO_ROOT,
  "tools/similar-code-sidecar/target/release/fallow-similar-code",
);
const BASELINE_SCHEMA = "fallow-semantic-clone-candle-baseline/v1";
const ABSOLUTE_TOLERANCE = 0.001;
const CANDIDATE_THRESHOLD = 0.8;
const SIDECAR_TIMEOUT_MS = 15 * 60 * 1000;

const fail = (message) => {
  throw new Error(message);
};

const parseArgs = (argv) => {
  const options = {
    baseline: DEFAULT_BASELINE,
    manifest: DEFAULT_MANIFEST,
    pretty: false,
    protocolManifest: DEFAULT_PROTOCOL_MANIFEST,
    sidecarBin: process.env.FALLOW_SIMILAR_CODE_BIN ?? DEFAULT_SIDECAR_BIN,
    writeBaseline: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--baseline") {
      options.baseline = argv[index + 1] ?? fail("--baseline requires a path");
      index += 1;
    } else if (argument === "--manifest") {
      options.manifest = argv[index + 1] ?? fail("--manifest requires a path");
      index += 1;
    } else if (argument === "--pretty") {
      options.pretty = true;
    } else if (argument === "--protocol-manifest") {
      options.protocolManifest = argv[index + 1] ?? fail("--protocol-manifest requires a path");
      index += 1;
    } else if (argument === "--sidecar-bin") {
      options.sidecarBin = argv[index + 1] ?? fail("--sidecar-bin requires a path");
      index += 1;
    } else if (argument === "--write-baseline") {
      options.writeBaseline = argv[index + 1] ?? fail("--write-baseline requires a path");
      index += 1;
    } else {
      fail(`unknown argument: ${argument}`);
    }
  }
  return options;
};

const cosine = (left, right) => {
  if (left.length !== right.length || left.length === 0) {
    fail("Candle vectors must have equal non-zero dimensions");
  }
  let dot = 0;
  let leftNorm = 0;
  let rightNorm = 0;
  for (let index = 0; index < left.length; index += 1) {
    dot += left[index] * right[index];
    leftNorm += left[index] * left[index];
    rightNorm += right[index] * right[index];
  }
  const similarity = dot / Math.sqrt(leftNorm * rightNorm);
  if (!Number.isFinite(similarity) || similarity < -1.000_001 || similarity > 1.000_001) {
    fail("Candle produced an invalid cosine similarity");
  }
  return Math.max(-1, Math.min(1, similarity));
};

const buildRequest = (loaded, protocol) => {
  let key = 0;
  const functions = [];
  const caseKeys = new Map();
  for (const testCase of loaded.manifest.cases) {
    const keys = [];
    for (const file of testCase.files) {
      keys.push(key);
      functions.push({ key, source: readFileSync(resolve(loaded.root, file.fixture), "utf8") });
      key += 1;
    }
    caseKeys.set(testCase.id, keys);
  }
  return {
    caseKeys,
    request: {
      operation: protocol.analysis_operation,
      protocol_version: protocol.wire_protocol_version,
      embedding_semantics_version: protocol.embedding_semantics_version,
      model_revision: protocol.model.revision,
      dimensions: protocol.model.dimensions,
      max_tokens: protocol.model.max_tokens,
      functions,
      limits: {
        max_functions: functions.length,
        batch_size: 1,
        timeout_ms: 600_000,
      },
    },
  };
};

const invokeSidecar = (binary, request, invoke = spawnSync) => {
  const input = `${JSON.stringify(request)}\n${JSON.stringify({ operation: "shutdown" })}\n`;
  const result = invoke(binary, ["serve"], {
    cwd: REPO_ROOT,
    encoding: "utf8",
    env: { ...process.env, HF_HUB_OFFLINE: "1" },
    input,
    maxBuffer: 16 * 1024 * 1024,
    timeout: SIDECAR_TIMEOUT_MS,
  });
  if (result.error?.code === "ETIMEDOUT") {
    fail("production Candle conformance timed out");
  }
  if (result.error) {
    fail(`failed to start the production Candle sidecar: ${result.error.message}`);
  }
  if (result.signal) {
    fail(`production Candle sidecar terminated by signal ${result.signal}`);
  }
  if (result.status !== 0) {
    fail(`production Candle sidecar exited ${result.status}: ${result.stderr.trim()}`);
  }
  const lines = result.stdout.split(/\r?\n/u).filter((line) => line.length > 0);
  if (lines.length !== 1) {
    fail(`production Candle sidecar returned ${lines.length} response lines, expected one`);
  }
  try {
    return JSON.parse(lines[0]);
  } catch (error) {
    fail(`production Candle sidecar returned invalid JSON: ${error.message}`);
  }
};

const validateResponse = (response, request, protocol) => {
  if (response.protocol_version !== protocol.wire_protocol_version) {
    fail("Candle response wire protocol differs from the release manifest");
  }
  if (response.embedding_semantics_version !== protocol.embedding_semantics_version) {
    fail("Candle response embedding semantics differ from the release manifest");
  }
  if (response.model_revision !== protocol.model.revision) {
    fail("Candle response model revision differs from the release manifest");
  }
  if (response.dimensions !== protocol.model.dimensions) {
    fail("Candle response dimensions differ from the release manifest");
  }
  if (response.status !== "complete" || response.errors?.length > 0) {
    fail(`Candle response was not complete: ${JSON.stringify(response.errors ?? [])}`);
  }
  if (response.completion?.embedded_functions !== request.functions.length) {
    fail("Candle response did not embed the complete conformance corpus");
  }
  if (!Array.isArray(response.vectors) || response.vectors.length !== request.functions.length) {
    fail("Candle response returned an incomplete vector set");
  }
  const vectors = new Map();
  for (const vector of response.vectors) {
    if (
      !Number.isSafeInteger(vector.key) ||
      !Array.isArray(vector.values) ||
      vector.values.length !== protocol.model.dimensions ||
      vector.values.some((value) => !Number.isFinite(value))
    ) {
      fail("Candle response returned an invalid vector");
    }
    if (vectors.has(vector.key)) {
      fail(`Candle response repeated vector key ${vector.key}`);
    }
    vectors.set(vector.key, vector.values);
  }
  return vectors;
};

const createEvidence = (loaded, protocol, response, request, caseKeys) => {
  const vectors = validateResponse(response, request, protocol);
  return {
    $schema: BASELINE_SCHEMA,
    corpus_revision: loaded.manifest.source.revision,
    provider: {
      runtime: "fallow-similar-code",
      execution: "candle-f32-cpu",
      source_left_machine: false,
      wire_protocol_version: protocol.wire_protocol_version,
      embedding_semantics_version: protocol.embedding_semantics_version,
    },
    model: {
      id: protocol.model.id,
      revision: protocol.model.revision,
      artifact: "f32-safetensors",
      dimensions: protocol.model.dimensions,
      max_tokens: protocol.model.max_tokens,
      normalization: protocol.model.normalization,
    },
    comparison: {
      metric: "cosine-similarity",
      threshold: CANDIDATE_THRESHOLD,
      absolute_tolerance: ABSOLUTE_TOLERANCE,
    },
    cases: loaded.manifest.cases.map((testCase) => {
      const keys = caseKeys.get(testCase.id);
      const similarity = cosine(vectors.get(keys[0]), vectors.get(keys[1]));
      return {
        id: testCase.id,
        similarity: Number(similarity.toFixed(6)),
        selected: similarity >= CANDIDATE_THRESHOLD,
      };
    }),
  };
};

const validateBaselineShape = (baseline, loaded, protocol) => {
  if (baseline.$schema !== BASELINE_SCHEMA) {
    fail(`unsupported Candle baseline schema: ${baseline.$schema}`);
  }
  if (baseline.corpus_revision !== loaded.manifest.source.revision) {
    fail("Candle baseline targets a different corpus revision");
  }
  if (baseline.provider?.execution !== "candle-f32-cpu") {
    fail("Candle baseline must identify the production F32 runtime");
  }
  if (baseline.provider?.source_left_machine !== false) {
    fail("Candle baseline must state that source stayed local");
  }
  if (
    baseline.provider?.wire_protocol_version !== protocol.wire_protocol_version ||
    baseline.provider?.embedding_semantics_version !== protocol.embedding_semantics_version
  ) {
    fail("Candle baseline protocol provenance is stale");
  }
  if (
    baseline.model?.id !== protocol.model.id ||
    baseline.model?.revision !== protocol.model.revision ||
    baseline.model?.artifact !== "f32-safetensors" ||
    baseline.model?.dimensions !== protocol.model.dimensions ||
    baseline.model?.max_tokens !== protocol.model.max_tokens ||
    baseline.model?.normalization !== protocol.model.normalization
  ) {
    fail("Candle baseline model provenance is stale");
  }
  if (
    baseline.comparison?.metric !== "cosine-similarity" ||
    baseline.comparison?.threshold !== CANDIDATE_THRESHOLD ||
    baseline.comparison?.absolute_tolerance !== ABSOLUTE_TOLERANCE
  ) {
    fail("Candle baseline comparison contract is stale");
  }
  const expectedIds = loaded.manifest.cases.map((testCase) => testCase.id);
  if (
    !Array.isArray(baseline.cases) ||
    baseline.cases.length !== expectedIds.length ||
    baseline.cases.some(
      (testCase, index) =>
        testCase.id !== expectedIds[index] ||
        !Number.isFinite(testCase.similarity) ||
        typeof testCase.selected !== "boolean",
    )
  ) {
    fail("Candle baseline does not exactly cover the locked corpus");
  }
};

const compareEvidence = (actual, baseline, loaded, protocol) => {
  validateBaselineShape(baseline, loaded, protocol);
  validateBaselineShape(actual, loaded, protocol);
  for (let index = 0; index < baseline.cases.length; index += 1) {
    const expectedCase = baseline.cases[index];
    const actualCase = actual.cases[index];
    const delta = Math.abs(expectedCase.similarity - actualCase.similarity);
    if (delta > baseline.comparison.absolute_tolerance) {
      fail(
        `${actualCase.id}: Candle similarity drift ${delta.toFixed(6)} exceeds ` +
          baseline.comparison.absolute_tolerance,
      );
    }
    if (actualCase.selected !== expectedCase.selected) {
      fail(`${actualCase.id}: Candle candidate selection changed`);
    }
  }
};

const runCandleConformance = (options, invoke = spawnSync) => {
  const loaded = loadManifest(options.manifest, { includeCandidateEvidence: false });
  const protocol = JSON.parse(readFileSync(resolve(options.protocolManifest), "utf8"));
  const { caseKeys, request } = buildRequest(loaded, protocol);
  const response = invokeSidecar(resolve(options.sidecarBin), request, invoke);
  const evidence = createEvidence(loaded, protocol, response, request, caseKeys);
  if (options.writeBaseline !== null) {
    writeFileSync(resolve(options.writeBaseline), `${JSON.stringify(evidence, null, 2)}\n`);
  } else {
    const baseline = JSON.parse(readFileSync(resolve(options.baseline), "utf8"));
    compareEvidence(evidence, baseline, loaded, protocol);
  }
  return evidence;
};

const main = () => {
  const options = parseArgs(process.argv.slice(2));
  const evidence = runCandleConformance(options);
  const output = options.pretty ? JSON.stringify(evidence, null, 2) : JSON.stringify(evidence);
  process.stdout.write(`${output}\n`);
};

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

export {
  ABSOLUTE_TOLERANCE,
  BASELINE_SCHEMA,
  buildRequest,
  compareEvidence,
  createEvidence,
  parseArgs,
  runCandleConformance,
  validateBaselineShape,
  validateResponse,
};
