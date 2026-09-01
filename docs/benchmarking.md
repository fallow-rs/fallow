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

Rust walltime retain gates are a local, manual procedure. Trustworthy
wall-clock numbers for release-LTO builds need dedicated hardware, and a shared
CI runner does not qualify: its variance swamps the five to ten percent deltas
a retain gate decides on, so a green-but-noisy lane would be worse than none.

Run one on a quiet machine, and check the load average first, because a loaded
machine can inflate a single benchmark several times over:

```bash
uptime
cargo codspeed build -p fallow-benchmarks --bench component_cache --features codspeed
codspeed run -m walltime -- cargo codspeed run -p fallow-benchmarks --bench component_cache
```

Measure the base and head commits the same way, and do not compare values
across different benchmarks. The upload succeeds before the follow-up poll
finishes, so a `Waiting for results...` timeout is server-side processing
rather than a failed run: read the numbers from the CodSpeed dashboard instead
of waiting on the poll. Simulation mode cannot run locally on macOS, since its
valgrind executor is unsupported there.

The Rust benchmark crates use CodSpeed's official Criterion compatibility
layer. Keep its major version aligned with `cargo-codspeed` so both simulation
and walltime result collection remain available.

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
  session reuse, warm parse-cache, health-cache, dead-code analysis and compact
  JSON rendering, fix dry-run planning, opt-in security and rule-pack policy
  analysis, list inventory rendering, and Viz HTML payload paths.
- `fallow-benchmarks/representative_sources`: focused source-shape extraction
  probes.
- `fallow-benchmarks/component_cache`: extraction cache store save, store load,
  and cached-module to module-info conversion.
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
