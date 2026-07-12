// Tests for update-dockerfile-pins.mjs, the release-time rewrite step that
// keeps the Dockerfile's ARG FALLOW_VERSION and its two per-arch sha256 pins
// in lockstep with a just-published release. Regression coverage for #1817
// (the pins previously drifted for 13 releases before a manual fix in #1805).
//
// Run: node --test .github/scripts/update-dockerfile-pins.test.mjs

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { computeUpdatedDockerfile } from "./update-dockerfile-pins.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./update-dockerfile-pins.mjs", import.meta.url));

const CURRENT_AMD64_SHA = "e0af720a13a1758f982e5dda590e57f633c6c4d2ba79de9b1bc5a952a7dd6766";
const CURRENT_ARM64_SHA = "f615c1ba69073ba8025ac03a6729ad8b8a0334c0c9059b7657cb5c05ee0b0c96";
const NEW_AMD64_SHA = "1".repeat(64);
const NEW_ARM64_SHA = "2".repeat(64);

// Mirrors the real Dockerfile's download stage: one ARG line, a case block
// keyed on TARGETARCH with an asset= line immediately followed by a sha256=
// line per arch. `order` lets tests exercise the case arms in either sequence
// since replacement must be asset-keyed, not position-keyed.
function makeFixture({ order = ["amd64", "arm64"] } = {}) {
  const arms = {
    amd64: [
      "    amd64) \\",
      '      asset="fallow-linux-x64-musl"; \\',
      `      sha256="${CURRENT_AMD64_SHA}"; \\`,
      "      ;; \\",
    ],
    arm64: [
      "    arm64) \\",
      '      asset="fallow-linux-arm64-musl"; \\',
      `      sha256="${CURRENT_ARM64_SHA}"; \\`,
      "      ;; \\",
    ],
  };

  return [
    "FROM debian:bookworm-slim AS download",
    "",
    "ARG FALLOW_VERSION=3.3.0",
    "ARG TARGETARCH",
    "",
    "# The sha256 pins below are bound to FALLOW_VERSION above; bump both together.",
    "RUN set -eux; \\",
    '  case "${TARGETARCH}" in \\',
    ...order.flatMap((arch) => arms[arch]),
    "    *) \\",
    '      echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; \\',
    "      exit 1; \\",
    "      ;; \\",
    "  esac; \\",
    '  curl -fsSL "https://example.invalid/${asset}" -o /usr/local/bin/fallow; \\',
    '  echo "${sha256}  /usr/local/bin/fallow" | sha256sum -c -; \\',
    "  chmod +x /usr/local/bin/fallow",
    "",
  ].join("\n");
}

const NEW_PINS = { version: "3.4.0", amd64Sha: NEW_AMD64_SHA, arm64Sha: NEW_ARM64_SHA };

test("rewrites the ARG line and both sha256 pins", () => {
  const updated = computeUpdatedDockerfile(makeFixture(), NEW_PINS);
  assert.match(updated, /^ARG FALLOW_VERSION=3\.4\.0$/m);
  assert.match(
    updated,
    new RegExp(`asset="fallow-linux-x64-musl";[\\s\\S]*?sha256="${NEW_AMD64_SHA}"`),
  );
  assert.match(
    updated,
    new RegExp(`asset="fallow-linux-arm64-musl";[\\s\\S]*?sha256="${NEW_ARM64_SHA}"`),
  );
});

test("is asset-keyed: survives swapped case-arm order", () => {
  const swapped = makeFixture({ order: ["arm64", "amd64"] });
  const updated = computeUpdatedDockerfile(swapped, NEW_PINS);
  assert.match(
    updated,
    new RegExp(`asset="fallow-linux-x64-musl";[\\s\\S]*?sha256="${NEW_AMD64_SHA}"`),
  );
  assert.match(
    updated,
    new RegExp(`asset="fallow-linux-arm64-musl";[\\s\\S]*?sha256="${NEW_ARM64_SHA}"`),
  );
});

