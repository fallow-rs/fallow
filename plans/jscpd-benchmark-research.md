# jscpd v5 duplication performance research

Written for fallow after rechecking the `jscpd 5.0.10` comparison.

## Summary

jscpd v5 is still much faster than fallow for raw duplication scanning because it uses a different detection strategy.

Fallow currently builds a suffix array and LCP array, then materializes every LCP interval above `min_tokens` into raw clone candidates before later filters reduce the result. On large repos this interval extraction dominates runtime.

jscpd v5 uses rolling `min_tokens` window hashes and an open-clone state machine. It extends contiguous matches while scanning and emits one clone when a match region ends. That avoids materializing the large intermediate LCP interval set.

## Key evidence

### Next.js fallow profile

Command shape:

```bash
RUST_LOG=fallow_core::duplicates=debug \
  ./target/release/fallow dupes \
  --format compact \
  --quiet \
  --performance \
  --summary \
  --no-cache \
  --root benchmarks/fixtures/real-world/next.js
```

Observed step timings:

| Step | Time |
| --- | ---: |
| Tokenize files | about 1.0s |
| Rank reduce | about 6ms |
| Concatenate | about 3ms |
| Suffix array | about 838ms |
| LCP array | about 14ms |
| Extract raw groups | about 11.8s |
| Build final groups | about 279ms |
| Stats | about 6ms |

The run produced `437942` raw groups, then reduced them to `4524` final clone groups.

Main conclusion: fallow is not primarily slow because of parsing, reporting, JSON serialization, or the suffix array itself. The bottleneck is `step5_extract_groups`.

Relevant fallow code:

- `crates/core/src/duplicates/detect/mod.rs`, suffix array and extraction pipeline.
- `crates/core/src/duplicates/detect/extraction.rs`, stack-based LCP interval extraction.
- `crates/core/src/duplicates/detect/filtering.rs`, late subset and line filtering.

### Final private rolling smoke

Current-tree repeated command for the two main parity probes:

```bash
node benchmarks/compare-dupes-rolling.mjs --skip-build --projects=zod,next.js --runs=3 --samples=12
```

Result:

| Project | Default | Rolling | Speedup | Exact drift | Coverage drift |
| --- | ---: | ---: | ---: | ---: | ---: |
| next.js | 12.96s | 6.50s | 1.99x | 7 missing / 11 extra | 3 missing / 3 extra |
| zod | 45ms | 22ms | 2.01x | 0 missing / 0 extra | 0 missing / 0 extra |

Decision: the private rolling path is useful as a performance prototype, but it is not ready to replace the default detector until the remaining `next.js` coverage drift is resolved.

Current-tree broader single-run fixture comparison:

```bash
node benchmarks/compare-dupes-rolling.mjs --skip-build --projects=typescript,next.js,zod,vite,astro,svelte,vue-core --runs=1 --samples=0
```

| Project | Default | Rolling | Speedup | Coverage drift |
| --- | ---: | ---: | ---: | ---: |
| astro | 695ms | 499ms | 1.39x | 0 missing / 0 extra |
| next.js | 13.06s | 6.36s | 2.05x | 3 missing / 3 extra |
| svelte | 281ms | 204ms | 1.38x | 0 missing / 0 extra |
| typescript | 14.33s | 7.67s | 1.87x | 0 missing / 0 extra |
| vite | 239ms | 137ms | 1.75x | 0 missing / 0 extra |
| vue-core | 129ms | 62ms | 2.06x | 0 missing / 1 extra |
| zod | 48ms | 24ms | 2.02x | 0 missing / 0 extra |

Decision update: rolling is not a direct default replacement yet. It is now faster on TypeScript after the uniform-extension fix and remains promising on `next.js`, `vite`, and `zod`. The private path now uses a component-heavy corpus fallback for Astro/Svelte-style projects, which removes the largest component-corpus coverage drift while preserving the main JS/TS speed wins. The latest one-run Svelte sample is faster again after lazy child-map allocation, but smaller mixed component repos can still have low single-digit drift, so this remains private.

Latest drift diagnosis: the component-corpus extras are mostly broad lexical parent pairs inside `.astro` and `.svelte` markup regions, not explicit section-boundary crossings. Examples include Astro footer/header markup where rolling emits a broad line range that the suffix path decomposes into smaller groups, and Svelte formatted-vs-flattened markup fixtures where rolling reports broad template spans that default output does not cover. This points at rolling pair emission and subset ordering rather than token namespace leakage.

