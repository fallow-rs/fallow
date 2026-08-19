#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import process from "node:process";

export const FAST_BENCHMARKS = [
  {
    label: "core analysis",
    cache_key: "core-analysis",
    package: "fallow-core",
    bench: "analysis",
    paths: [
      "crates/core/",
      "crates/config/",
      "crates/extract/",
      "crates/graph/",
      "crates/security/",
      "crates/types/",
    ],
  },
  {
    label: "engine dupes detect",
    cache_key: "engine-dupes-detect",
    package: "fallow-engine",
    bench: "dupes_detect",
    paths: ["crates/engine/", "crates/extract/", "crates/types/"],
  },
  {
    label: "stable programmatic API",
    cache_key: "programmatic-stable",
    package: "fallow-benchmarks",
    bench: "programmatic_stable",
    paths: [
      "crates/api/",
      "crates/benchmarks/benches/programmatic_stable.rs",
      "crates/cli/src/coverage/analyze.rs",
      "crates/cli/src/coverage/mod.rs",
      "crates/cli/src/health/coverage.rs",
      "crates/cli/src/health/mod.rs",
      "crates/cli/src/init.rs",
      "crates/cli/src/inspect.rs",
      "crates/cli/src/json_style.rs",
      "crates/cli/src/lib.rs",
      "crates/cli/src/onboarding.rs",
      "crates/config/",
      "crates/core/",
      "crates/engine/",
      "crates/extract/",
      "crates/graph/",
      "crates/output/",
      "crates/types/",
    ],
  },
  {
    label: "representative sources",
    cache_key: "representative-sources",
    package: "fallow-benchmarks",
    bench: "representative_sources",
    paths: [
      "crates/benchmarks/benches/representative_sources.rs",
      "crates/benchmarks/fixtures/",
      "crates/core/",
      "crates/extract/",
      "crates/types/",
    ],
  },
  {
    label: "config component",
    cache_key: "component-config",
    package: "fallow-benchmarks",
    bench: "component_config",
    paths: ["crates/benchmarks/benches/component_config.rs", "crates/config/"],
  },
  {
    label: "engine component",
    cache_key: "component-engine",
    package: "fallow-benchmarks",
    bench: "component_engine",
    paths: [
      "crates/benchmarks/benches/component_engine.rs",
      "crates/config/",
      "crates/core/",
      "crates/engine/",
      "crates/extract/",
      "crates/graph/",
      "crates/types/",
    ],
  },
  {
    label: "graph component",
    cache_key: "component-graph",
    package: "fallow-benchmarks",
    bench: "component_graph",
    paths: [
      "crates/benchmarks/benches/component_graph.rs",
      "crates/config/",
      "crates/graph/",
      "crates/types/",
    ],
  },
  {
    label: "output component",
    cache_key: "component-output",
    package: "fallow-benchmarks",
    bench: "component_output",
    paths: [
      "crates/benchmarks/benches/component_output.rs",
      "crates/cli/src/cli_report.rs",
      "crates/cli/src/report/",
      "crates/output/",
      "crates/types/",
    ],
  },
];

export const GLOBAL_BENCHMARK_FILES = [
  ".github/actions/setup-rust/",
  ".github/scripts/generate-benchmark-matrix.mjs",
  ".github/scripts/generate-benchmark-matrix.test.mjs",
  ".github/workflows/bench.yml",
  "Cargo.lock",
  "Cargo.toml",
  "crates/benchmarks/Cargo.toml",
  "scripts/check-benchmark-harness.py",
  "rust-toolchain.toml",
];

const printableTarget = ({ label, cache_key, package: packageName, bench }) => ({
  label,
  cache_key,
  package: packageName,
  bench,
});

const pathMatches = (file, pattern) => file === pattern || file.startsWith(pattern);

export const allFastTargets = () => FAST_BENCHMARKS.map(printableTarget);

export const selectFastTargets = (changedFiles) => {
  if (changedFiles === null) {
    return allFastTargets();
  }

  const files = [...new Set(changedFiles)].filter(Boolean).toSorted();
  if (files.length === 0) {
    return [];
  }

  if (files.some((file) => GLOBAL_BENCHMARK_FILES.some((pattern) => pathMatches(file, pattern)))) {
    return allFastTargets();
  }

  return FAST_BENCHMARKS.filter((target) =>
    files.some((file) => target.paths.some((pattern) => pathMatches(file, pattern))),
  ).map(printableTarget);
};

const git = (args) =>
  execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });

const changedFilesFromRange = (range) =>
  git(["diff", "--name-only", "--diff-filter=ACMRTUXB", range])
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

export const changedFilesFromEnvironment = (env = process.env) => {
  if (env.BENCH_CHANGED_FILES !== undefined) {
    return env.BENCH_CHANGED_FILES.split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
  }

  const eventName = env.GITHUB_EVENT_NAME ?? "";
  if (eventName === "workflow_dispatch" || eventName === "merge_group") {
    return null;
  }

  try {
    if (eventName === "pull_request" && env.GITHUB_BASE_REF) {
      git(["fetch", "--no-tags", "--depth=1", "origin", env.GITHUB_BASE_REF]);
      return changedFilesFromRange(`origin/${env.GITHUB_BASE_REF}...HEAD`);
    }

    if (eventName === "push" && env.BENCH_EVENT_BEFORE) {
      return changedFilesFromRange(`${env.BENCH_EVENT_BEFORE}..HEAD`);
    }

    return changedFilesFromRange("HEAD^..HEAD");
  } catch (error) {
    console.error(
      `Could not determine changed files, running all fast benchmarks: ${error.message}`,
    );
    return null;
  }
};

const printMatrix = (targets) => {
  process.stdout.write(`${JSON.stringify(targets)}\n`);
};

export const main = (argv = process.argv.slice(2), env = process.env) => {
  if (argv.includes("--all")) {
    printMatrix(allFastTargets());
    return;
  }

  const changedFiles = changedFilesFromEnvironment(env);
  printMatrix(selectFastTargets(changedFiles));
};

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
