import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  checkPolicy,
  validateDependencyTree,
  validatePolicy,
} from "./check-similar-code-sidecar-audit.mjs";

const POLICY_PATH = resolve(
  import.meta.dirname,
  "../tools/similar-code-sidecar/audit-allowlist.json",
);
const loadPolicy = () => JSON.parse(readFileSync(POLICY_PATH, "utf8"));

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

test("temporary paste allowlist is narrow and time bounded", () => {
  const policy = validatePolicy(loadPolicy(), "2026-08-26");

  assert.equal(policy.advisories[0].id, "RUSTSEC-2024-0436");
  assert.equal(policy.advisories[0].approved_chains.length, 2);
  assert.throws(() => validatePolicy(policy, "2026-12-01"), /expired/);
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

test("temporary paste allowlist rejects an adversarial third reverse dependency chain", () => {
  const adversarial = `${TREE}\n1unapproved-macro-consumer v9.9.9\n2fallow-similar-code-sidecar v3.18.0 (/fixture)`;

  assert.throws(() => validateDependencyTree(adversarial), /unapproved reverse edge.*consumer/u);
});

test("policy check uses the actual cargo dependency tree contract", () => {
  const policy = checkPolicy({
    today: "2026-08-26",
    run: (_command, args) => {
      assert.ok(
        args.includes("tools/similar-code-sidecar/Cargo.toml") ||
          args.some((arg) => arg.endsWith("tools/similar-code-sidecar/Cargo.toml")),
      );
      assert.deepEqual(args.slice(-4), ["--prefix", "depth", "-i", "paste"]);
      return { error: null, status: 0, stderr: "", stdout: TREE };
    },
  });

  assert.equal(policy.owner, "tools/similar-code-sidecar");
});
