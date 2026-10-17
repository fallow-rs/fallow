#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const POLICY_PATH = resolve(REPO_ROOT, "tools/similar-code-sidecar/audit-allowlist.json");
const SIDECAR_MANIFEST = resolve(REPO_ROOT, "tools/similar-code-sidecar/Cargo.toml");
const EXPECTED_SCHEMA = "fallow-similar-code-audit-allowlist/v1";
const SHA_PATTERN = /^[0-9a-f]{40}$/u;
const CHECKSUM_PATTERN = /^[0-9a-f]{64}$/u;
const SIDECAR_PACKAGE = "fallow-similar-code-sidecar";
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/u;
const PASTE_REVERSE_EDGES = new Set([
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
const ZIP_REVERSE_EDGES = new Set([
  "zip v8.6.0 -> candle-core v0.11.0",
  "candle-core v0.11.0 -> candle-nn v0.11.0",
  "candle-core v0.11.0 -> candle-transformers v0.11.0",
  `candle-core v0.11.0 -> ${SIDECAR_PACKAGE}`,
  "candle-nn v0.11.0 -> candle-transformers v0.11.0",
  `candle-nn v0.11.0 -> ${SIDECAR_PACKAGE}`,
  `candle-transformers v0.11.0 -> ${SIDECAR_PACKAGE}`,
]);

/** Every crate that owns an accepted advisory, with its exact version and reverse edges. */
const ACCEPTED_PACKAGES = {
  paste: { version: "1.0.15", reverseEdges: PASTE_REVERSE_EDGES },
  zip: { version: "8.6.0", reverseEdges: ZIP_REVERSE_EDGES },
};

/** The only advisories that the release audit may ignore for the similar-code sidecar. */
const ACCEPTED_ADVISORIES = [
  { id: "RUSTSEC-2024-0436", package: "paste", kind: "unmaintained" },
  { id: "RUSTSEC-2026-0367", package: "zip", kind: "vulnerability" },
  { id: "RUSTSEC-2026-0368", package: "zip", kind: "vulnerability" },
  { id: "RUSTSEC-2026-0369", package: "zip", kind: "vulnerability" },
];
export const ACCEPTED_ADVISORY_IDS = ACCEPTED_ADVISORIES.map(({ id }) => id);

const fail = (message) => {
  throw new Error(message);
};

const CRATES_IO_API = "https://crates.io/api/v1/crates/";

const evidenceOf = (advisory, key) => advisory.upstream_evidence?.[key];

const pinsSource = (record, kind) => {
  const revision = record?.[`${kind}_revision`];
  return SHA_PATTERN.test(revision) && Boolean(record[`${kind}_source`]?.includes(revision));
};

const pinsRevisions = (record) => pinsSource(record, "release") && pinsSource(record, "main");

const pinsCratesIo = (record) =>
  CHECKSUM_PATTERN.test(record?.release_checksum) &&
  Boolean(record.crates_io_source?.startsWith(CRATES_IO_API));

const hasChains = (advisory, count, accept) => {
  const chains = advisory.approved_chains;
  return Array.isArray(chains) && chains.length === count && accept(chains);
};

const checkedOn = (date) => (advisory) => evidenceOf(advisory, "checked_on") === date;

const COMMON_RULES = [
  {
    test: (advisory, expected) =>
      advisory.package === expected.package &&
      advisory.version === ACCEPTED_PACKAGES[expected.package].version &&
      advisory.kind === expected.kind,
    message: ({ id, package: name, kind }) =>
      `similar-code audit allowlist entry ${id} must own ${name} ${ACCEPTED_PACKAGES[name].version} as ${kind}`,
  },
  {
    test: (advisory) => Boolean(advisory.reason && advisory.removal_condition),
    message: ({ id }) =>
      `similar-code audit allowlist entry ${id} requires a reason and removal condition`,
  },
];

const PASTE_RULES = [
  {
    test: (advisory) =>
      hasChains(
        advisory,
        2,
        (chains) =>
          chains.some((chain) => chain.includes("gemm 0.19.0")) &&
          chains.some((chain) => chain.includes("tokenizers 0.22.2")),
      ),
    message: () => "similar-code audit allowlist must name both approved paste chains",
  },
  {
    test: checkedOn("2026-08-26"),
    message: () => "similar-code audit upstream evidence date is missing or changed",
  },
  {
    test: (advisory) =>
      ["candle_release", "tokenizers_upstream"].every((key) =>
        pinsRevisions(evidenceOf(advisory, key)),
      ),
    message: () =>
      "similar-code audit evidence must pin release and current official source revisions",
  },
  {
    test: (advisory) => pinsSource(evidenceOf(advisory, "gemm_upstream"), "main"),
    message: () => "gemm audit evidence must pin the current official source revision",
  },
  {
    test: (advisory) =>
      ["candle_release", "gemm_upstream", "tokenizers_upstream"].every((key) =>
        pinsCratesIo(evidenceOf(advisory, key)),
      ),
    message: () => "similar-code audit evidence must pin the latest crates.io release checksum",
  },
];

const ZIP_RULES = [
  {
    test: (advisory) =>
      hasChains(advisory, 3, (chains) =>
        chains.every((chain) => chain.endsWith("candle-core 0.11.0 -> zip 8.6.0")),
      ),
    message: ({ id }) =>
      `similar-code audit allowlist entry ${id} must name the three approved zip chains`,
  },
  {
    test: checkedOn("2026-10-17"),
    message: ({ id }) => `similar-code audit evidence date for ${id} is missing or changed`,
  },
  {
    test: (advisory) => pinsRevisions(evidenceOf(advisory, "candle_release")),
    message: ({ id }) =>
      `zip audit evidence for ${id} must pin release and current official source revisions`,
  },
  {
    test: (advisory) => {
      const candle = evidenceOf(advisory, "candle_release");
      return candle.version === "0.11.0" && candle.zip_requirement === "8.6.0";
    },
    message: ({ id }) => `zip audit evidence for ${id} must record the candle-core zip requirement`,
  },
  {
    test: (advisory) => {
      const zip = evidenceOf(advisory, "zip_upstream");
      return (
        zip?.fixed_in === ">=9.0.0" &&
        /^9\.\d+\.\d+$/u.test(zip.latest_release) &&
        DATE_PATTERN.test(zip.released_on)
      );
    },
    message: ({ id }) =>
      `zip audit evidence for ${id} must record the fixed zip release and its date`,
  },
  {
    test: (advisory) =>
      ["candle_release", "zip_upstream"].every((key) => pinsCratesIo(evidenceOf(advisory, key))),
    message: ({ id }) =>
      `zip audit evidence for ${id} must pin the latest crates.io release checksum`,
  },
];

const PACKAGE_RULES = { paste: PASTE_RULES, zip: ZIP_RULES };

const validateAdvisory = (advisory, expected) => {
  for (const rule of [...COMMON_RULES, ...PACKAGE_RULES[expected.package]]) {
    if (!rule.test(advisory, expected)) fail(rule.message(expected));
  }
};

export const validatePolicy = (policy, today = new Date().toISOString().slice(0, 10)) => {
  if (policy.$schema !== EXPECTED_SCHEMA) {
    fail(`unsupported similar-code audit allowlist schema: ${policy.$schema}`);
  }
  if (policy.owner !== "tools/similar-code-sidecar") {
    fail("similar-code audit allowlist must have one narrow owner");
  }
  if (!DATE_PATTERN.test(policy.review_by) || today > policy.review_by) {
    fail(`similar-code audit allowlist expired on ${policy.review_by}`);
  }
  if (
    !Number.isSafeInteger(policy.release_binary_max_bytes) ||
    policy.release_binary_max_bytes < 1
  ) {
    fail("similar-code audit allowlist requires a positive release binary size ceiling");
  }
  if (!Array.isArray(policy.advisories)) {
    fail("similar-code audit allowlist requires an advisories list");
  }
  const actualIds = policy.advisories.map((advisory) => advisory?.id);
  if (
    actualIds.length !== ACCEPTED_ADVISORY_IDS.length ||
    new Set(actualIds).size !== actualIds.length ||
    !ACCEPTED_ADVISORY_IDS.every((id) => actualIds.includes(id))
  ) {
    fail(
      `similar-code audit allowlist must own exactly ${ACCEPTED_ADVISORY_IDS.join(", ")}; found ${actualIds.join(", ")}`,
    );
  }
  for (const expected of ACCEPTED_ADVISORIES) {
    const advisory = policy.advisories.find(({ id }) => id === expected.id);
    validateAdvisory(advisory, expected);
  }
  return policy;
};

const dependencyIdentity = ({ name, version }) => {
  if (name === SIDECAR_PACKAGE) return name;
  return `${name} v${version}`;
};

export const parseDependencyTree = (tree, packageName = "paste") => {
  const root = `${packageName} v${ACCEPTED_PACKAGES[packageName].version}`;
  const lines = tree.trim().split(/\r?\n/u);
  if (lines.length === 0 || lines[0].length === 0) {
    fail(`similar-code ${packageName} dependency tree is empty`);
  }
  const stack = [];
  const edges = new Set();
  for (const [index, line] of lines.entries()) {
    const match = /^(\d+)([A-Za-z0-9_.-]+) v([^\s]+)(?:\s|$)/u.exec(line);
    if (!match) {
      fail(`similar-code ${packageName} dependency tree has an unparseable line: ${line}`);
    }
    const depth = Number(match[1]);
    const node = { name: match[2], version: match[3] };
    if (index === 0) {
      if (depth !== 0 || dependencyIdentity(node) !== root) {
        fail(`similar-code dependency tree must start with exact ${root}`);
      }
    } else if (depth === 0 || depth > stack.length) {
      fail(`similar-code ${packageName} dependency tree has invalid depth at: ${line}`);
    } else {
      edges.add(`${dependencyIdentity(stack[depth - 1])} -> ${dependencyIdentity(node)}`);
    }
    stack[depth] = node;
    stack.length = depth + 1;
  }
  return edges;
};

export const validateDependencyTree = (tree, packageName = "paste") => {
  const expectedEdges = ACCEPTED_PACKAGES[packageName].reverseEdges;
  const actualEdges = parseDependencyTree(tree, packageName);
  for (const edge of actualEdges) {
    if (!expectedEdges.has(edge)) {
      fail(`similar-code ${packageName} dependency tree has an unapproved reverse edge: ${edge}`);
    }
  }
  for (const edge of expectedEdges) {
    if (!actualEdges.has(edge)) {
      fail(`similar-code ${packageName} dependency tree changed: missing reverse edge ${edge}`);
    }
  }
};

const inspectReverseDependencies = (run, packageName) => {
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
      packageName,
    ],
    { cwd: REPO_ROOT, encoding: "utf8", maxBuffer: 4 * 1024 * 1024, timeout: 60_000 },
  );
  if (result.error) {
    fail(`failed to inspect similar-code dependencies: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`failed to inspect similar-code dependencies: ${result.stderr.trim()}`);
  }
  validateDependencyTree(result.stdout, packageName);
};

export const checkPolicy = ({ run = spawnSync, today } = {}) => {
  const policy = validatePolicy(JSON.parse(readFileSync(POLICY_PATH, "utf8")), today);
  for (const packageName of Object.keys(ACCEPTED_PACKAGES)) {
    inspectReverseDependencies(run, packageName);
  }
  return policy;
};

const main = () => {
  try {
    const { review_by: reviewBy } = checkPolicy();
    process.stdout.write(
      `similar-code audit allowlist valid through ${reviewBy} for ${ACCEPTED_ADVISORY_IDS.join(", ")}; remove each entry when its removal condition holds\n`,
    );
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
};

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) main();
