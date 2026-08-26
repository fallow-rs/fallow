#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const POLICY_PATH = resolve(REPO_ROOT, "tools/similar-code-sidecar/audit-allowlist.json");
const SIDECAR_MANIFEST = resolve(REPO_ROOT, "tools/similar-code-sidecar/Cargo.toml");
const EXPECTED_SCHEMA = "fallow-similar-code-audit-allowlist/v1";
const EXPECTED_ADVISORY = "RUSTSEC-2024-0436";
const SHA_PATTERN = /^[0-9a-f]{40}$/u;
const CHECKSUM_PATTERN = /^[0-9a-f]{64}$/u;
const SIDECAR_PACKAGE = "fallow-similar-code-sidecar";
const EXPECTED_REVERSE_EDGES = new Set([
  "paste v1.0.15 -> gemm v0.19.0",
  "paste v1.0.15 -> gemm-c32 v0.19.0",
  "paste v1.0.15 -> gemm-c64 v0.19.0",
  "paste v1.0.15 -> gemm-common v0.19.0",
  "paste v1.0.15 -> gemm-f16 v0.19.0",
  "paste v1.0.15 -> gemm-f32 v0.19.0",
  "paste v1.0.15 -> gemm-f64 v0.19.0",
  "paste v1.0.15 -> pulp v0.22.3",
  "paste v1.0.15 -> tokenizers v0.22.2",
  "gemm v0.19.0 -> candle-core v0.11.0",
  "gemm-c32 v0.19.0 -> gemm v0.19.0",
  "gemm-c64 v0.19.0 -> gemm v0.19.0",
  "gemm-common v0.19.0 -> gemm v0.19.0",
  "gemm-common v0.19.0 -> gemm-c32 v0.19.0",
  "gemm-common v0.19.0 -> gemm-c64 v0.19.0",
  "gemm-common v0.19.0 -> gemm-f16 v0.19.0",
  "gemm-common v0.19.0 -> gemm-f32 v0.19.0",
  "gemm-common v0.19.0 -> gemm-f64 v0.19.0",
  "gemm-f16 v0.19.0 -> gemm v0.19.0",
  "gemm-f32 v0.19.0 -> gemm v0.19.0",
  "gemm-f32 v0.19.0 -> gemm-f16 v0.19.0",
  "gemm-f64 v0.19.0 -> gemm v0.19.0",
  "pulp v0.22.3 -> gemm-common v0.19.0",
  "tokenizers v0.22.2 -> candle-core v0.11.0",
  `tokenizers v0.22.2 -> ${SIDECAR_PACKAGE}`,
  "candle-core v0.11.0 -> candle-nn v0.11.0",
  "candle-core v0.11.0 -> candle-transformers v0.11.0",
  `candle-core v0.11.0 -> ${SIDECAR_PACKAGE}`,
  "candle-nn v0.11.0 -> candle-transformers v0.11.0",
  `candle-nn v0.11.0 -> ${SIDECAR_PACKAGE}`,
  `candle-transformers v0.11.0 -> ${SIDECAR_PACKAGE}`,
]);

const fail = (message) => {
  throw new Error(message);
};