test("rejects a Dockerfile with no ARG FALLOW_VERSION line", () => {
  const noArg = makeFixture().replace(/^ARG FALLOW_VERSION=.*$\n/m, "");
  assert.throws(() => computeUpdatedDockerfile(noArg, NEW_PINS), /found 0/);
});

test("rejects a Dockerfile with a duplicate ARG FALLOW_VERSION line", () => {
  const duped = makeFixture().replace(
    "ARG FALLOW_VERSION=3.3.0",
    "ARG FALLOW_VERSION=3.3.0\nARG FALLOW_VERSION=3.3.0",
  );
  assert.throws(() => computeUpdatedDockerfile(duped, NEW_PINS), /found 2/);
});

test("rejects an asset block missing its sha256 pin", () => {
  const missingSha = makeFixture().replace(`      sha256="${CURRENT_AMD64_SHA}"; \\\n`, "");
  assert.throws(
    () => computeUpdatedDockerfile(missingSha, NEW_PINS),
    /fallow-linux-x64-musl.*no sha256 pin/,
  );
});

test("rejects a sha256 pin with no preceding asset= line", () => {
  const orphanSha = makeFixture().replace('      asset="fallow-linux-x64-musl"; \\\n', "");
  assert.throws(() => computeUpdatedDockerfile(orphanSha, NEW_PINS), /no preceding asset= line/);
});

test("rejects an unexpected asset name", () => {
  const renamed = makeFixture().replace(
    'asset="fallow-linux-x64-musl";',
    'asset="fallow-linux-riscv64-musl";',
  );
  assert.throws(() => computeUpdatedDockerfile(renamed, NEW_PINS), /unexpected asset/);
});

test("rejects a duplicate asset entry", () => {
  const doubled = makeFixture().replace(
    'asset="fallow-linux-arm64-musl";',
    'asset="fallow-linux-x64-musl";',
  );
  assert.throws(() => computeUpdatedDockerfile(doubled, NEW_PINS), /duplicate asset entry/);
});

test("rejects an invalid version argument", () => {
  assert.throws(
    () => computeUpdatedDockerfile(makeFixture(), { ...NEW_PINS, version: "v3.4.0" }),
    /invalid version/,
  );
});

test("rejects an invalid sha256 argument", () => {
  assert.throws(
    () => computeUpdatedDockerfile(makeFixture(), { ...NEW_PINS, amd64Sha: "deadbeef" }),
    /invalid amd64 sha256/,
  );
});

test("rejects a no-op rewrite", () => {
  assert.throws(
    () =>
      computeUpdatedDockerfile(makeFixture(), {
        version: "3.3.0",
        amd64Sha: CURRENT_AMD64_SHA,
        arm64Sha: CURRENT_ARM64_SHA,
      }),
    /no changes/,
  );
});

// CLI-level coverage: the exported function is pure, so these confirm the
// file-touching contract the plan requires (rewrite in place on success,
// untouched on failure) actually holds at the process boundary.
function withTempDockerfile(fn) {
  const work = mkdtempSync(join(tmpdir(), "dockerfile-pins-"));
  const path = join(work, "Dockerfile");
  writeFileSync(path, makeFixture());
  try {
    return fn(path);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

test("CLI: rewrites the file in place on success", () => {
  withTempDockerfile((path) => {
    execFileSync("node", [SCRIPT_PATH, "3.4.0", NEW_AMD64_SHA, NEW_ARM64_SHA, path]);
    const updated = readFileSync(path, "utf8");
    assert.match(updated, /^ARG FALLOW_VERSION=3\.4\.0$/m);
    assert.match(updated, new RegExp(NEW_AMD64_SHA));
  });
});

test("CLI: leaves the file untouched and exits non-zero on a guard failure", () => {
  withTempDockerfile((path) => {
    const before = readFileSync(path, "utf8");
    assert.throws(() =>
      execFileSync("node", [SCRIPT_PATH, "v3.4.0", NEW_AMD64_SHA, NEW_ARM64_SHA, path], {
        stdio: "pipe",
      }),
    );
    assert.equal(readFileSync(path, "utf8"), before);
  });
});
