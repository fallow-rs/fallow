import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import {
  backendExecutableMode,
  getVsixVariant,
  TYPE_AWARE_SIDECAR_FILES,
} from "../editors/vscode/scripts/vsix-targets.mjs";
import {
  EXPECTED_TARGETS,
  computePayload,
  fetchJson,
  marketplaceDownloads,
  openVsxDownload,
  parseChecksums,
  retry,
  validateInventory,
  verifyExtractedPayload,
} from "./vscode-public-verify.mjs";

const VERSION = "3.17.0";
const FIXTURE_ROOT = "scripts/fixtures/vscode-public-verify";

const loadMarketplace = () =>
  JSON.parse(readFileSync(join(FIXTURE_ROOT, "marketplace.json"), "utf8"));

const entryFor = (target) => {
  const variant = getVsixVariant(target);
  return {
    file: `fallow-vscode-${VERSION}-${target}.vsix`,
    target,
    targetPlatform: target === "universal" ? null : target,
    version: VERSION,
    bytes: 100,
    sha256: "1".repeat(64),
    payload: { fileCount: 2, sha256: "2".repeat(64) },
    typescriptVersion: "7.0.0-dev.20260815.1",
    backends: variant.backends.map((backend) => ({
      package: backend.packageName,
      os: backend.os,
      cpu: backend.cpu,
      executable: `dist/type-aware/node_modules/${backend.packageName}/lib/${backend.executable}`,
      bytes: 10,
      sha256: "3".repeat(64),
      mode: backendExecutableMode(backend),
    })),
    sidecar: TYPE_AWARE_SIDECAR_FILES.map(({ path, mode }) => ({
      path,
      bytes: 10,
      sha256: "4".repeat(64),
      mode,
    })),
  };
};

const writeModeRecords = (extension, entry) => {
  for (const record of [
    ...entry.backends.map(({ executable, mode }) => ({ path: executable, mode })),
    ...entry.sidecar,
  ]) {
    const path = join(extension, record.path);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, record.path);
    chmodSync(path, record.mode);
  }
};

const validInventory = () => ({
  schemaVersion: 1,
  extensionVersion: VERSION,
  entries: EXPECTED_TARGETS.map(entryFor),
});

const withTemporaryDirectory = (operation) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-public-verify-test-"));
  try {
    return operation(root);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
};

test("inventory requires the universal-first closed target catalog", () => {
  assert.equal(validateInventory(validInventory()).entries[0].target, "universal");

  const missing = validInventory();
  missing.entries.pop();
  assert.throws(() => validateInventory(missing), /exact target set/u);

  const reordered = validInventory();
  [reordered.entries[0], reordered.entries[1]] = [reordered.entries[1], reordered.entries[0]];
  assert.throws(() => validateInventory(reordered), /exact target set/u);

  const unexpected = validInventory();
  unexpected.entries[6] = { ...unexpected.entries[6], target: "freebsd-x64" };
  assert.throws(() => validateInventory(unexpected), /exact target set/u);
});

test("checksum records reject path traversal and duplicates", () => {
  const hash = "a".repeat(64);
  assert.deepEqual([...parseChecksums(`${hash}  archive.vsix\n`).keys()], ["archive.vsix"]);
  assert.throws(() => parseChecksums(`${hash}  ../archive.vsix\n`), /invalid/u);
  assert.throws(
    () => parseChecksums(`${hash}  archive.vsix\n${hash}  archive.vsix\n`),
    /duplicate/u,
  );
});

test("Marketplace metadata requires every exact version and target tuple", () => {
  const downloads = marketplaceDownloads(loadMarketplace(), VERSION);
  assert.deepEqual([...downloads.keys()], EXPECTED_TARGETS);

  const missing = loadMarketplace();
  missing.results[0].extensions[0].versions.pop();
  assert.throws(() => marketplaceDownloads(missing, VERSION), /exact target set/u);

  const duplicate = loadMarketplace();
  duplicate.results[0].extensions[0].versions.push(
    structuredClone(duplicate.results[0].extensions[0].versions[0]),
  );
  assert.throws(() => marketplaceDownloads(duplicate, VERSION), /duplicate target universal/u);

  const stale = loadMarketplace();
  stale.results[0].extensions[0].versions[0].version = "3.16.0";
  assert.throws(() => marketplaceDownloads(stale, VERSION), /exact target set/u);
});

