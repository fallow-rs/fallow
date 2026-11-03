import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";

import {
  ACCEPTED_ADVISORY_IDS,
  checkPolicy,
  validateDependencyTree,
  validatePolicy,
} from "./check-similar-code-sidecar-audit.mjs";

const POLICY_PATH = resolve(
  import.meta.dirname,
  "../tools/similar-code-sidecar/audit-allowlist.json",
);
const loadPolicy = () => JSON.parse(readFileSync(POLICY_PATH, "utf8"));
const WORKFLOWS_DIR = resolve(import.meta.dirname, "../.github/workflows");
const RELEASE_WORKFLOWS = ["release.yml", "release-validation.yml"];

const TREE = `0paste v1.0.15 (proc-macro)
1gemm v0.19.0
2candle-core v0.11.1
3candle-nn v0.11.1
4candle-transformers v0.11.1
5fallow-similar-code-sidecar v3.18.0 (/fixture)
4fallow-similar-code-sidecar v3.18.0 (/fixture)
3candle-transformers v0.11.1 (*)
3fallow-similar-code-sidecar v3.18.0 (/fixture)
1gemm-c32 v0.19.0
2gemm v0.19.0 (*)
1gemm-c64 v0.19.0
2gemm v0.19.0 (*)
1gemm-common v0.19.0
2gemm v0.19.0 (*)
2gemm-c32 v0.19.0 (*)
2gemm-c64 v0.19.0 (*)
2gemm-f16 v0.19.0
3gemm v0.19.0 (*)
2gemm-f32 v0.19.0
3gemm v0.19.0 (*)
3gemm-f16 v0.19.0 (*)
2gemm-f64 v0.19.0
3gemm v0.19.0 (*)
1gemm-f16 v0.19.0 (*)
1gemm-f32 v0.19.0 (*)
1gemm-f64 v0.19.0 (*)
1pulp v0.22.3
2gemm-common v0.19.0 (*)
1tokenizers v0.23.2
2candle-core v0.11.1 (*)
2fallow-similar-code-sidecar v3.18.0 (/fixture)`;

test("temporary paste allowlist is narrow and time bounded", () => {
  const policy = validatePolicy(loadPolicy(), "2026-08-26");

  assert.equal(policy.advisories[0].id, "RUSTSEC-2024-0436");
  assert.equal(policy.advisories[0].approved_chains.length, 2);
  assert.throws(() => validatePolicy(policy, "2026-12-01"), /expired/);
});

/** Returns the flag lines of every multi-line `cargo audit` call in a workflow. */
const cargoAuditCalls = (workflow) =>
  [...workflow.matchAll(/cargo audit \\\n((?:\s+--[^\n]+\n)+)/gu)].map((match) => match[1]);

test("release audit ignores exactly the advisories that the allowlist owns", () => {
  const sidecarAudits = RELEASE_WORKFLOWS.flatMap((file) =>
    cargoAuditCalls(readFileSync(resolve(WORKFLOWS_DIR, file), "utf8"))
      .filter((flags) => flags.includes("tools/similar-code-sidecar/Cargo.lock"))
      .map((flags) => ({ file, flags })),
  );
  // Release validation runs before the dispatch and inside release.yml, so
  // a new upstream advisory fails before any version or tag exists.
  assert.deepEqual(
    sidecarAudits.map(({ file }) => file),
    ["release-validation.yml"],
    "release validation must own the one similar-code sidecar audit",
  );
  for (const { file, flags } of sidecarAudits) {
    assert.match(flags, /--file tools\/similar-code-sidecar\/Cargo\.lock/u, file);
    assert.match(flags, /--deny unmaintained/u, file);
    const ignored = [...flags.matchAll(/--ignore (\S+)/gu)].map((match) => match[1]);
    assert.deepEqual(ignored, ACCEPTED_ADVISORY_IDS, file);
  }
  assert.deepEqual(
    loadPolicy().advisories.map(({ id }) => id),
    ACCEPTED_ADVISORY_IDS,
  );
});

