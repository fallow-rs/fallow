#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";

import { loadManifest } from "./semantic-clone-conformance.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "tests/semantic-clone-corpus/manifest.json");
const MODEL = "jinaai/jina-embeddings-v2-base-code";
const MODEL_REVISION = "516f4baf13dec4ddddda8631e019b5737c8bc250";
const MODEL_LICENSE = "Apache-2.0";
const DTYPE = "q8";
const NATIVE_DIMENSIONS = 768;
const EXPERIMENTAL_DIMENSIONS = 256;
const THRESHOLD = 0.8;
const RUNTIME_LOCK_REFERENCE = "runtime/package-lock.json";
const TRANSFORMERS_PACKAGE_PATH = "node_modules/@huggingface/transformers";

const fail = (message) => {
  throw new Error(message);
};

const sha256 = (contents) => createHash("sha256").update(contents).digest("hex");

const VALUE_FLAGS = {
  "--compare": { key: "compare", missing: "--compare requires an evidence path" },
  "--manifest": { key: "manifest", missing: "--manifest requires a path" },
  "--model-cache-state": {
    key: "modelCacheState",
    missing: "--model-cache-state requires cold, warm, or unknown",
  },
  "--runtime-lock": { key: "runtimeLock", missing: "--runtime-lock requires a path" },
  "--transformers-module": {
    key: "transformersModule",
    missing: "--transformers-module requires a path",
  },
};

const parseArgs = (argv) => {
  const options = {
    compare: null,
    manifest: DEFAULT_MANIFEST,
    modelCacheState: "unknown",
    runtimeLock: null,
    transformersModule: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = VALUE_FLAGS[argv[index]] ?? fail(`unknown argument: ${argv[index]}`);
    options[flag.key] = argv[index + 1] ?? fail(flag.missing);
    index += 1;
  }
  if (options.transformersModule === null) {
    fail("--transformers-module is required; see the corpus README for isolated setup");
  }
  if (options.runtimeLock === null) {
    fail("--runtime-lock is required to pin the complete model runtime dependency graph");
  }
  if (!["cold", "warm", "unknown"].includes(options.modelCacheState)) {
    fail("--model-cache-state requires cold, warm, or unknown");
  }
  return options;
};

const cosine = (left, right, dimensions) => {
  let dot = 0;
  let leftNorm = 0;
  let rightNorm = 0;
  for (let index = 0; index < dimensions; index += 1) {
    dot += left[index] * right[index];
    leftNorm += left[index] * left[index];
    rightNorm += right[index] * right[index];
  }
  return dot / Math.sqrt(leftNorm * rightNorm);
};

const profile = (id, dimensions, modelSupportsDimensions, vectors, cases) => ({
  id,
  dimensions,
  model_supports_dimensions: modelSupportsDimensions,
  threshold: THRESHOLD,
  cases: cases.map((testCase, index) => ({
    id: testCase.id,
    similarity: Number(cosine(vectors[index * 2], vectors[index * 2 + 1], dimensions).toFixed(6)),
  })),
});

const readPackageVersion = (modulePath) => {
  const packagePath = resolve(dirname(modulePath), "../package.json");
  return JSON.parse(readFileSync(packagePath, "utf8")).version;
};

const validateRuntimeProvenance = (loaded, options) => {
  const runtimeLockPath = resolve(options.runtimeLock);
  const runtimeLock = readFileSync(runtimeLockPath);
  const recordedRuntimeLock = readFileSync(resolve(loaded.root, RUNTIME_LOCK_REFERENCE));
  if (!runtimeLock.equals(recordedRuntimeLock)) {
    fail(`runtime lock must match the recorded ${RUNTIME_LOCK_REFERENCE}`);
  }

  const runtimeLockData = JSON.parse(runtimeLock.toString("utf8"));
  if (!Number.isSafeInteger(runtimeLockData.lockfileVersion)) {
    fail("runtime lock must be an npm package lock");
  }
  if (runtimeLockData.packages?.[""]?.dependencies?.["@huggingface/transformers"] !== "4.3.1") {
    fail("runtime lock must declare @huggingface/transformers 4.3.1 at its root");
  }
  const lockedTransformers = runtimeLockData.packages?.[TRANSFORMERS_PACKAGE_PATH];
  if (lockedTransformers?.version !== "4.3.1" || typeof lockedTransformers.integrity !== "string") {
    fail("runtime lock must contain the exact Transformers package and integrity");
  }

  const modulePath = resolve(options.transformersModule);
  const expectedModulePath = resolve(
    dirname(runtimeLockPath),
    TRANSFORMERS_PACKAGE_PATH,
    "dist/transformers.node.mjs",
  );
  if (modulePath !== expectedModulePath) {
    fail("Transformers module must come from the installation governed by --runtime-lock");
  }
  const runtimeVersion = readPackageVersion(modulePath);
  if (runtimeVersion !== lockedTransformers.version) {
    fail(`loaded Transformers ${runtimeVersion} differs from locked ${lockedTransformers.version}`);
  }

  return { modulePath, runtimeLock, runtimeLockData, runtimeVersion };
};

