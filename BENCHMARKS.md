# Benchmark Methodology

This document describes how fallow's performance benchmarks are structured, how to reproduce them, and how to interpret results.

## Overview

Fallow uses two benchmark layers:

1. **Criterion (Rust)**: Microbenchmarks for regression detection in CI. Measures individual pipeline stages and full end-to-end analysis at various project sizes (10, 100, 1000, 5000 files).
2. **End-to-end (Node.js)**: Wall-clock timings and peak memory of the fallow CLI on synthetic and real-world projects.

## Project Sizes

| Size    | Files | Purpose                          |
|---------|------:|----------------------------------|
| tiny    |    10 | Baseline / startup overhead      |
| small   |    50 | Small library                    |
| medium  |   200 | Typical module                   |
| large   | 1,000 | Monorepo package / mid-size app  |
| xlarge  | 5,000 | Large monorepo / enterprise app  |

Synthetic projects use deterministic seeding (Mulberry32, seed `42 + fileCount`) for reproducibility across runs and machines. Each project includes a realistic mix of TypeScript constructs: interfaces, types, functions, constants, and import graphs with ~80% used / ~20% dead code.

## What Is Measured

### Check (dead code analysis)

Full pipeline: file discovery → parallel Oxc parsing → import resolution → module graph construction → re-export chain propagation → dead code detection.

### Dupes (code duplication)

Full pipeline: file discovery → tokenization → normalization → suffix array construction → LCP computation → clone extraction → family grouping.

### Circular (circular dependency detection)

Full pipeline: file discovery → parallel Oxc parsing → import resolution → module graph construction → Tarjan's SCC algorithm.

### Cache Modes

- **Cold cache** (`--no-cache`): No cache read or write. Measures raw analysis speed.
- **Warm cache**: Cache populated by a prior run. Measures incremental analysis speed where file content hashes match cached results, skipping re-parsing.

## Metrics Collected

| Metric | Source | Description |
|--------|--------|-------------|
| Wall time | `performance.now()` / Criterion | End-to-end elapsed time |
| Peak RSS | `/usr/bin/time -l` (macOS) or `-v` (Linux) | Maximum resident set size |
| Issue count | JSON output parsing | Correctness cross-check |
| Min/Max/Mean/Median | Statistical aggregation | Distribution characterization |

## Reproducing Benchmarks

### Prerequisites

```bash
# Rust toolchain (stable)
rustup update stable

# Node.js (for end-to-end benchmarks)
cd benchmarks && npm install
```

### Criterion Benchmarks

```bash
# All benchmarks (both standard and large-scale)
cargo bench

# Only standard benchmarks (fast)
cargo bench --bench analysis

# Only large-scale benchmarks (1000+ files, slower)
cargo bench --bench large_analysis
```

Large-scale benchmarks use `sample_size(10)` and `measurement_time(60s)` to accommodate longer iteration times.

### End-to-end Benchmarks

```bash
cd benchmarks

# Generate synthetic fixtures (required once)
npm run generate           # check fixtures (tiny → xlarge)
npm run generate:dupes     # dupes fixtures (tiny → xlarge)
npm run generate:circular  # circular dep fixtures (tiny → xlarge)

# Download real-world projects (required once)
npm run download-fixtures  # preact, fastify, zod, vue-core, svelte, query, vite, next.js

# Run benchmarks
npm run bench              # dead code (all fixtures)
npm run bench:synthetic    # synthetic only
npm run bench:real-world   # real-world only
npm run bench:dupes        # duplication (all fixtures)
npm run bench:circular     # circular dependencies (all fixtures)

# Customize runs
npm run bench -- --runs=10 --warmup=3
```

From the repository root, the equivalent setup command is:

```bash
npm --prefix benchmarks run download-fixtures
```

It populates the intentionally untracked
`benchmarks/fixtures/real-world` corpus used by reviewer and panel workflows.

### Output

Benchmark scripts print:
1. **Environment info**: CPU model, core count, RAM, OS, Node/Rust versions
2. **Per-project tables**: cold cache and warm cache timings with memory usage
3. **Summary table**: all projects with timings and peak RSS

