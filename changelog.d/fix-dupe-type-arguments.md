### Fixed

- **`fallow dupes` compares the type arguments of calls and `new` expressions.** The tokens of `check<Omit<A, "a">>()` were the same as the tokens of `check()`, and `new Map<string, A>()` gave the tokens of `new Map()`. So calls that differ only in their type arguments were reported as clones, for example the `expectTypeOf<...>()` assertions of a type test file. These type arguments now give tokens, the same as the type arguments of a type alias. Clone groups in code with many generic calls can split or end, and a real copy with generic calls can now reach `minTokens`. With `crossLanguage`, type arguments are still removed. The duplication token cache is rebuilt once.
  (Closes [#5401](https://github.com/fallow-rs/fallow/issues/5401))