const generateEvidence = async (options) => {
  const loaded = loadManifest(options.manifest, { includeCandidateEvidence: false });
  const { modulePath, runtimeLock, runtimeLockData, runtimeVersion } = validateRuntimeProvenance(
    loaded,
    options,
  );
  const { pipeline } = await import(pathToFileURL(modulePath).href);
  const texts = loaded.manifest.cases.flatMap((testCase) =>
    testCase.files.map((file) => readFileSync(resolve(loaded.root, file.fixture), "utf8")),
  );

  const loadStarted = performance.now();
  const extractor = await pipeline("feature-extraction", MODEL, {
    dtype: DTYPE,
    revision: MODEL_REVISION,
  });
  const loadMs = performance.now() - loadStarted;

  const embeddingStarted = performance.now();
  let maxObservedRssBytes = process.memoryUsage().rss;
  const vectors = [];
  for (const text of texts) {
    const output = await extractor(text, { pooling: "mean", normalize: true });
    const dimensions = output.dims[1];
    if (dimensions !== NATIVE_DIMENSIONS) {
      fail(`model returned ${dimensions} dimensions, expected ${NATIVE_DIMENSIONS}`);
    }
    vectors.push(Array.from(output.data));
    maxObservedRssBytes = Math.max(maxObservedRssBytes, process.memoryUsage().rss);
  }
  const embeddingMs = performance.now() - embeddingStarted;

  return {
    $schema: "fallow-semantic-clone-model-evidence/v1",
    corpus_revision: loaded.manifest.source.revision,
    provider: {
      runtime: "@huggingface/transformers",
      runtime_version: runtimeVersion,
      runtime_lock_file: RUNTIME_LOCK_REFERENCE,
      runtime_lock_sha256: sha256(runtimeLock),
      runtime_lock_version: runtimeLockData.lockfileVersion,
      execution: "local-onnx",
      source_left_machine: false,
    },
    model: {
      id: MODEL,
      revision: MODEL_REVISION,
      license: MODEL_LICENSE,
      artifact: DTYPE,
      native_dimensions: NATIVE_DIMENSIONS,
      generation_parameters: {
        batch_size: 1,
        pooling: "mean",
        normalize: true,
      },
    },
    resource_observation: {
      model_cache: options.modelCacheState,
      embedding_batch_size: 1,
      load_ms: Math.round(loadMs),
      embedding_ms: Math.round(embeddingMs),
      max_observed_rss_bytes: maxObservedRssBytes,
      rss_bytes_after_embedding: process.memoryUsage().rss,
      runtime: process.version,
      platform: process.platform,
      architecture: process.arch,
    },
    profiles: [
      profile("native-768", NATIVE_DIMENSIONS, true, vectors, loaded.manifest.cases),
      profile(
        "naive-truncation-256",
        EXPERIMENTAL_DIMENSIONS,
        false,
        vectors,
        loaded.manifest.cases,
      ),
    ],
  };
};

const rankCases = (cases) =>
  cases
    .toSorted(
      (left, right) => right.similarity - left.similarity || left.id.localeCompare(right.id),
    )
    .map((testCase) => testCase.id);

const compareProfile = (previous, current) => {
  const previousCases = new Map(previous.cases.map((testCase) => [testCase.id, testCase]));
  const currentIds = new Set(current.cases.map((testCase) => testCase.id));
  const cases = current.cases
    .filter((testCase) => previousCases.has(testCase.id))
    .map((testCase) => {
      const before = previousCases.get(testCase.id).similarity;
      const after = testCase.similarity;
      return {
        id: testCase.id,
        previous: before,
        current: after,
        delta: Number((after - before).toFixed(6)),
        previous_selected: before >= previous.threshold,
        current_selected: after >= current.threshold,
      };
    });
  const sharedIds = new Set(cases.map((testCase) => testCase.id));
  const previousOrder = rankCases(previous.cases.filter((testCase) => sharedIds.has(testCase.id)));
  const currentOrder = rankCases(current.cases.filter((testCase) => sharedIds.has(testCase.id)));
  return {
    id: current.id,
    previous_threshold: previous.threshold,
    current_threshold: current.threshold,
    cases,
    max_abs_delta: cases.reduce((max, testCase) => Math.max(max, Math.abs(testCase.delta)), 0),
    threshold_crossings: cases.filter(
      (testCase) => testCase.previous_selected !== testCase.current_selected,
    ),
    previous_order: previousOrder,
    current_order: currentOrder,
    order_changed: previousOrder.some((id, index) => id !== currentOrder[index]),
    added_cases: current.cases
      .filter((testCase) => !previousCases.has(testCase.id))
      .map((testCase) => testCase.id),
    removed_cases: previous.cases
      .filter((testCase) => !currentIds.has(testCase.id))
      .map((testCase) => testCase.id),
  };
};

