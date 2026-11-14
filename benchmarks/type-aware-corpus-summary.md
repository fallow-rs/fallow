# Type-aware corpus summary

Gate: GO

- Candidates: 466
- Confirmation precision: 1
- Confirmed as used: 167 of 466 candidates (confirmation yield 0.358369)
- Checked confirmation yield (gate): 0.383908 over 435 checked candidates
- Contract-preserved false positives: 73
- Combined safe-resolution yield: 0.515021
- Independently adjudicated truth coverage: 0.519313
- Correct-unused retention: 1
- Abstention: 0.328326
- Candidates behind blocking diagnostics: 31 (share 0.066524)
- Candidates without type evidence (abstained or unresolved): 226 (share 0.484979)
- Median marginal CPU overhead: 485 ms
- P95 marginal CPU overhead (gate): 3577 ms
- P95 marginal wall overhead: 2242.973 ms
- Independent repositories with confirmed value: 7
- Semantic buckets with confirmed value: callback-inference, exact-cross-file-symbol-use

No corpus-wide recall is claimed. Indeterminate candidates are excluded from adjudicated accuracy metrics.

Blocking diagnostics come from missing dependencies, which the corpus does not install, and from TypeScript 7 configuration incompatibility. Candidates in a program with blocking diagnostics have no type evidence and are excluded from the checked confirmation yield:

- astro: 5 blocked candidates; `packages/astro/tsconfig.test.json` (1 diagnostics), `packages/language-tools/language-server/tsconfig.build.json` (1 diagnostics)
- next.js: 25 blocked candidates; `tsconfig.json` (2 diagnostics)
- preact: 1 blocked candidates; `<inferred>` (1 diagnostics), `demo/tsconfig.json` (2 diagnostics)

Paths that both corpus runs ignore:

- zod: `deno/lib/**`. deno/lib is a copy of src that the build:deno script generates for deno.land/x. Its class members are public API there, but no package.json entry declares that.

Raw machine output and source evidence remain under target/type-aware-corpus and are not tracked.
