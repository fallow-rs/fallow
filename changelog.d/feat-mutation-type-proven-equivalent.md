### Added

- **`fallow mutation --typecheck` now marks an optional-chaining mutant as `equivalent` when the types prove it.** The mutants `a?.b` to `a.b`, `a?.()` to `a()` and `a?.[k]` to `a[k]` change nothing when the type of `a` excludes `null` and `undefined`. The mutant gets the status `equivalent` and `equivalent_reason: "type-proven-never-nullish"`, and it leaves the score. The proof needs `strictNullChecks` and `noUncheckedIndexedAccess`. It does not hold for `any`, `unknown`, a type parameter, a cast or a non-null assertion. `typecheck.equivalent` counts the proven mutants.
