### Fixed

- **`fallow mutation --max-mutants N` now runs up to N covered mutants.** The plan applies the limit before the dry run knows the coverage. On a large project, all N best-ranked mutants could have no covering test, so the run executed no mutant and showed no score. Now an uncovered planned mutant is `no_coverage` and does not use the limit. The next covered `pending` mutant in rank order runs in its place. The `mutant-limit` completion reason now appears only when a covered mutant stays `pending`.