/**
 * Compares two model evidence documents profile by profile and case by case.
 * Profiles and cases match by id. The result keeps the order of `current`.
 */
const compareEvidence = (previous, current) => {
  const previousProfiles = new Map(previous.profiles.map((entry) => [entry.id, entry]));
  const currentIds = new Set(current.profiles.map((entry) => entry.id));
  return {
    previous_runtime_version: previous.provider?.runtime_version ?? null,
    current_runtime_version: current.provider?.runtime_version ?? null,
    profiles: current.profiles
      .filter((entry) => previousProfiles.has(entry.id))
      .map((entry) => compareProfile(previousProfiles.get(entry.id), entry)),
    added_profiles: current.profiles
      .filter((entry) => !previousProfiles.has(entry.id))
      .map((entry) => entry.id),
    removed_profiles: previous.profiles
      .filter((entry) => !currentIds.has(entry.id))
      .map((entry) => entry.id),
  };
};

const formatSimilarity = (value) => value.toFixed(6);

const formatDelta = (value) => `${value >= 0 ? "+" : "-"}${Math.abs(value).toFixed(6)}`;

const formatSelected = (selected) => (selected ? "selected" : "not selected");

const formatCaseTable = (entry) => {
  const width = Math.max(4, ...entry.cases.map((testCase) => testCase.id.length));
  return [
    `  ${"case".padEnd(width)}  previous  current   delta`,
    ...entry.cases.map(
      (testCase) =>
        `  ${testCase.id.padEnd(width)}  ${formatSimilarity(testCase.previous)}  ${formatSimilarity(testCase.current)}  ${formatDelta(testCase.delta)}`,
    ),
  ];
};

const formatCrossings = (entry) => {
  if (entry.threshold_crossings.length === 0) {
    return ["  threshold crossings: none"];
  }
  return [
    "  threshold crossings:",
    ...entry.threshold_crossings.map(
      (testCase) =>
        `    ${testCase.id}: ${formatSelected(testCase.previous_selected)} -> ${formatSelected(testCase.current_selected)}`,
    ),
  ];
};

const formatOrder = (entry) => {
  if (!entry.order_changed) {
    return ["  case order: unchanged"];
  }
  return [
    "  case order changed:",
    `    previous: ${entry.previous_order.join(", ")}`,
    `    current:  ${entry.current_order.join(", ")}`,
  ];
};

const formatIdList = (label, ids) => (ids.length === 0 ? [] : [`${label}: ${ids.join(", ")}`]);

const formatProfile = (entry) => [
  "",
  `Profile ${entry.id} (threshold ${entry.current_threshold})`,
  ...(entry.previous_threshold === entry.current_threshold
    ? []
    : [`  threshold changed: ${entry.previous_threshold} -> ${entry.current_threshold}`]),
  ...formatCaseTable(entry),
  `  max absolute delta: ${formatSimilarity(entry.max_abs_delta)}`,
  ...formatCrossings(entry),
  ...formatOrder(entry),
  ...formatIdList("  added cases", entry.added_cases),
  ...formatIdList("  removed cases", entry.removed_cases),
];

/** Formats a `compareEvidence` result as a plain text review summary. */
const formatComparison = (comparison) => {
  const lines = [
    `Model evidence comparison: runtime ${comparison.previous_runtime_version} -> ${comparison.current_runtime_version}`,
    ...comparison.profiles.flatMap(formatProfile),
  ];
  for (const [label, ids] of [
    ["Added profiles", comparison.added_profiles],
    ["Removed profiles", comparison.removed_profiles],
  ]) {
    if (ids.length > 0) {
      lines.push("", ...formatIdList(label, ids));
    }
  }
  return `${lines.join("\n")}\n`;
};

const main = async () => {
  const options = parseArgs(process.argv.slice(2));
  const previous =
    options.compare === null ? null : JSON.parse(readFileSync(resolve(options.compare), "utf8"));
  const evidence = await generateEvidence(options);
  process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
  if (previous !== null) {
    process.stderr.write(formatComparison(compareEvidence(previous, evidence)));
  }
};

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}

export { compareEvidence, formatComparison, parseArgs };