## Interpreting Results

- **Median** is the primary metric (robust to outliers).
- **Min** indicates best-case (OS caches warm, no contention).
- **Max** indicates worst-case (cold OS caches, contention).
- **Cache speedup** shows the ratio of cold-to-warm median times. Values > 1.5x indicate significant parsing savings from caching.
- **Peak RSS** measures maximum memory usage. Lower is better for CI environments with constrained memory.

## Hardware Considerations

Benchmark results vary with hardware. Key factors:

- **CPU core count**: fallow uses rayon for parallel parsing. More cores = faster cold cache analysis.
- **Disk speed**: SSD vs HDD significantly affects file discovery and first-read performance.
- **Available RAM**: Large projects (5000+ files) with duplication detection can use several hundred MB.

When publishing results, always include the environment info printed by the benchmark scripts.

## Reference Results (2026-06-19)

Environment: Apple M5 (10 cores), 32 GB RAM, macOS 26.4, Node v22.22.1, rustc 1.95.0. fallow 2.100.0. Real-world fixtures, cold runs (no cache), median of 5, 2 warmup. Warm (cached) runs are faster again.

### Dead code: `fallow dead-code`

| Project | Files | Time | Peak RSS |
|---------|------:|-----:|---------:|
| astro | 2,859 | 3.76s | 873.1 MB |
| fastify | 286 | 64ms | 53.5 MB |
| next.js | 20,558 | 2.95s | 513.1 MB |
| preact | 244 | 74ms | 40.5 MB |
| TanStack/query | 901 | 560ms | 228.4 MB |
| svelte | 3,337 | 611ms | 128.4 MB |
| TypeScript | 38,146 | 2.22s | 494.2 MB |
| vite | 1,420 | 595ms | 102.8 MB |
| vue/core | 522 | 138ms | 71.7 MB |
| zod | 174 | 47ms | 39.1 MB |

### Duplication: `fallow dupes`

| Project | Files | Time | Peak RSS |
|---------|------:|-----:|---------:|
| astro | 2,859 | 549ms | 199.5 MB |
| fastify | 286 | 90ms | 105.3 MB |
| next.js | 20,552 | 12.66s | 981.9 MB |
| preact | 244 | 58ms | 67.0 MB |
| TanStack/query | 901 | 133ms | 131.9 MB |
| svelte | 3,337 | 317ms | 124.5 MB |
| TypeScript | 38,146 | 13.45s | 1.94 GB |
| vite | 1,420 | 174ms | 93.1 MB |
| vue/core | 522 | 109ms | 149.1 MB |
| zod | 174 | 54ms | 62.5 MB |

### Circular dependencies: `fallow dead-code --circular-deps`

The time covers the full pipeline: discovery, parsing, graph building, and cycle detection.

| Project | Files | Time | Cycles | Peak RSS |
|---------|------:|-----:|-------:|---------:|
| astro | 2,859 | 3.81s | 42 | 842.8 MB |
| fastify | 286 | 97ms | 20 | 50.7 MB |
| next.js | 20,552 | 3.00s | 178 | 474.5 MB |
| preact | 244 | 75ms | 5 | 39.8 MB |
| TanStack/query | 901 | 557ms | 0 | 229.7 MB |
| svelte | 3,337 | 595ms | 39 | 123.8 MB |
| TypeScript | 38,146 | 2.18s | 114 | 519.0 MB |
| vite | 1,420 | 581ms | 66 | 101.6 MB |
| vue/core | 522 | 137ms | 58 | 72.6 MB |
| zod | 174 | 43ms | 0 | 38.4 MB |

## CI Integration

The `.github/workflows/bench.yml` workflow runs the Rust benchmarks under CodSpeed on pushes to main and on pull requests with the `ci:perf` label. CodSpeed keeps the history and compares a pull request with its base. The workflow measures only the Rust benchmarks, not the end-to-end benchmarks above.

`.github/workflows/bench-cli-instructions.yml` runs the release CLI under CodSpeed CPU simulation on pinned public projects. See [docs/benchmarking.md](docs/benchmarking.md) for details.
