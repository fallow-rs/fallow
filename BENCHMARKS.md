# Benchmark Methodology

This document describes how fallow's performance benchmarks are structured, how to reproduce them, and how to interpret results.

## Overview

Fallow uses two benchmark layers:

1. **Criterion (Rust)**: Microbenchmarks for regression detection in CI. Measures individual pipeline stages and full end-to-end analysis at various project sizes (10, 100, 1000, 5000 files).
2. **Comparative (Node.js)**: Wall-clock comparisons against knip (unused code), jscpd (duplication), and madge/dpdm (circular dependencies) on synthetic and real-world projects.

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

# Node.js (for comparative benchmarks)
cd benchmarks && npm install

# Optional: install knip v6 for three-way comparison
cd benchmarks/knip6 && npm install
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

### Comparative Benchmarks

```bash
cd benchmarks

# Generate synthetic fixtures (required once)
npm run generate           # check fixtures (tiny → xlarge)
npm run generate:dupes     # dupes fixtures (tiny → xlarge)
npm run generate:circular  # circular dep fixtures (tiny → xlarge)

# Download real-world projects (required once)
npm run download-fixtures  # preact, fastify, zod, vue-core, svelte, query, vite, next.js

# Run benchmarks (includes knip v6 if installed in benchmarks/knip6/)
npm run bench              # fallow vs knip v5 + v6 (all fixtures)
npm run bench:synthetic    # synthetic only
npm run bench:real-world   # real-world only
npm run bench:dupes        # fallow dupes vs jscpd (all fixtures)
npm run bench:circular     # fallow vs madge + dpdm (all fixtures)

# Customize runs
npm run bench -- --runs=10 --warmup=3
```

### Output

Benchmark scripts print:
1. **Environment info**: CPU model, core count, RAM, OS, Node/Rust versions
2. **Per-project tables**: cold cache, warm cache, and competitor timings with memory usage
3. **Summary table**: all projects with speedup ratios and peak RSS

## Interpreting Results

- **Median** is the primary comparison metric (robust to outliers).
- **Min** indicates best-case (OS caches warm, no contention).
- **Max** indicates worst-case (GC pauses for JS tools, cold OS caches).
- **Cache speedup** shows the ratio of cold-to-warm median times. Values > 1.5x indicate significant parsing savings from caching.
- **Peak RSS** measures maximum memory usage. Lower is better for CI environments with constrained memory.
- **Speedup** is `competitor_median / fallow_median`. Values > 1.0x mean fallow is faster.

## Hardware Considerations

Benchmark results vary with hardware. Key factors:

- **CPU core count**: fallow uses rayon for parallel parsing. More cores = faster cold cache analysis. Single-threaded tools (knip) don't benefit.
- **Disk speed**: SSD vs HDD significantly affects file discovery and first-read performance.
- **Available RAM**: Large projects (5000+ files) with duplication detection can use several hundred MB.

When publishing results, always include the environment info printed by the benchmark scripts.

## Reference Results (2026-06-19)

Environment: Apple M5 (10 cores), 32 GB RAM, macOS 26.4, Node v22.22.1, rustc 1.95.0. fallow 2.100.0, knip 5.87.0, knip 6.6.1, jscpd 5.0.10, madge 8.0.0, dpdm 4.0.1. Real-world fixtures, cold runs, median of 5, 2 warmup.

### Dead code: fallow dead-code vs knip

| Project | Files | fallow | knip v5 | knip v6 | vs v5 | vs v6 | fallow RSS | knip v6 RSS |
|---------|------:|-------:|--------:|--------:|------:|------:|-----------:|------------:|
| astro | 2,859 | 3.76s | 3.91s | 1.21s | knip 1.0x | knip 3.1x | 873.1 MB | 371.4 MB |
| fastify | 286 | 64ms | 903ms | 205ms | fallow 13.7x | fallow 3.2x | 53.5 MB | 105.3 MB |
| next.js | 20,558 | 2.95s | errors* | errors* | n/a | n/a | 513.1 MB | n/a |
| preact | 244 | 74ms | 822ms | 2.01s | fallow 10.3x | fallow 27.1x | 40.5 MB | 107.3 MB |
| TanStack/query | 901 | 560ms | 2.86s | 1.04s | fallow 4.9x | fallow 1.9x | 228.4 MB | 363.6 MB |
| svelte | 3,337 | 611ms | 2.00s | 632ms | fallow 2.6x | fallow 1.0x | 128.4 MB | 233.3 MB |
| TypeScript | 38,146 | 2.22s | 2.84s | 736ms | fallow 1.2x | knip 3.0x | 494.2 MB | 339.2 MB |
| vite | 1,420 | 595ms | errors* | errors* | n/a | n/a | 102.8 MB | n/a |
| vue/core | 522 | 138ms | errors* | errors* | n/a | n/a | 71.7 MB | n/a |
| zod | 174 | 47ms | 614ms | 279ms | fallow 13.0x | fallow 5.9x | 39.1 MB | 160.2 MB |

