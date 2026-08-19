# Benchmarking

Fallow uses Criterion-compatible Rust benchmarks with CodSpeed simulation in
`.github/workflows/bench.yml`. The workflow is intentionally split into small
shards so PR feedback stays useful and noisy suites do not hide real
regressions.

The optional TypeScript semantic companion is measured separately with
CodSpeed walltime. Simulation cannot measure the interpreted Node.js process or
its child process, so `tools/type-aware-sidecar/bench/session.mjs` uses the
supported Tinybench integration to track cold Program construction and warm
persistent-session reuse as distinct benchmarks.

Rust walltime retain gates run through the separate, manual
`.github/workflows/bench-rust-walltime.yml` workflow. Its fixed suite choices
cover core analysis, stable programmatic sessions, duplicate detection, focused
source extraction, and config, engine, and output components without accepting
arbitrary commands. Keeping it manual avoids adding release-LTO builds to every
pull request while preserving comparable Linux base/head evidence on CodSpeed's
dedicated macro runners:

```bash
gh workflow run bench-rust-walltime.yml \
  --ref <commit-or-branch> \
  -f workload=analysis
```

The Rust benchmark crates use CodSpeed's official Criterion compatibility
layer. Keep its major version aligned with `cargo-codspeed` so both simulation
and walltime result collection remain available.

Use the same workload on the exact base and head refs, then compare those two
walltime runs in CodSpeed. Do not compare values across workload choices.

Fast PR shards are selected by `.github/scripts/generate-benchmark-matrix.mjs`.
Like Oxc's benchmark workflow, this keeps the tracked surface broad while only
running the shards affected by a given change. Manual and merge-queue runs use
the full fast matrix, and global benchmark or Cargo changes fall back to all
fast shards.

## Shards

Fast PR shards:

- `fallow-core/analysis`: core parser, graph, cache, resolver, and duplicate
  detector paths.
- `fallow-engine/dupes_detect`: duplicate-detection engine paths (triggered by
  `crates/engine/`, `crates/extract/`, and `crates/types/` changes).
- `fallow-benchmarks/programmatic_stable`: deterministic programmatic API,
  session reuse, warm parse-cache, health-cache, fix dry-run planning, and
  opt-in security candidate analysis, list inventory rendering, and Viz HTML
  payload paths.
- `fallow-benchmarks/representative_sources`: focused source-shape extraction
  probes.
- `fallow-benchmarks/component_config`: config loading, resolution, workspace
  discovery, and workspace diagnostics.
- `fallow-benchmarks/component_engine`: typed engine session loading, parser
  reuse, project-analysis artifacts, guard policy resolution, warm symbol trace
  traversal, and suppression inventory analysis.
- `fallow-benchmarks/component_graph`: project-state file, stable-key, and
  workspace lookup operations.
- `fallow-benchmarks/component_output`: output envelope serialization and CI
  comment rendering.

Full main/manual shards:

- `fallow-core/scaling_analysis`: larger synthetic scaling probes.
- `fallow-engine/dupes_pipeline`: full duplicate-detection pipelines at large
  project sizes.

`programmatic_commands` still exists for local walltime investigation, but it
contains git/audit scenarios and must not run in the fast CodSpeed matrix.
Deterministic command paths, including circular-dependency analysis, belong in
`programmatic_stable` so CodSpeed tracks them continuously.
`large_analysis` likewise remains available as a local-only high-cost analysis
suite; its archived identities no longer run in CodSpeed CI.

## Adding Benchmarks

Use the smallest shard that matches the path being measured:

- Add stable API/session/cache coverage to `programmatic_stable`.
- Add source-shape extraction probes to `representative_sources`.
- Add architecture-layer probes to the matching `component_*` shard.
- Add engine-level duplicate-detection probes to `dupes_detect`; keep only
  broad parser, graph, and cache probes in `analysis`.
- Add large synthetic or high-variance probes only to full shards.

Keep benchmark names globally unique across `crates/*/benches/*.rs`.
Benchmarks in `programmatic_stable` must use the `stable_` prefix because they
are part of the fast PR regression signal.

## Validation

Run this before changing benchmark matrices or bench targets:

```bash
node --test .github/scripts/generate-benchmark-matrix.test.mjs
python3 scripts/check-benchmark-harness.py
cargo check -p fallow-benchmarks --benches
cargo check -p fallow-core --benches
```

For local signal, prefer targeted Criterion runs:

```bash
cargo bench -p fallow-benchmarks --bench programmatic_stable <filter> -- --sample-size 10
cargo bench -p fallow-core --bench analysis <filter> -- --sample-size 10
npm run bench --prefix tools/type-aware-sidecar
```

Use CodSpeed CI as the release-grade signal. Local `cargo codspeed` runs are
useful smoke checks, but the GitHub workflow is the source of truth for tracked
performance reports.

For correctness or output-contract release evidence on public projects, use the
separate public smoke conformance lane:

```bash
npm run conformance:public-smoke
```

That lane writes compact summaries under `target/public-smoke-conformance/` and
does not report timing data.
