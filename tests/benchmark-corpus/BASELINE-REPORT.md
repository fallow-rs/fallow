# Fallow Duplication Accuracy Baseline

**Corpus designed:** 2026-03-18
**Floor last verified:** 2026-09-07 on fallow 3.23.0, reproduced exactly
**Corpus:** Custom curated JS/TS benchmark (`tests/benchmark-corpus/`)

## What these numbers are

The corpus is hand-written by this project: the clones were authored to be
found, and the negative pairs were authored to be missed. Precision and recall
measured against it therefore say nothing about accuracy on real code, and
nothing about how fallow compares to any other tool.

What they do say is whether today's binary still behaves the way it did when
the floor was recorded. `results/accuracy-baseline.json` is that floor, and
`evaluate.sh` exits non-zero when any mode drops below it. Treat it as a
regression tripwire and nothing more. Re-verify with the command under
[Reproducing](#reproducing); the version line above records the last binary the
floor was checked against.

## Methodology

### Corpus Design

14 TypeScript files across 5 categories:

| Category | Files | Description |
|----------|-------|-------------|
| Type-1 (exact) | 4 | Identical copies: DataProcessor class (143 lines), LRUCache class (132 lines) |
| Type-2 (renamed) | 4 | Same structure, all identifiers renamed: HTTP client, Event bus |
| Type-3 (near-miss) | 4 | Added/removed/modified statements: FormValidator, PriorityQueue |
| Type-4 (semantic) | 2 | Same semantics, different implementation: quicksort vs mergesort |
| Negative | 3 | Structurally different: AuthService, FileWatcher, ConnectionPool |

### Ground Truth

- **7 clone pairs** with expected detection per mode
- **5 negative pairs** (should not be detected)
- Each pair has minimum overlap threshold

### Evaluation

- **True Positive (TP):** Ground truth pair detected with >= min_overlap_lines
- **False Positive (FP):** Detection between negative pairs, or Type-4 detection (beyond token-based capability)
- **False Negative (FN):** Expected pair not detected in a mode where it should be
- Settings: `--min-tokens 30 --min-lines 3` (lowered from defaults to capture smaller clone fragments)

## Results

### Summary Table

| Mode | Groups | TP | FP | FN | Precision | Recall | F1 |
|------|--------|----|----|------|-----------|--------|------|
| strict | 28 | 4 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| mild | 28 | 4 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| weak | 29 | 4 | 0 | 0 | 100.0% | 100.0% | 100.0% |
| semantic | 35 | 6 | 2 | 0 | 75.0% | 100.0% | 85.7% |
| defaults | 11 | 4 | 0 | 0 | 100.0% | 100.0% | 100.0% |

> Note: "defaults" uses `--min-tokens 50 --min-lines 5` (production settings).

The semantic row's two false positives are the only place this corpus produces
a wrong answer, so they are worth naming rather than reading as a percentage.
Both are interface declarations that survive identifier blinding:

- **AuthToken vs CacheEntry** (`src/negative/auth-service.ts` against
  `src/type1-exact/cache-manager-a.ts`): 4 to 5 typed fields each, 12 lines of
  overlap once identifiers are blinded.
- **AuthToken vs WatchEvent** (`src/negative/auth-service.ts` against
  `src/negative/file-watcher.ts`): the same shape, 15 lines of overlap.

A third detection, the shared binary-search helper inside the Type-4 sort pair,
is scored as a false positive because Type-4 is out of scope for token
matching, but the two helpers really do share structure. See
[False Positive Analysis](#false-positive-analysis) for the full trace.

### Per Clone Type

| Type | Pairs | Strict | Mild | Weak | Semantic |
|------|-------|--------|------|------|----------|
| Type-1 (exact) | 2 | 2/2 | 2/2 | 2/2 | 2/2 |
| Type-2 (renamed) | 2 | 0/0 expected | 0/0 expected | 0/0 expected | **2/2** |
| Type-3 (near-miss) | 2 | 2/2 | 2/2 | 2/2 | 2/2 |
| Type-4 (semantic) | 1 | 0/0 correct | 0/0 correct | 0/0 correct | 1 (FP) |

### Per-Pair Detail

| Pair ID | Type | Strict | Mild | Weak | Semantic | Overlap |
|---------|------|--------|------|------|----------|---------|
| T1-dataproc | type-1 | TP (141 lines) | TP | TP | TP | Full file |
| T1-cache | type-1 | TP (132 lines) | TP | TP | TP | Full file |
| T2-http | type-2 | TN | TN | TN | **TP (136 lines)** | Full file |
| T2-eventbus | type-2 | TN | TN | TN | **TP (123 lines)** | Full file |
| T3-validator | type-3 | TP (32 lines) | TP | TP | TP (35 lines) | Shared switch cases |
| T3-queue | type-3 | TP (22 lines) | TP | TP | TP (25 lines) | Binary search + updatePriority |
| T4-sort | type-4 | TN | TN | TN | FP (16 lines) | Binary search helper |

## False Positive Analysis

### Semantic Mode (2 FPs)

1. **NEG-auth-vs-cache (Group 20):** Interface declarations with similar `{ string; number; }` field patterns matched when identifiers are blinded. AuthToken vs CacheEntry: both have 4-5 typed fields → structural similarity after blinding. 12 lines overlap.

2. **NEG-auth-vs-watcher (Group 14):** Similar pattern — interface blocks with `string`/`number` fields match when identifiers are blinded. 15 lines overlap.

**Root cause:** Semantic mode blinds all identifiers and literals, which makes structurally similar TypeScript interfaces (sequences of `field: type;` declarations) look identical. This is an inherent trade-off of aggressive normalization.

### Type-4 Sort Detection (1 FP in semantic)

The `binarySearch`/`searchSorted` helper functions share structural similarity after identifier blinding (both are `while` loops with `mid` computation and comparison). This is actually a legitimate Type-2 sub-clone within the Type-4 pair — the binary search functions genuinely have the same structure.

## Key Observations

1. **Strict/Mild equivalence:** Confirmed identical results (expected, since both preserve identifiers with AST-based tokenization; whitespace/comments already absent).

2. **Weak mode:** Nearly identical to strict/mild. String literal blinding found 1 additional group (29 vs 28) but no new cross-file matches. Confirms the corpus has minimal string-literal-only variation.

3. **Semantic mode strengths:** Successfully detects Type-2 renamed clones (the primary use case for identifier blinding). Full-file overlap detected for both HTTP client and Event bus pairs.

4. **Semantic mode weakness:** Over-matches on TypeScript interface declarations. This is predictable — interface bodies become sequences of `IDENT: IDENT;` which are very common.

5. **Fragment granularity:** Strict mode found 28 groups from just 7 pairs because it correctly identifies sub-fragments (individual method matches within larger clone pairs). The Type-3 near-miss pairs generate 5-10 sub-groups each for the shared code blocks.

6. **Default settings are conservative:** With `min-tokens=50, min-lines=5`, only 11 groups are reported — all accurate, no FPs. The defaults are well-calibrated for production use.

## Recommendations

1. **For CI adoption:** Use default settings (`mild`, `min-tokens=50`, `min-lines=5`). Zero FPs in this benchmark.

2. **For deep analysis:** Use `semantic` mode but filter results — interface-only matches are noise. Consider adding heuristic to down-weight pure interface/type alias matches.

3. **Potential improvement:** Add optional `--exclude-interfaces` or `--min-code-ratio` flag to filter groups where > 80% of matched tokens are type declarations.

## Reproducing

```bash
# From project root, against a built binary
npm run check:dupes-accuracy

# Or directly, choosing the binary and keeping the raw JSON
./tests/benchmark-corpus/evaluate.sh \
  --fallow-bin ./target/release/fallow \
  --results-dir /tmp/dupes-eval
```

`evaluate.sh` writes to a scratch directory unless `--results-dir` says
otherwise, scores the run against the committed floor, and exits non-zero when
a mode drops below it. Pass `--update-floor` only after the new numbers are
understood.

Committed floor: `tests/benchmark-corpus/results/accuracy-baseline.json`
