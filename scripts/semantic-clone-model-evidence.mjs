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

const parseArgs = (argv) => {
  const options = {
    manifest: DEFAULT_MANIFEST,
    modelCacheState: "unknown",
    runtimeLock: null,
    transformersModule: null,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--manifest") {
      options.manifest = argv[index + 1] ?? fail("--manifest requires a path");
      index += 1;
    } else if (argument === "--model-cache-state") {
      options.modelCacheState =
        argv[index + 1] ?? fail("--model-cache-state requires cold, warm, or unknown");
      index += 1;
    } else if (argument === "--runtime-lock") {
      options.runtimeLock = argv[index + 1] ?? fail("--runtime-lock requires a path");
      index += 1;
    } else if (argument === "--transformers-module") {
      options.transformersModule = argv[index + 1] ?? fail("--transformers-module requires a path");
      index += 1;
    } else {
      fail(`unknown argument: ${argument}`);
    }
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
  if (runtimeLockData.packages?.[""]?.dependencies?.["@huggingface/transformers"] !== "4.2.0") {
    fail("runtime lock must declare @huggingface/transformers 4.2.0 at its root");
  }
  const lockedTransformers = runtimeLockData.packages?.[TRANSFORMERS_PACKAGE_PATH];
  if (lockedTransformers?.version !== "4.2.0" || typeof lockedTransformers.integrity !== "string") {
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

const main = async () => {
  const options = parseArgs(process.argv.slice(2));
  const evidence = await generateEvidence(options);
  process.stdout.write(`${JSON.stringify(evidence, null, 2)}\n`);
};

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main().catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}

export { parseArgs };