const validatePolicy = (policy, today = new Date().toISOString().slice(0, 10)) => {
  if (policy.$schema !== EXPECTED_SCHEMA) {
    fail(`unsupported similar-code audit allowlist schema: ${policy.$schema}`);
  }
  if (policy.owner !== "tools/similar-code-sidecar") {
    fail("similar-code audit allowlist must have one narrow owner");
  }
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(policy.review_by) || today > policy.review_by) {
    fail(`similar-code audit allowlist expired on ${policy.review_by}`);
  }
  if (
    !Number.isSafeInteger(policy.release_binary_max_bytes) ||
    policy.release_binary_max_bytes < 1
  ) {
    fail("similar-code audit allowlist requires a positive release binary size ceiling");
  }
  if (!Array.isArray(policy.advisories) || policy.advisories.length !== 1) {
    fail("similar-code audit allowlist must contain exactly one advisory");
  }
  const advisory = policy.advisories[0];
  if (
    advisory.id !== EXPECTED_ADVISORY ||
    advisory.package !== "paste" ||
    advisory.version !== "1.0.15" ||
    advisory.kind !== "unmaintained"
  ) {
    fail("similar-code audit allowlist must only own paste RUSTSEC-2024-0436");
  }
  if (!advisory.reason || !advisory.removal_condition) {
    fail("similar-code audit allowlist requires a reason and removal condition");
  }
  if (
    !Array.isArray(advisory.approved_chains) ||
    advisory.approved_chains.length !== 2 ||
    !advisory.approved_chains.some((chain) => chain.includes("gemm 0.19.0")) ||
    !advisory.approved_chains.some((chain) => chain.includes("tokenizers 0.22.2"))
  ) {
    fail("similar-code audit allowlist must name both approved paste chains");
  }
  const evidence = advisory.upstream_evidence;
  if (evidence?.checked_on !== "2026-08-26") {
    fail("similar-code audit upstream evidence date is missing or changed");
  }
  for (const record of [evidence.candle_release, evidence.tokenizers_upstream]) {
    if (
      !SHA_PATTERN.test(record?.release_revision) ||
      !SHA_PATTERN.test(record?.main_revision) ||
      !record.release_source?.includes(record.release_revision) ||
      !record.main_source?.includes(record.main_revision)
    ) {
      fail("similar-code audit evidence must pin release and current official source revisions");
    }
  }
  if (
    !SHA_PATTERN.test(evidence.gemm_upstream?.main_revision) ||
    !evidence.gemm_upstream?.main_source?.includes(evidence.gemm_upstream.main_revision)
  ) {
    fail("gemm audit evidence must pin the current official source revision");
  }
  for (const record of [
    evidence.candle_release,
    evidence.gemm_upstream,
    evidence.tokenizers_upstream,
  ]) {
    if (
      !CHECKSUM_PATTERN.test(record?.release_checksum) ||
      !record.crates_io_source?.startsWith("https://crates.io/api/v1/crates/")
    ) {
      fail("similar-code audit evidence must pin the latest crates.io release checksum");
    }
  }
  return policy;
};

const dependencyIdentity = ({ name, version }) => {
  if (name === SIDECAR_PACKAGE) return name;
  return `${name} v${version}`;
};

const parseDependencyTree = (tree) => {
  const lines = tree.trim().split(/\r?\n/u);
  if (lines.length === 0 || lines[0].length === 0) {
    fail("similar-code paste dependency tree is empty");
  }
  const stack = [];
  const edges = new Set();
  for (const [index, line] of lines.entries()) {
    const match = /^(\d+)([A-Za-z0-9_.-]+) v([^\s]+)(?:\s|$)/u.exec(line);
    if (!match) {
      fail(`similar-code paste dependency tree has an unparseable line: ${line}`);
    }
    const depth = Number(match[1]);
    const node = { name: match[2], version: match[3] };
    if (index === 0) {
      if (depth !== 0 || dependencyIdentity(node) !== "paste v1.0.15") {
        fail("similar-code dependency tree must start with exact paste v1.0.15");
      }
    } else if (depth === 0 || depth > stack.length) {
      fail(`similar-code paste dependency tree has invalid depth at: ${line}`);
    } else {
      edges.add(`${dependencyIdentity(stack[depth - 1])} -> ${dependencyIdentity(node)}`);
    }
    stack[depth] = node;
    stack.length = depth + 1;
  }
  return edges;
};

const validateDependencyTree = (tree) => {
  const actualEdges = parseDependencyTree(tree);
  for (const edge of actualEdges) {
    if (!EXPECTED_REVERSE_EDGES.has(edge)) {
      fail(`similar-code paste dependency tree has an unapproved reverse edge: ${edge}`);
    }
  }
  for (const edge of EXPECTED_REVERSE_EDGES) {
    if (!actualEdges.has(edge)) {
      fail(`similar-code paste dependency tree changed: missing reverse edge ${edge}`);
    }
  }
};

const checkPolicy = ({ run = spawnSync, today } = {}) => {
  const policy = validatePolicy(JSON.parse(readFileSync(POLICY_PATH, "utf8")), today);
  const result = run(
    "cargo",
    [
      "tree",
      "--manifest-path",
      SIDECAR_MANIFEST,
      "--edges",
      "normal",
      "--prefix",
      "depth",
      "-i",
      "paste",
    ],
    { cwd: REPO_ROOT, encoding: "utf8", maxBuffer: 4 * 1024 * 1024, timeout: 60_000 },
  );
  if (result.error) {
    fail(`failed to inspect similar-code dependencies: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`failed to inspect similar-code dependencies: ${result.stderr.trim()}`);
  }
  validateDependencyTree(result.stdout);
  return policy;
};

const main = () => {
  const policy = checkPolicy();
  process.stdout.write(
    `similar-code audit allowlist valid through ${policy.review_by}; remove it when both paste chains disappear\n`,
  );
};

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

export { checkPolicy, parseDependencyTree, validateDependencyTree, validatePolicy };
