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
const ZIP_ADVISORY_IDS = ["RUSTSEC-2026-0367", "RUSTSEC-2026-0368", "RUSTSEC-2026-0369"];
const zipAdvisory = (policy, id) => policy.advisories.find((advisory) => advisory.id === id);

const TREE = `0paste v1.0.15 (proc-macro)
1gemm v0.19.0
2candle-core v0.11.0
3candle-nn v0.11.0
4candle-transformers v0.11.0
5fallow-similar-code-sidecar v3.18.0 (/fixture)
4fallow-similar-code-sidecar v3.18.0 (/fixture)
3candle-transformers v0.11.0 (*)
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
1tokenizers v0.22.2
2candle-core v0.11.0 (*)
2fallow-similar-code-sidecar v3.18.0 (/fixture)`;

const ZIP_TREE = `0zip v8.6.0
1candle-core v0.11.0
2candle-nn v0.11.0
3candle-transformers v0.11.0
4fallow-similar-code-sidecar v3.18.0 (/fixture)
3fallow-similar-code-sidecar v3.18.0 (/fixture)
2candle-transformers v0.11.0 (*)
2fallow-similar-code-sidecar v3.18.0 (/fixture)`;
const TREES = { paste: TREE, zip: ZIP_TREE };

test("temporary paste allowlist is narrow and time bounded", () => {
  const policy = validatePolicy(loadPolicy(), "2026-08-26");

  assert.equal(policy.advisories[0].id, "RUSTSEC-2024-0436");
  assert.equal(policy.advisories[0].approved_chains.length, 2);
  assert.throws(() => validatePolicy(policy, "2026-12-01"), /expired/);
});

test("temporary zip allowlist owns exactly the three unreachable extraction advisories", () => {
  const policy = validatePolicy(loadPolicy(), "2026-10-17");

  assert.deepEqual(
    policy.advisories.map(({ id }) => id),
    ["RUSTSEC-2024-0436", ...ZIP_ADVISORY_IDS],
  );
  for (const id of ZIP_ADVISORY_IDS) {
    const advisory = zipAdvisory(policy, id);
    assert.equal(advisory.package, "zip");
    assert.equal(advisory.version, "8.6.0");
    assert.equal(advisory.kind, "vulnerability");
    assert.equal(advisory.upstream_evidence.candle_release.zip_requirement, "8.6.0");
    assert.equal(advisory.upstream_evidence.zip_upstream.fixed_in, ">=9.0.0");
  }
});

test("temporary zip allowlist rejects unknown, missing, and changed advisories", () => {
  const unknown = structuredClone(loadPolicy());
  unknown.advisories.push({
    ...zipAdvisory(unknown, "RUSTSEC-2026-0367"),
    id: "RUSTSEC-2026-9999",
  });
  assert.throws(() => validatePolicy(unknown, "2026-10-17"), /must own exactly/u);

  const renamed = structuredClone(loadPolicy());
  zipAdvisory(renamed, "RUSTSEC-2026-0369").id = "RUSTSEC-2026-9999";
  assert.throws(() => validatePolicy(renamed, "2026-10-17"), /must own exactly/u);

  const missing = structuredClone(loadPolicy());
  missing.advisories = missing.advisories.filter(({ id }) => id !== "RUSTSEC-2026-0368");
  assert.throws(() => validatePolicy(missing, "2026-10-17"), /must own exactly/u);

  const duplicate = structuredClone(loadPolicy());
  duplicate.advisories[3] = structuredClone(duplicate.advisories[2]);
  assert.throws(() => validatePolicy(duplicate, "2026-10-17"), /must own exactly/u);

  const wrongVersion = structuredClone(loadPolicy());
  zipAdvisory(wrongVersion, "RUSTSEC-2026-0367").version = "8.5.0";
  assert.throws(() => validatePolicy(wrongVersion, "2026-10-17"), /must own zip 8\.6\.0/u);

  const wrongKind = structuredClone(loadPolicy());
  zipAdvisory(wrongKind, "RUSTSEC-2026-0368").kind = "unmaintained";
  assert.throws(() => validatePolicy(wrongKind, "2026-10-17"), /as vulnerability/u);
});

test("temporary zip allowlist requires a reason, removal condition, chains, and evidence", () => {
  const cases = [
    [(advisory) => delete advisory.reason, /reason and removal condition/u],
    [(advisory) => (advisory.removal_condition = ""), /reason and removal condition/u],
    [(advisory) => advisory.approved_chains.pop(), /three approved zip chains/u],
    [(advisory) => (advisory.upstream_evidence.checked_on = "2026-10-01"), /evidence date/u],
    [
      (advisory) => (advisory.upstream_evidence.candle_release.zip_requirement = "9.0.0"),
      /candle-core zip requirement/u,
    ],
    [
      (advisory) => (advisory.upstream_evidence.candle_release.main_source = "main"),
      /source revisions/u,
    ],
    [
      (advisory) => delete advisory.upstream_evidence.zip_upstream.released_on,
      /fixed zip release/u,
    ],
    [
      (advisory) => (advisory.upstream_evidence.zip_upstream.release_checksum = "unknown"),
      /release checksum/u,
    ],
  ];
  for (const [mutate, pattern] of cases) {
    for (const id of ZIP_ADVISORY_IDS) {
      const policy = structuredClone(loadPolicy());
      mutate(zipAdvisory(policy, id));
      assert.throws(() => validatePolicy(policy, "2026-10-17"), pattern, id);
    }
  }
  assert.throws(() => validatePolicy(loadPolicy(), "2026-12-01"), /expired/u);
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
    () => validateDependencyTree(TREE.replace("tokenizers v0.22.2", "tokenizers v0.23.1")),
    /unapproved|changed/,
  );
});

test("temporary zip allowlist fails when dependency ownership changes", () => {
  validateDependencyTree(ZIP_TREE, "zip");
  assert.throws(
    () => validateDependencyTree(ZIP_TREE.replace("zip v8.6.0", "zip v9.0.4"), "zip"),
    /must start with exact zip v8\.6\.0/u,
  );
  assert.throws(
    () =>
      validateDependencyTree(
        `${ZIP_TREE}\n1unapproved-archive-reader v1.0.0\n2fallow-similar-code-sidecar v3.18.0 (/fixture)`,
        "zip",
      ),
    /unapproved reverse edge.*archive-reader/u,
  );
  assert.throws(
    () => validateDependencyTree(ZIP_TREE.split("\n").slice(0, 5).join("\n"), "zip"),
    /missing reverse edge/u,
  );
  assert.throws(() => validateDependencyTree(TREE, "zip"), /must start with exact zip/u);
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
      return { error: null, status: 0, stderr: "", stdout: TREES[packageName] };
    },
  });

  assert.equal(policy.owner, "tools/similar-code-sidecar");
  assert.deepEqual(inspected, ["paste", "zip"]);
});

test("check-similar-code-sidecar-audit.mjs reads the cargo tree when Node starts it", () => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-sidecar-audit-start-"));
  try {
    // A fake cargo prints the fixture tree, so the run needs no network and no build.
    writeFileSync(join(dir, "paste.txt"), `${TREE}\n`);
    writeFileSync(join(dir, "zip.txt"), `${ZIP_TREE}\n`);
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