Latest Next.js sampled-repro diagnosis: `benchmarks/compare-dupes-rolling.mjs --write-repros=/tmp/fallow-rolling-repros --samples=8 --projects=next.js` copies the sampled drift files into a small repro project. In that sampled project, the ReactRefresh missing group disappears, so that miss depends on files outside the bounded sample. The sampled project still reproduces the `next-after-app-api-usage` line-shifted miss and the `large-shell/[slug]/page.tsx` broad rolling extra. That makes `next-after` and `large-shell` better next parity targets than ReactRefresh.

### jscpd v5 algorithm

jscpd v5 npm package is a small JS wrapper around a native Rust binary, for example `cpd-darwin-arm64/cpd-bin/cpd`.

Relevant upstream code:

- `rust/crates/cpd-core/src/detect.rs`
- `rust/crates/cpd-finder/src/orchestrate.rs`
- `rust/crates/cpd-tokenizer/src/tokenizer.rs`

Algorithm shape:

- Pre-hash detection tokens during tokenization.
- Group prepared sources by format.
- For each format group, scan rolling windows of length `min_tokens`.
- Store first occurrences in an `FxHashMap<u64, Occurrence>`.
- Verify hash hits by comparing the underlying token hash slice.
- Maintain one `open_clone` for contiguous matches.
- Flush one clone per contiguous region.
- Use a capped secondary occurrence pass for repeated windows.

This is why jscpd can process a large corpus without building the broad intermediate set fallow builds.

### Things ruled out

`--top 20` does not improve fallow runtime materially. The expensive candidate extraction already happened before truncation.

`--min-occurrences 3` does not improve fallow runtime materially. It cuts report volume, but too late.

The persistent token cache helps only slightly on the Next.js fixture. Tokenization is not the dominant cost.

jscpd with `--workers 1` remains much faster than fallow. The gap is not mainly parallelism.

Equalizing `--max-size` did not explain the Next.js gap.

Adding `tsx,jsx` to the jscpd benchmark command increases jscpd's workload, but it remains very fast. The benchmark should include those formats for fairness.

### Follow-up extraction experiments

Kept changes:

- Only push LCP stack entries when `cur_lcp >= min_tokens` and the LCP rises above the previous stack top. This reduces zero and flat plateau work without changing default compact output on `next.js`.
- Add a two-instance raw group fast path in `build_raw_group`. This skips sort and dedupe allocation for the common pair case while preserving default compact output on `next.js`.
- Skip per-instance boundary prefix checks when no file in the analyzed corpus has section-boundary tokens. This preserves default compact output on `next.js` and helps JS-only projects avoid an unnecessary hot-loop branch.
- Add `benchmarks/compare-dupes-rolling.mjs` for default-vs-rolling timing, exact compact drift, line-coverage drift, and optional bounded drift samples via `--samples=N`. Current smoke result: `zod` remains clean, while `next.js` is faster with rolling but still reports coverage drift, so rolling remains private.
- Replace the rolling prototype's per-offset `min_tokens` hash scan with O(1) rolling hash updates and skip boundary checks in rolling recursion when the corpus has no section-boundary tokens. This keeps `zod` clean and improves the private prototype, but does not solve the remaining `next.js` coverage drift.
- Let rolling seed exploration continue when a same-file pair can extend until its ranges become adjacent, and check past intervening same-file occurrences with a bounded pair guard. This fixes adjacent repeated-block misses without the broad root-seed relaxation that made `zod` pathological. Current `next.js` smoke drift is down to low single-digit coverage misses/extras, but rolling still remains private.
- Reuse the ordered occurrence invariant in rolling raw-group helpers instead of allocating and sorting temporary `(file_id, offset)` vectors. This preserves the current `next.js` drift profile and keeps `zod` exact while reducing private-path overhead.
- Add a rolling two-occurrence extension fast path that scans the pair directly instead of allocating child maps one token at a time. This preserves the current drift profile and reduces the private path's raw-extension time on `next.js`.
- Hoist rolling pair-extension boundary-prefix lookups out of the per-token loops and use a no-boundary fast path per pair. This preserves the current `next.js` and `zod` drift profile while improving the private rolling path's repeated cold-scan median.
- Replace the recursive uniform multi-occurrence extension path with an iterative loop. The previous recursive shape overflowed the stack on the TypeScript fixture; the iterative version preserves output and lets the broader real-world comparison complete.
- Fast-forward uniform multi-occurrence extension by scanning the common next token directly instead of rebuilding child maps one token at a time. This preserves the TypeScript drift profile and changes the repeated TypeScript rolling median from slower than default to faster than default.
- Store rolling seed classes as a single occurrence until a duplicate window is found. This mirrors jscpd's first-hit storage shape more closely, avoids a `Vec` allocation for every unique rolling window, preserves the current drift profile, and improves the private rolling path on the main real-project probes.
- Store rolling child buckets as a single occurrence until a duplicate next-token child is found. This avoids allocating a `Vec` for singleton children in recursive raw-group expansion, preserves the current drift profile, and improves the private rolling path on the broad real-project smoke.
- Make rolling left-maximal overlap counts lazy after the adjacency checks. This avoids two non-overlap scans when adjacency already proves a seed left-maximal, preserves the current drift profile, and improves the private rolling path's within-run speedup on the main probes.
- Preallocate recursive rolling child hash maps up to a capped size. This reduces hash-map growth in the child-grouping hot path, preserves the current drift profile, and improves most broad real-project probes despite noisy `next.js` wall time.
- Reuse the component-boundary scan result when entering the private rolling path. This avoids rescanning JS-only corpora to discover that no boundary prefixes are needed, preserves the current drift profile, and improves the repeated `next.js` rolling median.
- Add a private hybrid fallback for component-heavy corpora based on the share of files with section-boundary tokens. This keeps rolling active for large JS/TS corpora such as `next.js` and `typescript`, while falling back to the suffix detector for Astro/Svelte-style corpora where rolling emitted broad markup parent pairs.
- Skip suffix-extraction boundary-prefix construction for corpora whose file extensions cannot produce boundary tokens. This is a conservative cleanup for plain JS/TS projects; it does not move the dominant `next.js` extraction bottleneck by itself.
- Combine rolling's exact-adjacent same-file check and bounded extends-to-adjacency scan into one pass. This preserves the current drift profile, avoids a duplicate same-file scan inside left-maximality, and improved the broad real-project smoke on the largest probes.
- Add `--write-repros=<DIR>` to `benchmarks/compare-dupes-rolling.mjs`. It copies the sampled missing and extra drift files into a small project, decodes compact-output path escapes such as `%5Bslug%5D`, and lets the next parity loop test reduced repros before changing rolling semantics.
- Store rolling hash buckets as a single `SeedClass` until a real hash collision creates a second distinct seed class. This avoids allocating a `Vec<SeedClass>` for the common one-class rolling hash bucket, preserves the current drift profile, and improves the repeated `next.js` and `zod` rolling medians.
- Consume rolling seed buckets directly when building raw groups instead of first materializing a separate `Vec<SeedClass>`. This preserves the current drift profile, removes an intermediate allocation and traversal, and improves the repeated main probes.
- Lazily allocate recursive rolling child maps only after seeing a second extendable child occurrence. Groups with zero or one extendable child cannot recurse, so this avoids needless hash-map work, preserves the current drift profile, and improves the repeated main probes.

Rejected changes:

- Capping raw-group temporary vector capacity at 16 regressed `next.js` extraction and build time because large intervals reallocated heavily.
- In-place dedupe of sorted raw instances preserved output but regressed `next.js` build time.
- A special exact two-suffix interval helper preserved output but regressed `next.js` extraction time.
- Lazy allocation for the first two valid instances preserved output but regressed `next.js` extraction time. The added hot-loop branching outweighed avoided allocations.
- Relaxing rolling child left-maximality did not reduce `next.js` drift and made rolling slower.
- Suppressing rolling parent seed groups when extendable children exist made `next.js` faster but lost too much default coverage, so it is not acceptable as a parity path.
- Caching per-file token lengths for the suffix extraction hot loop preserved output but was neutral to slightly worse on `next.js`, so it was not kept.
- Sorting rolling seed classes and child groups made comparisons deterministic but slowed the private path on `next.js`, so it was not kept in the speed-focused prototype.
- Replacing the bounded same-file pair guard with a distance-window guard kept the speed gain but worsened `next.js` coverage drift, so the pair guard remains the better private prototype.
- Recursing into every extendable rolling child class after a split preserved the current `next.js` drift but regressed `zod`, so the child left-maximality guard remains.
- Raising the rolling adjacent-pair lookahead cap from 512 to 4096 did not reduce the remaining `next.js` drift and slowed the private path, so the lower cap remains.
- Splitting suffix extraction into a separate no-boundary hot loop and hoisting per-file token lengths preserved output but regressed the measured `next.js` profile, so the current single loop remains.
- Treating adjacent line intervals as continuous in `remove_line_subsets` removed one rolling-only same-file extra, but it also changed default `next.js` output and worsened rolling parity, so the shared filter stays unchanged.
- Special-casing three-occurrence rolling child splitting preserved the current drift profile but slowed the private path, so the hash-map child splitter remains.
- Suppressing rolling pair groups that share starts with near-length multi-instance raw groups fixed the ReactRefresh raw-group suppression locally, but it caused broad `next.js` drift and slowed the private path, so rolling pair groups stay unpruned.
- Sorting rolling token-subset filtering to prefer near-length multi-instance groups over pairs reduced one local suppression mode, but it caused broad `next.js` drift, so the shared length-descending subset order remains.
- Preallocating raw-group output vectors in suffix extraction and rolling preserved output, but the measured `next.js` profile was not better, so the default `Vec::new()` growth remains.
- Reordering the rolling left-maximal checks to run the count-based condition before the bounded adjacency-extension search preserved drift, but the repeated `next.js` rolling median regressed, so the original boolean order remains.
- Preallocating the rolling seed bucket map by total window count matched jscpd's store-sizing shape, but the repeated `next.js` rolling median regressed and exact drift changed without improving coverage drift, so the default growing map remains.
- Recursing into split child groups with at least four occurrences and an adjacent same-file pair did not reduce the remaining `next.js` coverage drift and slowed the repeated `next.js` rolling median, so the stricter child left-maximal gate remains.
- Specializing uniform multi-occurrence extension for no-boundary corpora removed boundary checks from the hot loop, but repeated real-project comparison regressed TypeScript and Vite versus the previous rolling medians, so the shared `next_extension_token` path remains.
- Adding a three-instance suffix-extraction fast path preserved focused duplicate tests, but the interval-pressure benchmark regressed from the current baseline, so the generic `sort_unstable` plus dedupe path remains.
- Returning only occurrence vectors from rolling seed building removed unused key fields after duplicate-window discovery, but repeated real-project comparison regressed the private rolling medians, especially TypeScript, so the post-filter `SeedClass` shape remains.
- Removing rolling-only same-file bridge groups eliminated the small extra-coverage samples on `next.js` and `vue-core`, but it also caused broad missing coverage (`next.js`: 789 coverage misses, `vue-core`: 186 coverage misses), so rolling keeps those bridge groups.
- Replacing recursive rolling child hash maps with linear buckets for small occurrence sets helped some small fixtures, but worsened the main large probes (`next.js` and TypeScript) at both 8- and 4-occurrence cutoffs, so child grouping keeps the hash-map path plus single-occurrence bucket storage.
- Replacing the no-boundary prefix vector with an empty slice removed a small allocation, but regressed the main broad real-project probes. The rolling path keeps a per-file `None` prefix vector so indexing shape stays uniform.
- Returning early from rolling left-maximality for two-occurrence classes after the previous-token and adjacency checks preserved the current drift profile, but repeated `next.js` and `zod` comparison did not improve the rolling median, so the count fallback remains unchanged.
- Replacing rolling seed bucket `get_mut` plus `insert` with the hash map entry API preserved drift but did not improve the repeated `next.js` and `zod` medians, so the previous insertion shape remains.
- Combining the two rolling non-overlap count scans inside left-maximality into a single pass preserved the current drift profile, but repeated `next.js` comparison regressed the rolling median, so the separate scans remain.
- Specializing rolling left-maximality for two-occurrence seed classes preserved the current drift profile, but repeated `next.js` comparison regressed the rolling median, so the generic path remains.
- Storing each rolling occurrence's previous token rank improved the locality of the left-maximal prefix check in theory, but it enlarged the occurrence record and regressed the repeated `next.js` rolling median while preserving the same drift, so occurrences remain `(file_id, offset)` only.
- Specializing rolling raw-group push for two-occurrence groups preserved the current drift profile, but repeated `next.js` and `zod` comparison did not improve the rolling median, so the generic dedupe helper remains.
- Splitting rolling recursive child-bucket collection into boundary and no-boundary helper functions preserved the current drift profile, but repeated `next.js` comparison regressed the rolling median, so the inline child scan remains.

