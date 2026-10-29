import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { assertTypescriptBackendResolvable } from "../src/backend-preflight.mjs";
import { supportsVersion } from "../src/backend-selection.mjs";
import { BACKENDS, BACKEND_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

const DEFAULT_BACKEND_ENV = {};
const withoutBackendSelection = () => {
  const env = { ...process.env };
  delete env.FALLOW_TYPE_AWARE_BACKEND;
  return env;
};

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

test("preflight accepts the sidecar's own install of each backend", () => {
  for (const backend of BACKENDS) {
    assertTypescriptBackendResolvable(undefined, { FALLOW_TYPE_AWARE_BACKEND: backend.family });
  }
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
      () => assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), DEFAULT_BACKEND_ENV),
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
      () => assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), DEFAULT_BACKEND_ENV),
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
      { encoding: "utf8", env: withoutBackendSelection() },
    );
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, /fallow-type-aware: resolved typescript 6\.0\.3/);
    assert.match(result.stderr, /npm ci/);
    assert.doesNotMatch(result.stderr, /ERR_MODULE_NOT_FOUND/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("preflight rejects an unknown backend selection", () => {
  assert.throws(
    () => assertTypescriptBackendResolvable(undefined, { FALLOW_TYPE_AWARE_BACKEND: "tsc" }),
    /unknown FALLOW_TYPE_AWARE_BACKEND value `tsc`; expected one of: typescript-go, tsrs/u,
  );
});

test("preflight names the pinned version when the selected backend is missing", () => {
  const root = makeTree();
  try {
    const tsrs = BACKENDS.find((backend) => backend.family === "tsrs");
    assert.throws(
      () =>
        assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), {
          FALLOW_TYPE_AWARE_BACKEND: "tsrs",
        }),
      new RegExp(
        `the tsrs backend is not installed; install tsrs@${tsrs.version.replaceAll(".", "\\.")}`,
        "u",
      ),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("preflight rejects a selected backend at another version", () => {
  const root = makeTree();
  try {
    const packageDir = path.join(root, "node_modules", "tsrs");
    mkdirSync(packageDir, { recursive: true });
    writeFileSync(
      path.join(packageDir, "package.json"),
      JSON.stringify({ name: "tsrs", version: "0.1.0" }),
    );
    assert.throws(
      () =>
        assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), {
          FALLOW_TYPE_AWARE_BACKEND: "tsrs",
        }),
      /resolved tsrs 0\.1\.0 .*the tsrs backend supports >=0\.18\.0 <1\.0\.0/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

const withFakeTsrs = (version, check) => {
  const root = makeTree();
  try {
    const packageDir = path.join(root, "node_modules", "tsrs");
    mkdirSync(packageDir, { recursive: true });
    writeFileSync(path.join(packageDir, "package.json"), JSON.stringify({ name: "tsrs", version }));
    check(() =>
      assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), {
        FALLOW_TYPE_AWARE_BACKEND: "tsrs",
      }),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("preflight accepts a newer tsrs inside the supported range", () => {
  for (const version of ["0.18.0-ts7.1.0-dev.20260929", "0.20.1-ts7.1.0-beta", "0.99.0"]) {
    withFakeTsrs(version, (preflight) => assert.doesNotThrow(preflight, version));
  }
});

test("preflight rejects a tsrs outside the supported range", () => {
  for (const version of ["0.17.0", "1.0.0-ts8.0.0"]) {
    withFakeTsrs(version, (preflight) =>
      assert.throws(preflight, /the tsrs backend supports >=0\.18\.0 <1\.0\.0/u, version),
    );
  }
});

test("preflight accepts a newer typescript 7 release", () => {
  const root = makeTree();
  try {
    writeFakeTypescript(root, "7.1.0-beta");
    assertTypescriptBackendResolvable(path.join(root, "probe.mjs"), DEFAULT_BACKEND_ENV);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("supported range compares major.minor.patch at its boundaries", () => {
  const tsrs = BACKENDS.find((backend) => backend.family === "tsrs");
  const typescript = BACKENDS[0];
  for (const version of ["0.18.0", "0.20.1+build", "0.99.99"]) {
    assert.ok(supportsVersion(tsrs, version), version);
  }
  for (const version of ["0.17.99", "1.0.0", "0.20", "0.20.1.2", "0.20.1foo", "garbage"]) {
    assert.ok(!supportsVersion(tsrs, version), version);
  }
  for (const version of ["7.0.0", "7.1.0-dev.20261029.1", "7.99.99"]) {
    assert.ok(supportsVersion(typescript, version), version);
  }
  for (const version of ["6.99.99", "8.0.0-beta"]) {
    assert.ok(!supportsVersion(typescript, version), version);
  }
  for (const backend of BACKENDS) {
    assert.ok(supportsVersion(backend, backend.version), backend.family);
  }
});
