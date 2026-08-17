import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { sha256Buffer, sha256File } from "./vsix-archive.mjs";
import {
  backendExecutableMode,
  TYPE_AWARE_SIDECAR_FILES,
  VSIX_VARIANTS,
  vsixFilename,
} from "./vsix-targets.mjs";

export const INVENTORY_FILENAME = "inventory.json";
export const CHECKSUMS_FILENAME = "SHA256SUMS";
export const INVENTORY_SCHEMA_VERSION = 1;

const SHA256_PATTERN = /^[0-9a-f]{64}$/u;

const requireSha256 = (value, label) => {
  if (!SHA256_PATTERN.test(value)) {
    throw new Error(`${label} must be a lowercase SHA-256 digest`);
  }
};

const validateEntry = (entry, variant, extensionVersion) => {
  const expectedFile = vsixFilename(extensionVersion, variant.target);
  if (entry.file !== expectedFile) {
    throw new Error(`unexpected inventory filename for ${variant.target}: ${entry.file}`);
  }
  if (entry.target !== variant.target || entry.targetPlatform !== variant.targetPlatform) {
    throw new Error(`inventory target metadata does not match ${variant.target}`);
  }
  if (entry.version !== extensionVersion) {
    throw new Error(`inventory version does not match ${extensionVersion} for ${variant.target}`);
  }
  if (!Number.isSafeInteger(entry.bytes) || entry.bytes <= 0 || entry.bytes > variant.maxBytes) {
    throw new Error(`invalid inventory size for ${variant.target}: ${entry.bytes}`);
  }
  requireSha256(entry.sha256, `${variant.target} archive`);
  if (!Number.isSafeInteger(entry.payload?.fileCount) || entry.payload.fileCount <= 0) {
    throw new Error(`invalid payload file count for ${variant.target}`);
  }
  requireSha256(entry.payload.sha256, `${variant.target} payload`);
  if (typeof entry.typescriptVersion !== "string" || entry.typescriptVersion.length === 0) {
    throw new Error(`missing TypeScript version for ${variant.target}`);
  }
  const expectedBackends = variant.backends.map(({ packageName }) => packageName);
  const actualBackends = entry.backends?.map(({ package: packageName }) => packageName);
  if (JSON.stringify(actualBackends) !== JSON.stringify(expectedBackends)) {
    throw new Error(`inventory backend closure does not match ${variant.target}`);
  }
  for (const backend of entry.backends) {
    const descriptor = variant.backends.find(({ packageName }) => packageName === backend.package);
    const expectedExecutable = descriptor
      ? `dist/type-aware/node_modules/${descriptor.packageName}/lib/${descriptor.executable}`
      : null;
    if (
      !descriptor ||
      backend.os !== descriptor.os ||
      backend.cpu !== descriptor.cpu ||
      backend.executable !== expectedExecutable ||
      !Number.isSafeInteger(backend.bytes) ||
      backend.bytes <= 0 ||
      backend.mode !== backendExecutableMode(descriptor)
    ) {
      throw new Error(`invalid backend inventory for ${backend.package}`);
    }
    requireSha256(backend.sha256, `${backend.package} executable`);
  }
  if (!Array.isArray(entry.sidecar) || entry.sidecar.length !== TYPE_AWARE_SIDECAR_FILES.length) {
    throw new Error(`invalid sidecar inventory for ${variant.target}`);
  }
  for (let index = 0; index < TYPE_AWARE_SIDECAR_FILES.length; index += 1) {
    const sidecar = entry.sidecar[index];
    const expected = TYPE_AWARE_SIDECAR_FILES[index];
    if (
      sidecar.path !== expected.path ||
      !Number.isSafeInteger(sidecar.bytes) ||
      sidecar.bytes <= 0 ||
      sidecar.mode !== expected.mode
    ) {
      throw new Error(`invalid sidecar inventory for ${variant.target}`);
    }
    requireSha256(sidecar.sha256, `${sidecar.path} sidecar`);
  }
};

export const validateInventory = (inventory) => {
  if (inventory.schemaVersion !== INVENTORY_SCHEMA_VERSION) {
    throw new Error(`unsupported VSIX inventory schema ${inventory.schemaVersion}`);
  }
  if (typeof inventory.extensionVersion !== "string") {
    throw new Error("VSIX inventory is missing extensionVersion");
  }
  if (!Array.isArray(inventory.entries) || inventory.entries.length !== VSIX_VARIANTS.length) {
    throw new Error(`VSIX inventory must contain exactly ${VSIX_VARIANTS.length} entries`);
  }
  const seenTargets = new Set();
  for (let index = 0; index < VSIX_VARIANTS.length; index += 1) {
    const variant = VSIX_VARIANTS[index];
    const entry = inventory.entries[index];
    if (seenTargets.has(entry.target)) {
      throw new Error(`duplicate VSIX inventory target ${entry.target}`);
    }
    seenTargets.add(entry.target);
    validateEntry(entry, variant, inventory.extensionVersion);
  }
  return inventory;
};

export const createInventory = (extensionVersion, entries) =>
  validateInventory({
    schemaVersion: INVENTORY_SCHEMA_VERSION,
    extensionVersion,
    entries: entries.map(({ file, ...entry }) => ({ file, ...entry })),
  });

export const serializeInventory = (inventory) =>
  `${JSON.stringify(validateInventory(inventory), null, 2)}\n`;

export const writeInventory = (outputDirectory, inventory) => {
  const inventoryPath = join(outputDirectory, INVENTORY_FILENAME);
  writeFileSync(inventoryPath, serializeInventory(inventory));
  const checksumEntries = [
    ...inventory.entries.map(({ file, sha256 }) => ({ file, sha256 })),
    { file: INVENTORY_FILENAME, sha256: sha256File(inventoryPath) },
  ];
  const checksums = `${checksumEntries.map(({ file, sha256 }) => `${sha256}  ${file}`).join("\n")}\n`;
  writeFileSync(join(outputDirectory, CHECKSUMS_FILENAME), checksums);
  return {
    inventoryPath,
    inventorySha256: sha256Buffer(Buffer.from(readFileSync(inventoryPath))),
  };
};
