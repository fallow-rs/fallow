### Fixed

- **`fallow mutation` keeps the arguments of compile-time calls as written.** A file that calls `feature()` from `bun:bundle` failed the baseline with "feature() argument must be a string literal", because the instrumented copy replaced the string with a mutant switch. The arguments of `feature()` from `bun:bundle`, of a Bun macro call and of `import.meta.glob()` now get no mutant. The condition around the call keeps its mutants. (#5285)
