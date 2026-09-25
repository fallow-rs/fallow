# Benchmarking

Fallow uses Criterion-compatible Rust benchmarks with CodSpeed simulation in
`.github/workflows/bench.yml`. The workflow is intentionally split into small
shards so PR feedback stays useful and noisy suites do not hide real
regressions.

Simulation jobs run with `RAYON_NUM_THREADS=1`. Rayon splits work adaptively
when a thread steals a job, so with more than one thread the simulated
instruction count changes between runs of the same code. A benchmark takes its
thread count from `bench_threads()` in `crates/benchmarks/benches/support/threads.rs`,
which follows that variable and keeps four threads for a local run.
`scripts/check-benchmark-harness.py` rejects a simulation job without the
variable and a literal thread count in a benchmark file.

Simulation leaves syscall time out of the value. A benchmark that CodSpeed
marks "dominated by syscalls" does not show file-system cost, so do not use it
to judge an I/O change.

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

## Work counters and the process clock

A benchmark measures a code path in process. Two `--performance` fields
measure a whole CLI run, and they need no benchmark harness:

- `counters` holds exact work counts for a dead-code run: source files and
  bytes read, parse cache bytes read, bytes through the CSS comment mask,
  specifier resolutions and distinct specifiers, resolver calls and
  canonicalize calls. These counts do not change with the thread count or
  the machine, so compare two runs with exact equality. A ratio of
  `resolve_specifier_calls` to `unique_specifiers` above 1.0 shows bindings
  that share a specifier. The resolver runs at most once for each distinct
  specifier in a file. A specifier that returns before the resolver, such as
  an external URL, an `npm:` or `jsr:` specifier or a static directory
  mapping, makes no resolver call. Thus `oxc_resolve_calls` minus
  `unique_specifiers` is a net value: the fallback calls are at least this
  difference. The parse masks the comments of
  each stylesheet once, so `css_masked_bytes` is equal to the size of the
  parsed stylesheets. A higher value shows a repeated mask.
- The health timings hold `git_log_bytes`. This count also changes with the
  churn window and the date, because commits move out of a relative window.
  Compare it only for runs on the same day with the same window.
- `process` holds the spans outside the pipeline: startup, thread pool,
  config, git, analysis, the work after the analysis, and output. Only a
  standalone `dead-code` run reports it, because only that command clocks
  its report output. `spans` gives the parent of each stage, so you can see
  which stages run before the `total_ms` clock starts.

Use the counters as the metric when a change removes repeated work. Use the
process spans to find the largest cost outside the pipeline. Measure the wall
time of a release build outside the process as the median of many runs, and
compare it with the sum of the process spans. The process clock starts in
`main` and stops when the report prints. The loader time before `main` and
the teardown after the report are outside it. On a small project, such as
`editors/vscode`, the spans cover about 88% of the external wall time for
this reason.

`performance_counters_are_exact_on_pinned_fixtures` in
`crates/cli/tests/integration/check_tests.rs` pins the counts for three fixtures. Update a
pinned number only when the work changed on purpose, and give the reason in the
commit. Drift invariant I9 checks that the counters do not depend on the
thread count.

## Allocation guard

`.github/workflows/allocs.yml` runs `crates/core/benches/allocations.rs`
under dhat and records four counts: total bytes, total blocks, peak bytes and
peak blocks. The fixture has multi-binding and type-only imports, barrel files
with named, type and star re-exports, a namespace import, a global stylesheet
with an `@import`, a CSS module with an unused class and one Vue single-file
component. The bench asserts that these paths still produce their findings,
so a fixture that stops reaching a path fails the run instead of showing lower
counts.

The workflow sets `RAYON_NUM_THREADS=1`. With one thread the counts differ
between runs by less than 0.02%, so the alert threshold is 101%. With four
threads the peak bytes differ by more than 1% between runs.
`scripts/check-benchmark-harness.py` rejects the workflow without the
variable. Run the bench locally with the same variable:

```bash
RAYON_NUM_THREADS=1 cargo bench -p fallow-core --bench allocations
```

## Shards

Fast PR shards:

- `fallow-core/analysis`: core parser, graph, cache, resolver, and duplicate
  detector paths.
- `fallow-core/entry_point_discovery`: entry-point discovery on a monorepo
  fixture, covering root-package discovery, per-workspace discovery, and
  plugin-glob discovery (triggered by `crates/core/src/discover/` changes).
  The package-shaped probes scale package count rather than source size,
  because that half of the stage is filesystem probing per `package.json`
  entry field. The plugin-glob probes scale pattern count instead: compiling
  the pattern set and matching it against every discovered file is CPU work
  that does no filesystem probing, and measurement on real projects put it at
  roughly 78 to 88 percent of the stage.
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
- `fallow-benchmarks/component_graph`: project-state construction.
- `fallow-benchmarks/component_output`: output envelope serialization and CI
  comment rendering.
- `fallow-benchmarks/lsp_save_publish`: the language server work of one save
  (analysis, diagnostic build, and publish plan) for a cold first run, a save
  after one file changed, and a no-op save. Each case asserts its publish
  count. A fourth case converts the diagnostic columns of one 5,000-line
  file to UTF-16.

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
- Add entry-point discovery probes to `entry_point_discovery`.
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
