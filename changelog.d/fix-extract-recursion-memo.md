### Fixed

- **A small source file with repeated or nested references no longer makes a run use all memory or run without end.** Five places did the same work again for each reference, so a short file could make the work grow exponentially or with the square of its size. Each place now keeps a memo or has a limit, and a value above a limit gets the result of an unknown value:
  - The component contract pass resolves the keys of each spread `const` object once. An object with more than 512 keys, or a spread chain deeper than the limit, gets no spread binding.
  - The StyleX consumer scan checks each constant once per resolution, with a step budget and a depth limit. A value above a limit is not static.
  - The duplication tokenizer tokenizes the parameter patterns of a function nested 4 or more parameter lists deep once instead of twice.
  - The MDX scan probes an open `import` or `export` statement at most 256 times. A longer statement takes lines until the next top-level statement, and the final check of the statement classifies it.
  - An extglob include pattern with more than 32 negated groups, for example in `import.meta.glob`, gets no exclude glob and matches more paths.

  The extraction and duplication cache versions change, so the first run after an upgrade parses all files again.
