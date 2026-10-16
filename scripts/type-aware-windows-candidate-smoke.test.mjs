import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  assertCommandSucceeded,
  assertSemanticReport,
  assertSmokePreconditions,
  assertStatusReport,
  packedFilename,
  stageReleaseVersion,
  typeAwareMetadata,
} from "./type-aware-windows-candidate-smoke.mjs";

const BACKEND = "7.0.0";
const SCRIPT_PATH = fileURLToPath(
  new URL("./type-aware-windows-candidate-smoke.mjs", import.meta.url),
);

const semanticMetadata = (extra = {}) => ({
  executed: true,
  backend_version: BACKEND,
  candidate_count: 3,
  projects: ["tsconfig.json"],
  ...extra,
});

test("assertSmokePreconditions accepts a complete win32 setup", () => {
  assert.doesNotThrow(() =>
    assertSmokePreconditions({
      platform: "win32",
      candidateBinary: "fallow.exe",
      platformPackage: "@fallow-cli/win32-x64-msvc",
    }),
  );
});

test("assertSmokePreconditions rejects a host that is not win32", () => {
  assert.throws(
    () =>
      assertSmokePreconditions({
        platform: "linux",
        candidateBinary: "fallow",
        platformPackage: "@fallow-cli/linux-x64-gnu",
      }),
    { message: "Windows candidate smoke requires win32" },
  );
});

test("assertSmokePreconditions requires the candidate binary", () => {
  for (const candidateBinary of [undefined, ""]) {
    assert.throws(
      () =>
        assertSmokePreconditions({
          platform: "win32",
          candidateBinary,
          platformPackage: "@fallow-cli/win32-x64-msvc",
        }),
      { message: "FALLOW_CANDIDATE_BIN is required" },
    );
  }
});

test("assertSmokePreconditions requires a known platform package", () => {
  assert.throws(
    () =>
      assertSmokePreconditions({
        platform: "win32",
        candidateBinary: "fallow.exe",
        platformPackage: null,
      }),
    assert.AssertionError,
  );
});

test("assertCommandSucceeded returns stdout for an accepted status", () => {
  assert.equal(assertCommandSucceeded("git", ["status"], { status: 0, stdout: "clean" }), "clean");
  assert.equal(
    assertCommandSucceeded("fallow", ["dead-code"], { status: 1, stdout: "{}" }, [0, 1]),
    "{}",
  );
});

test("assertCommandSucceeded reports the command and its output on failure", () => {
  assert.throws(
    () =>
      assertCommandSucceeded("npm", ["pack", "--json"], {
        status: 2,
        stdout: "out",
        stderr: "err",
      }),
    { message: "npm pack --json failed\nout\nerr" },
  );
});

test("assertCommandSucceeded treats a timeout status of null as a failure", () => {
  assert.throws(() => assertCommandSucceeded("git", ["clone"], { status: null }), {
    message: "git clone failed\n\n",
  });
});

test("packedFilename reads the first tarball of npm pack output", () => {
  const stdout = JSON.stringify([{ filename: "fallow-2.0.0.tgz" }, { filename: "other.tgz" }]);
  assert.equal(packedFilename(stdout), "fallow-2.0.0.tgz");
});

test("packedFilename rejects output that is not JSON", () => {
  assert.throws(() => packedFilename("npm warn"), SyntaxError);
});

test("stageReleaseVersion sets the version and keeps the key order", () => {
  const manifest = { name: "@fallow-cli/win32-x64-msvc", version: "1.0.0", os: ["win32"] };
  const staged = stageReleaseVersion(manifest, "2.0.0");
  assert.equal(
    JSON.stringify(staged),
    '{"name":"@fallow-cli/win32-x64-msvc","version":"2.0.0","os":["win32"]}',
  );
  assert.equal(manifest.version, "1.0.0");
});

test("assertStatusReport accepts the npm-wrapper backend", () => {
  assert.doesNotThrow(() =>
    assertStatusReport(
      { available: true, discovery_source: "npm-wrapper", backend_version: BACKEND },
      BACKEND,
    ),
  );
});

test("assertStatusReport rejects an unavailable, foreign, or stale backend", () => {
  const valid = { available: true, discovery_source: "npm-wrapper", backend_version: BACKEND };
  for (const status of [
    { ...valid, available: false },
    { ...valid, discovery_source: "path" },
    { ...valid, backend_version: "6.0.0" },
  ]) {
    assert.throws(() => assertStatusReport(status, BACKEND), assert.AssertionError);
  }
});

test("typeAwareMetadata reads the standalone and the combined report shape", () => {
  const metadata = semanticMetadata();
  assert.equal(typeAwareMetadata({ _meta: { type_aware: metadata } }), metadata);
  assert.equal(typeAwareMetadata({ _meta: { check: { type_aware: metadata } } }), metadata);
  assert.equal(typeAwareMetadata({}), undefined);
});

test("assertSemanticReport accepts a completed semantic pass", () => {
  assert.doesNotThrow(() =>
    assertSemanticReport({ _meta: { type_aware: semanticMetadata() } }, BACKEND),
  );
});

test("assertSemanticReport rejects a report without a completed semantic pass", () => {
  for (const result of [
    {},
    { _meta: { type_aware: semanticMetadata({ executed: false }) } },
    { _meta: { type_aware: semanticMetadata({ backend_version: "6.0.0" }) } },
    { _meta: { type_aware: semanticMetadata({ candidate_count: 0 }) } },
    { _meta: { check: { type_aware: semanticMetadata({ projects: [] }) } } },
  ]) {
    assert.throws(() => assertSemanticReport(result, BACKEND), assert.AssertionError);
  }
});

// The main guard must start the smoke when Node runs the script. If the guard
// fails to match, the CI step does nothing and still passes.
test(
  "running the script outside win32 starts the smoke and stops at the platform check",
  { skip: process.platform === "win32" },
  () => {
    const result = spawnSync(process.execPath, [SCRIPT_PATH], { encoding: "utf8" });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Windows candidate smoke requires win32/u);
  },
);
