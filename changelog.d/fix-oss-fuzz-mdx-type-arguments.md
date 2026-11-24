### Fixed

- **A crafted TypeScript or MDX file with nested `<` and parenthesized patterns can no longer make the analysis hang.** The TypeScript parser tries `a<` as the start of type arguments, and it tries a `(` before `{`, `[` or `(` as the parameters of an arrow function. Each nested `r<({[` thus tripled the parse work, and a few hundred bytes kept the parser busy for minutes while the file stayed under the nesting limit. Fallow now counts such a `(` as one more nesting level, so the file gets a failed parse instead. Real code stays far below the limit.
