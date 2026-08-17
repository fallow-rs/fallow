#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  createWriteStream,
  existsSync,
  lstatSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, relative, resolve, sep } from "node:path";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import {
  backendExecutableMode,
  getVsixVariant,
  TYPE_AWARE_SIDECAR_FILES,
  VSIX_VARIANTS,
  vsixFilename,
} from "../editors/vscode/scripts/vsix-targets.mjs";

export const EXPECTED_TARGETS = VSIX_VARIANTS.map(({ target }) => target);

const PUBLISHER = "fallow-rs";
const EXTENSION_NAME = "fallow-vscode";
const EXTENSION_ID = `${PUBLISHER}.${EXTENSION_NAME}`;
const MARKETPLACE_QUERY_URL =
  "https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery?api-version=7.2-preview.1";
const OPEN_VSX_API_ROOT = "https://open-vsx.org/api";
const MARKETPLACE_PACKAGE_ASSET = "Microsoft.VisualStudio.Services.VSIXPackage";
const DEFAULT_ATTEMPTS = 24;
const DEFAULT_RETRY_MS = 15_000;
const REQUEST_TIMEOUT_MS = 120_000;

const sha256 = (value) => createHash("sha256").update(value).digest("hex");

const hashFile = (path) => sha256(readFileSync(path));

const normalizeTarget = (targetPlatform) => targetPlatform || "universal";

const sleep = (durationMs) => new Promise((accept) => setTimeout(accept, durationMs));

const assertHttpsUrl = (value, label) => {
  const url = new URL(value);
  assert.equal(url.protocol, "https:", `${label} must use HTTPS`);
  return url.href;
};

const assertExactTargets = (targets, label) => {
  assert.deepEqual(targets, EXPECTED_TARGETS, `${label} must contain the exact target set`);
};

/** Validate the immutable package inventory produced by the release packaging job. */
export const validateInventory = (inventory) => {
  assert.equal(inventory?.schemaVersion, 1, "inventory schemaVersion must be 1");
  assert.match(
    inventory?.extensionVersion ?? "",
    /^\d+\.\d+\.\d+$/u,
    "inventory extensionVersion must be semantic",
  );
  assert.ok(Array.isArray(inventory.entries), "inventory entries must be an array");
  assertExactTargets(
    inventory.entries.map((entry) => entry.target),
    "inventory",
  );

  for (const [index, entry] of inventory.entries.entries()) {
    const target = EXPECTED_TARGETS[index];
    const targetPlatform = target === "universal" ? null : target;
    assert.equal(entry.targetPlatform, targetPlatform, `${target} targetPlatform mismatch`);
    assert.equal(entry.version, inventory.extensionVersion, `${target} version mismatch`);
    assert.equal(
      entry.file,
      vsixFilename(inventory.extensionVersion, target),
      `${target} filename mismatch`,
    );
    assert.ok(Number.isSafeInteger(entry.bytes) && entry.bytes > 0, `${target} bytes are invalid`);
    assert.match(entry.sha256 ?? "", /^[a-f0-9]{64}$/u, `${target} sha256 is invalid`);
    assert.ok(
      Number.isSafeInteger(entry.payload?.fileCount) && entry.payload.fileCount > 0,
      `${target} payload fileCount is invalid`,
    );
    assert.match(
      entry.payload?.sha256 ?? "",
      /^[a-f0-9]{64}$/u,
      `${target} payload sha256 is invalid`,
    );
    assert.equal(
      entry.typescriptVersion?.length > 0,
      true,
      `${target} TypeScript version is invalid`,
    );
    const variant = getVsixVariant(target);
    assert.equal(
      entry.backends?.length,
      variant.backends.length,
      `${target} backend count mismatch`,
    );
    for (let backendIndex = 0; backendIndex < variant.backends.length; backendIndex += 1) {
      const backend = entry.backends[backendIndex];
      const descriptor = variant.backends[backendIndex];
      assert.equal(backend.package, descriptor.packageName, `${target} backend package mismatch`);
      assert.equal(backend.os, descriptor.os, `${target} backend OS mismatch`);
      assert.equal(backend.cpu, descriptor.cpu, `${target} backend CPU mismatch`);
      assert.equal(
        backend.executable,
        `dist/type-aware/node_modules/${descriptor.packageName}/lib/${descriptor.executable}`,
        `${target} backend path mismatch`,
      );
      assert.equal(
        backend.mode,
        backendExecutableMode(descriptor),
        `${target} backend mode mismatch`,
      );
      assert.match(backend.sha256 ?? "", /^[a-f0-9]{64}$/u, `${target} backend hash is invalid`);
    }
    assert.deepEqual(
      entry.sidecar?.map(({ path, mode }) => ({ path, mode })),
      TYPE_AWARE_SIDECAR_FILES,
      `${target} sidecar contract mismatch`,
    );
    for (const sidecar of entry.sidecar) {
      assert.match(sidecar.sha256 ?? "", /^[a-f0-9]{64}$/u, `${target} sidecar hash is invalid`);
    }
  }

  return inventory;
};