## Benchmark correction needed

Current comparator commands should not use only:

```bash
--format typescript,javascript
```

For JS/TS web projects, use:

```bash
--format typescript,javascript,tsx,jsx
```

Otherwise jscpd skips `.tsx` and `.jsx` while fallow includes them.

Keep the existing benchmark guard in `benchmarks/bench-dupes.mjs` that checks `node_modules/jscpd/package.json` against `package-lock.json`. The previous stale local install made `jscpd 4.2.5` look much slower than the locked `5.0.10`.

## Implementation plan

### 1. Add a fair benchmark mode for jscpd v5

Goal: prevent bad conclusions before changing code.

Work:

- Update `benchmarks/bench-dupes.mjs` jscpd args to include `tsx,jsx`.
- Consider moving the format list to a named constant, for example `JS_WEB_FORMATS`.
- Keep version mismatch guard.
- Record both time and tool-specific duplication volume.
- Label clone groups and duplication percentage as tool-specific metrics, not equivalent recall.

Validation:

- `node benchmarks/bench-dupes.mjs --real-world --runs=1 --warmup=0 --projects=zod`
- One heavier spot check on `next.js`.

### 2. Add a fallow duplicate detection benchmark that isolates extraction

Goal: make the bottleneck measurable in CI or local perf loops.

Work:

- Add or extend a benchmark around `CloneDetector::detect`.
- Include a fixture pattern that reproduces high LCP interval pressure.
- Track at least:
  - tokens
  - raw groups
  - final clone groups
  - extraction time
  - total detection time

Validation:

- `cargo check -p fallow-core --benches`
- Targeted benchmark run against the new detector benchmark.

### 3. Prototype a rolling-window detector path

Goal: test whether jscpd's algorithm class fits fallow's output requirements.

Work:

- Add an internal alternative detector behind a private config gate or feature flag.
- Reuse fallow's current tokenization and normalization first.
- Group streams by source namespace or format so JS, style, and markup do not cross-match accidentally.
- Use rolling `min_tokens` hashes and verify token slices on hash hits.
- Emit maximal contiguous clone regions instead of every LCP interval.
- Preserve current `CloneGroup` output shape.

Validation:

- Existing duplicate tests.
- New tests for:
  - identical files
  - overlapping same-file clones
  - 3+ repeated files
  - `.vue`, `.svelte`, `.astro` section boundaries
  - CSS or style token boundaries
  - import-ignore behavior
  - `skip_local`
  - `min_lines`

Risk:

- jscpd's model emits pair-like clones. Fallow groups all instances into richer clone groups. The prototype must either group equivalent pair clones or accept a changed grouping model only with explicit review.

### 4. Compare detector semantics before optimizing further

Goal: avoid a faster but worse detector.

Work:

- Run current suffix-array detector and rolling prototype on the same fixture set.
- Compare:
  - total duplicated lines
  - final clone groups
  - sampled findings
  - same-file overlap behavior
  - repeated multi-file clone grouping
  - false-positive noise from very common token windows

Decision:

- Keep rolling detector only if it materially improves large-project runtime and preserves useful fallow findings.
- If grouping quality drops, consider a hybrid approach:
  - rolling windows find candidate regions
  - current grouping and reporting logic build final clone groups from those candidates

### 5. Optimize current LCP extraction only if rolling prototype is too risky

Fallback path:

- Add earlier interval pruning in `extract_clone_groups`.
- Avoid building raw groups that are clearly contained in already-covered intervals.
- Push line and token coverage checks closer to extraction.
- Cap pathological repeated-window intervals before `RawGroup` allocation.

Expected impact:

- Smaller than a rolling detector rewrite, but lower semantic risk.

## Proposed priority

1. Fix benchmark fairness for `tsx,jsx`.
2. Add an isolated benchmark for the extraction bottleneck.
3. Prototype rolling-window detection behind a private switch.
4. Compare semantics and performance on `zod`, `svelte`, `vue-core`, `query`, `vite`, `next.js`, and `typescript`.
5. Decide between rolling detector, hybrid candidate detector, or targeted LCP extraction pruning.

## Current working hypothesis

The best long-term direction is not to micro-optimize suffix-array construction. The measured hot path is LCP interval extraction and raw-group materialization. A candidate-first rolling-window detector, possibly feeding fallow's existing grouping/reporting surface, is the most promising route to close the jscpd gap.
