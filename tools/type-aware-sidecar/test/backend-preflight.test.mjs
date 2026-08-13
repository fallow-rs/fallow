import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { assertTypescriptBackendResolvable } from "../src/backend-preflight.mjs";
import { BACKEND_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

const makeTree = () => mkdtempSync(path.join(tmpdir(), "fallow-preflight-"));

const writeFakeTypescript = (root, version) => {
  const packageDir = path.join(root, "node_modules", "typescript");
  mkdirSync(packageDir, { recursive: true });
  writeFileSync(
    path.join(packageDir, "package.json"),
    JSON.stringify({ name: "typescript", version }),
  );
  return packageDir;
};

test("preflight accepts the sidecar's own typescript install", () => {
  assertTypescriptBackendResolvable();
});

test("root npm install stays in typescript lockstep with the sidecar", () => {
  const readManifest = (manifestPath) => JSON.parse(readFileSync(manifestPath, "utf8"));
  const repoManifest = readManifest(path.join(sidecarRoot, "..", "..", "package.json"));
  const sidecarManifest = readManifest(path.join(sidecarRoot, "package.json"));
  assert.equal(
    repoManifest.overrides?.typescript,
    sidecarManifest.dependencies.typescript,
    "the repository root overrides typescript so a root-only npm ci resolves a sidecar-compatible install",
  );
});

test("preflight names the version conflict when typescript is too old", () => {
  const root = makeTree();
  try {
    const packageDir = writeFakeTypescript(root, "6.0.3");
    assert.throws(
      () => assertTypescriptBackendResolvable(path.join(root, "probe.mjs")),
      (error) => {
        assert.match(error.message, /resolved typescript 6\.0\.3/);
        assert.ok(error.message.includes(packageDir));
        assert.ok(error.message.includes(BACKEND_VERSION));
        assert.match(error.message, /npm ci/);
        return true;
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("preflight reports a missing typescript install", () => {
  const root = makeTree();
  try {
    assert.throws(
      () => assertTypescriptBackendResolvable(path.join(root, "probe.mjs")),
      (error) => {
        assert.match(error.message, /typescript is not installed/);
        assert.match(error.message, /npm ci/);
        return true;
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("executable exits 2 with an actionable message when only an old typescript resolves", () => {
  const root = makeTree();
  try {
    cpSync(
      path.join(sidecarRoot, "fallow-type-aware.mjs"),
      path.join(root, "fallow-type-aware.mjs"),
    );
    cpSync(path.join(sidecarRoot, "src"), path.join(root, "src"), { recursive: true });
    writeFakeTypescript(root, "6.0.3");
    const result = spawnSync(
      process.execPath,
      [path.join(root, "fallow-type-aware.mjs"), "--status"],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, /fallow-type-aware: resolved typescript 6\.0\.3/);
    assert.match(result.stderr, /npm ci/);
    assert.doesNotMatch(result.stderr, /ERR_MODULE_NOT_FOUND/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
