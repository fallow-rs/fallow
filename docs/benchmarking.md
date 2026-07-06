# Benchmarking

Fallow uses Criterion-compatible Rust benchmarks with CodSpeed simulation in
`.github/workflows/bench.yml`. The workflow is intentionally split into small
shards so PR feedback stays useful and noisy suites do not hide real
regressions.

Fast PR shards are selected by `.github/scripts/generate-benchmark-matrix.mjs`.
Like Oxc's benchmark workflow, this keeps the tracked surface broad while only
running the shards affected by a given change. Manual and merge-queue runs use
the full fast matrix, and global benchmark or Cargo changes fall back to all
fast shards.

## Shards

Fast PR shards:

- `fallow-core/analysis`: core parser, graph, cache, resolver, and duplicate
  detector paths.
- `fallow-benchmarks/programmatic_stable`: deterministic programmatic API,
  session reuse, warm parse-cache, and health-cache paths.
- `fallow-benchmarks/representative_sources`: focused source-shape extraction
  probes.
- `fallow-benchmarks/component_config`: config loading, resolution, workspace
  discovery, and workspace diagnostics.
- `fallow-benchmarks/component_engine`: typed engine session loading, parser
  reuse, and project-analysis artifacts.
- `fallow-benchmarks/component_graph`: project-state file, stable-key, and
  workspace lookup operations.
- `fallow-benchmarks/component_output`: output envelope serialization and CI
  comment rendering.

Full main/manual shards:

- `fallow-core/scaling_analysis`: larger synthetic scaling probes.
- `fallow-core/large_analysis`: broad high-cost analysis probes.

`programmatic_commands` still exists for local walltime investigation, but it
contains git/audit scenarios and must not run in the fast CodSpeed matrix.

## Adding Benchmarks

Use the smallest shard that matches the path being measured:

- Add stable API/session/cache coverage to `programmatic_stable`.
- Add source-shape extraction probes to `representative_sources`.
- Add architecture-layer probes to the matching `component_*` shard.
- Add broad parser, graph, cache, or duplication probes to `analysis`.
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