test("temporary paste evidence pins released crates and current official revisions", () => {
  const invalidChecksum = structuredClone(loadPolicy());
  invalidChecksum.advisories[0].upstream_evidence.gemm_upstream.release_checksum = "unknown";
  assert.throws(() => validatePolicy(invalidChecksum, "2026-08-26"), /release checksum/);

  const invalidRevision = structuredClone(loadPolicy());
  invalidRevision.advisories[0].upstream_evidence.tokenizers_upstream.main_source =
    "https://github.com/huggingface/tokenizers/blob/main/tokenizers/Cargo.toml";
  assert.throws(() => validatePolicy(invalidRevision, "2026-08-26"), /source revisions/);
});

test("temporary paste allowlist fails when dependency ownership changes", () => {
  validateDependencyTree(TREE);
  assert.throws(
    () => validateDependencyTree(TREE.replace("gemm v0.19.0", "gemm v0.20.0")),
    /unapproved|changed/,
  );
  assert.throws(
    () => validateDependencyTree(TREE.replace("tokenizers v0.23.2", "tokenizers v0.24.0")),
    /unapproved|changed/,
  );
});

test("temporary paste allowlist rejects an adversarial third reverse dependency chain", () => {
  const adversarial = `${TREE}\n1unapproved-macro-consumer v9.9.9\n2fallow-similar-code-sidecar v3.18.0 (/fixture)`;

  assert.throws(() => validateDependencyTree(adversarial), /unapproved reverse edge.*consumer/u);
});

test("policy check uses the actual cargo dependency tree contract", () => {
  const inspected = [];
  const policy = checkPolicy({
    today: "2026-08-26",
    run: (_command, args) => {
      assert.ok(
        args.includes("tools/similar-code-sidecar/Cargo.toml") ||
          args.some((arg) => arg.endsWith("tools/similar-code-sidecar/Cargo.toml")),
      );
      assert.deepEqual(args.slice(-4, -1), ["--prefix", "depth", "-i"]);
      const packageName = args.at(-1);
      inspected.push(packageName);
      return { error: null, status: 0, stderr: "", stdout: TREE };
    },
  });

  assert.equal(policy.owner, "tools/similar-code-sidecar");
  assert.deepEqual(inspected, ["paste"]);
});

test("check-similar-code-sidecar-audit.mjs reads the cargo tree when Node starts it", () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-sidecar-audit-start-"));
  try {
    // A fake cargo prints the fixture tree, so the run needs no network and no build.
    writeFileSync(join(dir, "paste.txt"), `${TREE}\n`);
    const fakeCargo = join(dir, "cargo");
    writeFileSync(fakeCargo, `#!/bin/sh\nfor last; do :; done\ncat "${dir}/${"$"}{last}.txt"\n`);
    chmodSync(fakeCargo, 0o755);

    const result = spawnSync(process.execPath, ["scripts/check-similar-code-sidecar-audit.mjs"], {
      cwd: resolve(import.meta.dirname, ".."),
      encoding: "utf8",
      env: { PATH: `${dir}${delimiter}${process.env.PATH ?? ""}` },
    });

    // The allowlist has a review date. After that date the run fails with a clear message.
    const { review_by: reviewBy } = loadPolicy();
    const today = new Date().toISOString().slice(0, 10);
    if (today > reviewBy) {
      assert.equal(result.status, 1, result.stderr);
      assert.match(result.stderr, new RegExp(`allowlist expired on ${reviewBy}`, "u"));
    } else {
      assert.equal(result.status, 0, result.stderr);
      assert.match(
        result.stdout,
        new RegExp(
          `^similar-code audit allowlist valid through ${reviewBy} for ${ACCEPTED_ADVISORY_IDS.join(", ")};`,
          "u",
        ),
      );
    }
  } finally {
    rmSync(dir, { force: true, recursive: true });
  }
});