\* knip (both v5 and v6) exits without valid output on next.js, vite, and vue/core: it fails loading those projects' own config files (jest.config.js, a BOM-prefixed config, and a nested vite.config.ts). fallow analyzes all three. fallow numbers are cold; warm (cached) runs are faster again.

### Duplication: fallow dupes vs jscpd

jscpd's Rust rewrite (5.x) is faster than fallow for raw duplication scanning on every project here. fallow's duplication checker runs inside the broader audit flow (dead code, dependencies, complexity, CSS, framework, security) rather than as a standalone scanner.

| Project | Files | fallow | jscpd | Speedup | fallow RSS | jscpd RSS |
|---------|------:|-------:|------:|--------:|-----------:|----------:|
| astro | 2,859 | 549ms | 189ms | jscpd 2.9x | 199.5 MB | 245.6 MB |
| fastify | 286 | 90ms | 64ms | jscpd 1.4x | 105.3 MB | 95.3 MB |
| next.js | 20,552 | 12.66s | 861ms | jscpd 14.7x | 981.9 MB | 668.7 MB |
| preact | 244 | 58ms | 49ms | jscpd 1.2x | 67.0 MB | 67.8 MB |
| TanStack/query | 901 | 133ms | 96ms | jscpd 1.4x | 131.9 MB | 126.3 MB |
| svelte | 3,337 | 317ms | 172ms | jscpd 1.8x | 124.5 MB | 117.3 MB |
| TypeScript | 38,146 | 13.45s | 4.58s | jscpd 2.9x | 1.94 GB | 4.56 GB |
| vite | 1,420 | 174ms | 74ms | jscpd 2.3x | 93.1 MB | 80.9 MB |
| vue/core | 522 | 109ms | 78ms | jscpd 1.4x | 149.1 MB | 143.1 MB |
| zod | 174 | 54ms | 53ms | jscpd 1.0x | 62.5 MB | 69.5 MB |

### Circular dependencies: fallow dead-code --circular-deps vs madge/dpdm

| Project | Files | fallow | cycles | madge | vs madge | dpdm | vs dpdm | fallow RSS |
|---------|------:|-------:|-------:|------:|---------:|-----:|--------:|-----------:|
| astro | 2,859 | 3.81s | 42 | 170ms | 0.0x | 138ms | 0.0x | 842.8 MB |
| fastify | 286 | 97ms | 20 | 224ms | 2.3x | 165ms | 1.7x | 50.7 MB |
| next.js | 20,552 | 3.00s | 178 | 485ms | 0.2x | 463ms | 0.2x | 474.5 MB |
| preact | 244 | 75ms | 5 | 299ms | 4.0x | 134ms | 1.8x | 39.8 MB |
| TanStack/query | 901 | 557ms | 0 | 169ms | 0.3x | 138ms | 0.2x | 229.7 MB |
| svelte | 3,337 | 595ms | 39 | 165ms | 0.3x | 134ms | 0.2x | 123.8 MB |
| TypeScript | 38,146 | 2.18s | 114 | 5.16s | 2.4x | 136ms | 0.1x | 519.0 MB |
| vite | 1,420 | 581ms | 66 | 165ms | 0.3x | 133ms | 0.2x | 101.6 MB |
| vue/core | 522 | 137ms | 58 | 173ms | 1.3x | 145ms | 1.1x | 72.6 MB |
| zod | 174 | 43ms | 0 | 532ms | 12.4x | 192ms | 4.5x | 38.4 MB |

Note: fallow runs a full analysis pipeline (discovery, parsing, graph building, SCC detection) while madge/dpdm only build an import dependency graph, so this is not a like-for-like comparison. On large monorepos fallow's pipeline overhead dominates; on small-to-medium projects and on TypeScript (vs madge) fallow wins. dpdm reports `?` for cycle counts on these projects, indicating incomplete detection, so its timings are not directly comparable.

### Summary ranges

| Comparison | Speed | Memory |
|:-----------|:------|:-------|
| fallow vs knip v6 | Mixed: fallow up to 27x faster (small/mid projects); knip faster on astro and TypeScript (around 3x); knip cannot analyze next.js, vite, or vue/core here | Generally less, except astro |
| fallow vs jscpd 5.x | jscpd 1.0-14.7x faster (its Rust rewrite); fallow does more per pass | Comparable, except TypeScript (fallow much less) |
| fallow vs madge | fallow faster on small/mid and on TypeScript; madge faster on large monorepos (it does far less) | Mixed |

## CI Integration

The `.github/workflows/bench.yml` workflow runs Criterion benchmarks on PRs and pushes to main (when Rust source files change):

- Results stored on `gh-pages` branch
- 10% regression threshold triggers alerts
- PR comments show benchmark comparisons
- Only measures the Criterion (Rust) benchmarks, not comparative benchmarks