/** Parse the packaging checksum file and reject duplicate or malformed records. */
export const parseChecksums = (source) => {
  const checksums = new Map();
  for (const line of source.trim().split(/\r?\n/u)) {
    const match = line.match(/^([a-f0-9]{64})  ([^/\\]+)$/u);
    assert.ok(match, `invalid SHA256SUMS record: ${line}`);
    assert.ok(!checksums.has(match[2]), `duplicate SHA256SUMS record: ${match[2]}`);
    checksums.set(match[2], match[1]);
  }
  return checksums;
};

/** Check every local artifact before using its inventory as the public reference. */
export const validateArtifactDirectory = (artifactDir) => {
  const inventoryPath = join(artifactDir, "inventory.json");
  const checksumsPath = join(artifactDir, "SHA256SUMS");
  const inventory = validateInventory(JSON.parse(readFileSync(inventoryPath, "utf8")));
  const checksums = parseChecksums(readFileSync(checksumsPath, "utf8"));
  const expectedChecksumFiles = [...inventory.entries.map((entry) => entry.file), "inventory.json"];
  assert.deepEqual(
    [...checksums.keys()].toSorted(),
    expectedChecksumFiles.toSorted(),
    "SHA256SUMS must cover only the VSIX files and inventory",
  );

  for (const entry of inventory.entries) {
    const path = join(artifactDir, entry.file);
    assert.ok(existsSync(path), `missing packaged VSIX: ${entry.file}`);
    assert.equal(statSync(path).size, entry.bytes, `${entry.file} byte size mismatch`);
    assert.equal(hashFile(path), entry.sha256, `${entry.file} sha256 mismatch`);
    assert.equal(checksums.get(entry.file), entry.sha256, `${entry.file} checksum mismatch`);
  }
  assert.equal(
    checksums.get("inventory.json"),
    hashFile(inventoryPath),
    "inventory checksum mismatch",
  );
  return inventory;
};

/** Resolve exact version and target downloads from a Marketplace gallery response. */
export const marketplaceDownloads = (response, version) => {
  const extensions = (response?.results ?? []).flatMap((result) => result.extensions ?? []);
  const matches = extensions.filter(
    (extension) =>
      extension.publisher?.publisherName?.toLowerCase() === PUBLISHER &&
      extension.extensionName === EXTENSION_NAME,
  );
  assert.equal(matches.length, 1, `Marketplace metadata must contain exactly one ${EXTENSION_ID}`);

  const versions = (matches[0].versions ?? []).filter((candidate) => candidate.version === version);
  const downloads = new Map();
  for (const candidate of versions) {
    const target = normalizeTarget(candidate.targetPlatform);
    assert.ok(
      EXPECTED_TARGETS.includes(target),
      `Marketplace returned unexpected target ${target}`,
    );
    assert.ok(!downloads.has(target), `Marketplace returned duplicate target ${target}`);
    const packageFile = (candidate.files ?? []).find(
      (file) => file.assetType === MARKETPLACE_PACKAGE_ASSET,
    );
    assert.ok(packageFile?.source, `Marketplace ${target} is missing its VSIX package`);
    downloads.set(target, assertHttpsUrl(packageFile.source, `Marketplace ${target} download`));
  }
  assertExactTargets(
    EXPECTED_TARGETS.filter((target) => downloads.has(target)),
    "Marketplace metadata",
  );
  return new Map(EXPECTED_TARGETS.map((target) => [target, downloads.get(target)]));
};

/** Validate one exact Open VSX target endpoint without accepting universal fallback. */
export const openVsxDownload = (metadata, version, target) => {
  assert.equal(metadata?.namespace, PUBLISHER, `Open VSX ${target} namespace mismatch`);
  assert.equal(metadata?.name, EXTENSION_NAME, `Open VSX ${target} extension mismatch`);
  assert.equal(metadata?.version, version, `Open VSX ${target} version mismatch`);
  assert.equal(
    normalizeTarget(metadata?.targetPlatform),
    target,
    `Open VSX ${target} returned a different target`,
  );
  assert.ok(metadata?.files?.download, `Open VSX ${target} is missing its VSIX download`);
  return assertHttpsUrl(metadata.files.download, `Open VSX ${target} download`);
};

