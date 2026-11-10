# Type-aware corpus summary

Gate: GO

- Candidates: 466
- Confirmation precision: 1
- Confirmed as used: 121 of 466 candidates (confirmation yield 0.259657)
- Checked confirmation yield (gate): 0.339888 over 356 checked candidates
- Contract-preserved false positives: 57
- Combined safe-resolution yield: 0.381974
- Independently adjudicated truth coverage: 0.386266
- Correct-unused retention: 1
- Abstention: 0.291845
- Candidates behind blocking diagnostics: 110 (share 0.236052)
- Candidates without type evidence (abstained or unresolved): 288 (share 0.618026)
- Median marginal overhead: 275.835 ms
- P95 marginal overhead: 3630.626 ms
- Independent repositories with confirmed value: 6
- Semantic buckets with confirmed value: callback-inference, exact-cross-file-symbol-use

No corpus-wide recall is claimed. Indeterminate candidates are excluded from adjudicated accuracy metrics.

Blocking diagnostics come from missing dependencies, which the corpus does not install, and from TypeScript 7 configuration incompatibility. Candidates in a program with blocking diagnostics have no type evidence and are excluded from the checked confirmation yield:

- svelte: 79 blocked candidates; `packages/svelte/tsconfig.json` (7 diagnostics)
- astro: 5 blocked candidates; `packages/astro/tsconfig.test.json` (1 diagnostics), `packages/language-tools/language-server/tsconfig.build.json` (1 diagnostics)
- next.js: 25 blocked candidates; `tsconfig.json` (2 diagnostics)
- preact: 1 blocked candidates; `<inferred>` (1 diagnostics), `demo/tsconfig.json` (2 diagnostics)

Paths that both corpus runs ignore:

- zod: `deno/lib/**`. deno/lib is a copy of src that the build:deno script generates for deno.land/x. Its class members are public API there, but no package.json entry declares that.

Raw machine output and source evidence remain under target/type-aware-corpus and are not tracked.