test("Open VSX exact endpoint metadata rejects universal fallback", () => {
  const metadata = {
    namespace: "fallow-rs",
    name: "fallow-vscode",
    version: VERSION,
    targetPlatform: "linux-x64",
    files: { download: "https://example.invalid/linux-x64.vsix" },
  };
  assert.equal(
    openVsxDownload(metadata, VERSION, "linux-x64"),
    "https://example.invalid/linux-x64.vsix",
  );
  assert.throws(
    () => openVsxDownload({ ...metadata, targetPlatform: "universal" }, VERSION, "linux-x64"),
    /different target/u,
  );
  assert.throws(
    () => openVsxDownload({ ...metadata, version: "3.16.0" }, VERSION, "linux-x64"),
    /version mismatch/u,
  );
});

test("registry metadata rejects rate limits and non-JSON responses", async () => {
  const originalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async () => new Response("rate limited", { status: 429 });
    await assert.rejects(fetchJson("https://example.invalid/metadata"), /HTTP 429/u);

    globalThis.fetch = async () =>
      new Response("<html>not registry metadata</html>", {
        headers: { "content-type": "text/html" },
        status: 200,
      });
    await assert.rejects(fetchJson("https://example.invalid/metadata"), /JSON/u);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("normalized payload verification ignores filesystem metadata but not content", () =>
  withTemporaryDirectory((root) => {
    const extension = join(root, "extension");
    mkdirSync(join(extension, "dist"), { recursive: true });
    writeFileSync(join(extension, "package.json"), '{"name":"fallow-vscode"}\n');
    writeFileSync(join(extension, "dist", "extension.js"), "export {};\n");
    const before = computePayload(root);
    assert.equal(before.fileCount, 2);

    writeFileSync(join(extension, "dist", "extension.js"), "export const changed = true;\n");
    assert.notEqual(computePayload(root).sha256, before.sha256);
  }));

test("semantic payload verification rejects a registry package with changed content", () =>
  withTemporaryDirectory((root) => {
    const extension = join(root, "extension");
    mkdirSync(extension, { recursive: true });
    writeFileSync(
      join(root, "extension.vsixmanifest"),
      `<Identity Id="fallow-vscode" Version="${VERSION}" Publisher="fallow-rs" TargetPlatform="linux-x64" />`,
    );
    writeFileSync(
      join(extension, "package.json"),
      JSON.stringify({ name: "fallow-vscode", publisher: "fallow-rs", version: VERSION }),
    );
    const entry = entryFor("linux-x64");
    writeModeRecords(extension, entry);
    entry.payload = computePayload(root);
    verifyExtractedPayload(root, entry);

    chmodSync(join(extension, entry.sidecar[0].path), 0o644);
    assert.throws(() => verifyExtractedPayload(root, entry), /archived mode mismatch/u);
    chmodSync(join(extension, entry.sidecar[0].path), entry.sidecar[0].mode);
    writeFileSync(join(extension, "package.json"), "{}\n");
    assert.throws(() => verifyExtractedPayload(root, entry), /package publisher mismatch/u);
  }));

test("registry propagation retry succeeds within its bound and rejects exhaustion", async () => {
  let attempts = 0;
  const result = await retry(
    "fixture registry",
    async () => {
      attempts += 1;
      if (attempts < 3) throw new Error("not propagated");
      return "ready";
    },
    { attempts: 3, retryMs: 0 },
  );
  assert.equal(result, "ready");
  assert.equal(attempts, 3);

  await assert.rejects(
    retry("fixture registry", async () => Promise.reject(new Error("still missing")), {
      attempts: 2,
      retryMs: 0,
    }),
    /did not become ready after 2 attempts/u,
  );
});