/** Retry registry propagation checks with a fixed upper bound. */
export const retry = async (label, operation, options = {}) => {
  const attempts = options.attempts ?? DEFAULT_ATTEMPTS;
  const retryMs = options.retryMs ?? DEFAULT_RETRY_MS;
  assert.ok(Number.isSafeInteger(attempts) && attempts > 0, "attempts must be positive");
  assert.ok(Number.isSafeInteger(retryMs) && retryMs >= 0, "retryMs must be non-negative");

  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await operation(attempt);
    } catch (error) {
      lastError = error;
      if (attempt === attempts) break;
      console.log(`${label} is not ready (attempt ${attempt}/${attempts}): ${error.message}`);
      await sleep(retryMs);
    }
  }
  throw new Error(`${label} did not become ready after ${attempts} attempts`, { cause: lastError });
};

export const fetchJson = async (url, options = {}) => {
  const response = await fetch(url, {
    ...options,
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  return response.json();
};

const queryMarketplace = async (version) => {
  const response = await fetchJson(MARKETPLACE_QUERY_URL, {
    method: "POST",
    headers: {
      Accept: "application/json;api-version=7.2-preview.1",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      filters: [
        {
          criteria: [{ filterType: 7, value: EXTENSION_ID }],
          pageNumber: 1,
          pageSize: 1,
          sortBy: 0,
          sortOrder: 0,
        },
      ],
      assetTypes: [MARKETPLACE_PACKAGE_ASSET],
      flags: 3,
    }),
  });
  return marketplaceDownloads(response, version);
};

const queryOpenVsx = async (version) => {
  const downloads = new Map();
  for (const target of EXPECTED_TARGETS) {
    const suffix = target === "universal" ? version : `${target}/${version}`;
    const url = `${OPEN_VSX_API_ROOT}/${PUBLISHER}/${EXTENSION_NAME}/${suffix}`;
    const metadata = await fetchJson(url);
    downloads.set(target, openVsxDownload(metadata, version, target));
  }
  return downloads;
};

const downloadToFile = async (url, destination) => {
  const response = await fetch(url, { signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS) });
  if (!response.ok || !response.body) {
    await response.body?.cancel();
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  await pipeline(response.body, createWriteStream(destination));
};

const walkFiles = (root) => {
  const files = [];
  const visit = (directory) => {
    for (const name of readdirSync(directory).toSorted()) {
      const path = join(directory, name);
      const stat = lstatSync(path);
      if (stat.isDirectory()) {
        visit(path);
      } else {
        assert.ok(stat.isFile(), `payload contains unsupported entry ${path}`);
        files.push(path);
      }
    }
  };
  visit(root);
  return files;
};

/** Compute the signature-independent extension payload digest used by packaging. */
export const computePayload = (extractedRoot) => {
  const extensionRoot = join(extractedRoot, "extension");
  assert.ok(existsSync(extensionRoot), "VSIX is missing its extension payload");
  const records = walkFiles(extensionRoot)
    .map((path) => {
      const content = readFileSync(path);
      const archivePath =
        `extension/${relative(extensionRoot, path).split(sep).join("/")}`.toLowerCase();
      return [archivePath, content.byteLength, sha256(content)];
    })
    .toSorted(([left], [right]) => left.localeCompare(right));
  const digest = createHash("sha256");
  for (const record of records) digest.update(`${JSON.stringify(record)}\n`, "utf8");
  return { fileCount: records.length, sha256: digest.digest("hex") };
};

const manifestAttributes = (manifest) => {
  const identity = manifest.match(/<Identity\s+([^>]+)\/>/u)?.[1];
  assert.ok(identity, "VSIX manifest is missing Identity");
  return new Map(
    [...identity.matchAll(/([A-Za-z]+)="([^"]*)"/gu)].map((match) => [match[1], match[2]]),
  );
};

/** Compare one extracted registry download with the prepared semantic payload. */
export const verifyExtractedPayload = (extractedRoot, entry) => {
  const manifest = readFileSync(join(extractedRoot, "extension.vsixmanifest"), "utf8");
  const identity = manifestAttributes(manifest);
  assert.equal(identity.get("Publisher"), PUBLISHER, `${entry.target} publisher mismatch`);
  assert.equal(identity.get("Id"), EXTENSION_NAME, `${entry.target} extension name mismatch`);
  assert.equal(identity.get("Version"), entry.version, `${entry.target} manifest version mismatch`);
  assert.equal(
    normalizeTarget(identity.get("TargetPlatform")),
    entry.target,
    `${entry.target} manifest target mismatch`,
  );

  const packageJson = JSON.parse(
    readFileSync(join(extractedRoot, "extension", "package.json"), "utf8"),
  );
  assert.equal(packageJson.publisher, PUBLISHER, `${entry.target} package publisher mismatch`);
  assert.equal(packageJson.name, EXTENSION_NAME, `${entry.target} package name mismatch`);
  assert.equal(packageJson.version, entry.version, `${entry.target} package version mismatch`);
  const extensionRoot = resolve(extractedRoot, "extension");
  for (const record of [
    ...entry.backends.map(({ executable, mode }) => ({ path: executable, mode })),
    ...entry.sidecar,
  ]) {
    const filePath = resolve(extensionRoot, record.path);
    assert.ok(
      filePath.startsWith(`${extensionRoot}${sep}`),
      `${record.path} escapes the extension`,
    );
    assert.equal(
      statSync(filePath).mode & 0o777,
      record.mode,
      `${entry.target} archived mode mismatch for ${record.path}`,
    );
  }
  assert.deepEqual(
    computePayload(extractedRoot),
    entry.payload,
    `${entry.target} payload mismatch`,
  );
};

const extractVsix = (archive, destination) => {
  const result = spawnSync("unzip", ["-q", archive, "-d", destination], { encoding: "utf8" });
  assert.equal(result.status, 0, `could not extract ${basename(archive)}: ${result.stderr}`);
};

const verifyDownload = async (registry, url, entry, temporaryRoot, retryOptions) =>
  retry(
    `${registry} ${entry.target} payload`,
    async (attempt) => {
      const archive = join(temporaryRoot, `${registry}-${entry.target}-${attempt}.vsix`);
      const extractedRoot = join(temporaryRoot, `${registry}-${entry.target}-${attempt}`);
      try {
        await downloadToFile(url, archive);
        assert.ok(
          statSync(archive).size <= getVsixVariant(entry.target).maxBytes,
          `${registry} ${entry.target} exceeds its VSIX size limit`,
        );
        extractVsix(archive, extractedRoot);
        verifyExtractedPayload(extractedRoot, entry);
      } finally {
        rmSync(archive, { force: true });
        rmSync(extractedRoot, { force: true, recursive: true });
      }
    },
    retryOptions,
  );

const parseArguments = (arguments_) => {
  const options = {
    artifactDir: null,
    attempts: DEFAULT_ATTEMPTS,
    retryMs: DEFAULT_RETRY_MS,
  };
  for (let index = 0; index < arguments_.length; index += 1) {
    const value = arguments_[index + 1];
    if (arguments_[index] === "--artifact-dir" && value) {
      options.artifactDir = resolve(value);
      index += 1;
    } else if (arguments_[index] === "--attempts" && value) {
      options.attempts = Number(value);
      index += 1;
    } else if (arguments_[index] === "--retry-ms" && value) {
      options.retryMs = Number(value);
      index += 1;
    } else {
      throw new Error(`unknown or incomplete argument: ${arguments_[index]}`);
    }
  }
  assert.ok(options.artifactDir, "--artifact-dir is required");
  return options;
};

const main = async () => {
  const options = parseArguments(process.argv.slice(2));
  const inventory = validateArtifactDirectory(options.artifactDir);
  const retryOptions = { attempts: options.attempts, retryMs: options.retryMs };
  const version = inventory.extensionVersion;

  const [marketplace, openVsx] = await Promise.all([
    retry("VS Code Marketplace metadata", () => queryMarketplace(version), retryOptions),
    retry("Open VSX metadata", () => queryOpenVsx(version), retryOptions),
  ]);

  const temporaryRoot = mkdtempSync(join(tmpdir(), "fallow-vscode-public-"));
  try {
    await Promise.all(
      inventory.entries.flatMap((entry) => [
        verifyDownload(
          "marketplace",
          marketplace.get(entry.target),
          entry,
          temporaryRoot,
          retryOptions,
        ),
        verifyDownload("open-vsx", openVsx.get(entry.target), entry, temporaryRoot, retryOptions),
      ]),
    );
  } finally {
    rmSync(temporaryRoot, { force: true, recursive: true });
  }
  console.log(`Verified ${EXTENSION_ID} ${version} for every public registry target`);
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
